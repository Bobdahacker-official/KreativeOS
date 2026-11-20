//! NVMe PCIe transport with polled admin and NVM queues.
use crate::{
    hardware::{millis, pci, pci_write},
    storage::{identity_mmio, marked_volume, MARKER_LBA, WRITE_LBAS},
    text::Text,
};
use core::{
    ptr,
    sync::atomic::{fence, Ordering},
};

const PAGE: usize = 4096;
const PRPS: usize = 4 * PAGE;
const DATA: usize = 5 * PAGE;
pub const PAGES: usize = 18;
const MAX_BYTES: usize = PAGES * PAGE - DATA;
const CC: usize = 0x14;
const CSTS: usize = 0x1c;
const READY: u32 = 1;
const FATAL: u32 = 2;
const SECTOR: usize = 512;
const MIN_SECTORS: u64 = 16384;

pub static mut DMA: usize = 0;

#[derive(Clone, Copy)]
struct Queue {
    id: u16,
    depth: u16,
    sq: usize,
    cq: usize,
    tail: u16,
    head: u16,
    cid: u16,
    phase: u16,
}

impl Queue {
    fn new(id: u16, depth: u16) -> Self {
        let offset = usize::from(id) * 2 * PAGE;
        Self {
            id,
            depth,
            sq: offset,
            cq: offset + PAGE,
            tail: 0,
            head: 0,
            cid: 0,
            phase: 1,
        }
    }
}

pub struct Disk {
    base: usize,
    stride: usize,
    ready_ms: u64,
    admin: Queue,
    io: Queue,
    max_bytes: usize,
    healthy: bool,
    pub bdf: (u8, u8, u8),
    pub namespace: u32,
    pub sectors: u64,
    pub model: Text<40>,
}

type Command = [u32; 16];

fn command(opcode: u8, nsid: u32) -> Command {
    let mut cmd = [0; 16];
    cmd[0] = u32::from(opcode);
    cmd[1] = nsid;
    cmd
}

fn wait(timeout: u64, mut condition: impl FnMut() -> bool) -> bool {
    let start = millis();
    while !condition() {
        if millis() - start >= timeout {
            return false;
        }
        core::hint::spin_loop();
    }
    true
}

unsafe fn read(base: usize, offset: usize) -> u32 {
    ptr::read_volatile((base + offset) as *const u32)
}

unsafe fn write(base: usize, offset: usize, value: u32) {
    ptr::write_volatile((base + offset) as *mut u32, value);
}

fn pointer(cmd: &mut Command, index: usize, address: usize) {
    cmd[index] = address as u32;
    cmd[index + 1] = (address >> 32) as u32;
}

impl Disk {
    unsafe fn submit(&mut self, admin: bool, mut cmd: Command) -> bool {
        if !self.healthy {
            return false;
        }
        let q = if admin { &mut self.admin } else { &mut self.io };
        let cid = q.cid;
        q.cid = q.cid.wrapping_add(1);
        cmd[0] |= u32::from(cid) << 16;
        let slot = (DMA + q.sq + usize::from(q.tail) * 64) as *mut Command;
        slot.write_volatile(cmd);
        q.tail = (q.tail + 1) % q.depth;
        fence(Ordering::SeqCst);
        let doorbell = 0x1000 + usize::from(q.id) * self.stride * 2;
        write(self.base, doorbell, u32::from(q.tail));

        let entry = (DMA + q.cq + usize::from(q.head) * 16) as *const u32;
        let mut result = 0;
        if !wait(if admin { self.ready_ms } else { 3000 }, || {
            result = entry.add(3).read_volatile();
            (result >> 16) as u16 & 1 == q.phase || read(self.base, CSTS) & FATAL != 0
        }) || read(self.base, CSTS) & FATAL != 0
        {
            self.healthy = false;
            return false;
        }
        fence(Ordering::SeqCst);
        let sq = entry.add(2).read_volatile();
        if result as u16 != cid || (sq >> 16) as u16 != q.id || sq as u16 != q.tail {
            self.healthy = false;
            return false;
        }
        q.head += 1;
        if q.head == q.depth {
            q.head = 0;
            q.phase ^= 1;
        }
        write(self.base, doorbell + self.stride, u32::from(q.head));
        (result >> 17) & 0x7ff == 0
    }

    unsafe fn identify(&mut self, nsid: u32, cns: u32) -> Option<&'static [u8; PAGE]> {
        ptr::write_bytes((DMA + DATA) as *mut u8, 0, PAGE);
        let mut cmd = command(0x06, nsid);
        pointer(&mut cmd, 6, DMA + DATA);
        cmd[10] = cns;
        self.submit(true, cmd)
            .then(|| &*((DMA + DATA) as *const [u8; PAGE]))
    }

    unsafe fn start(&mut self) -> bool {
        if !self.stop() {
            return false;
        }
        ptr::write_bytes(DMA as *mut u8, 0, PAGES * PAGE);
        write(self.base, 0x0c, u32::MAX); // Poll completions with interrupts masked.
        let depth = u32::from(self.admin.depth) - 1;
        write(self.base, 0x24, depth | (depth << 16));
        write(self.base, 0x28, (DMA + self.admin.sq) as u32);
        write(self.base, 0x2c, 0);
        write(self.base, 0x30, (DMA + self.admin.cq) as u32);
        write(self.base, 0x34, 0);
        fence(Ordering::SeqCst);
        // NVM command set, 4 KiB pages, 64-byte SQEs and 16-byte CQEs.
        write(self.base, CC, 1 | (6 << 16) | (4 << 20));
        if !wait(self.ready_ms, || {
            read(self.base, CSTS) & (READY | FATAL) != 0
        }) || read(self.base, CSTS) & (READY | FATAL) != READY
        {
            return false;
        }
        let Some(identity) = self.identify(0, 1) else {
            return false;
        };
        let model = identity[24..64].map_ascii();
        self.model.set(model.str().trim());
        let mdts = identity[77];
        if mdts != 0 {
            self.max_bytes = PAGE
                .checked_shl(u32::from(mdts))
                .unwrap_or(MAX_BYTES)
                .min(MAX_BYTES)
                .max(PAGE);
        }
        // Request one pair of I/O queues before creating them.
        let mut feature = command(0x09, 0);
        feature[10] = 7;
        if !self.submit(true, feature) {
            return false;
        }
        let parameters = u32::from(self.io.id) | ((u32::from(self.io.depth) - 1) << 16);
        let mut cq = command(0x05, 0);
        pointer(&mut cq, 6, DMA + self.io.cq);
        cq[10] = parameters;
        cq[11] = 1; // Physically contiguous queue, interrupt disabled.
        if !self.submit(true, cq) {
            return false;
        }
        let mut sq = command(0x01, 0);
        pointer(&mut sq, 6, DMA + self.io.sq);
        sq[10] = parameters;
        sq[11] = 1 | (u32::from(self.io.id) << 16);
        self.submit(true, sq)
    }

    unsafe fn stop(&self) -> bool {
        write(self.base, CC, read(self.base, CC) & !1);
        wait(self.ready_ms, || read(self.base, CSTS) & READY == 0)
    }

    unsafe fn marked(&mut self, nsid: u32) -> bool {
        let Some(identity) = self.identify(nsid, 0) else {
            return false;
        };
        let sectors = u64::from_le_bytes(identity[..8].try_into().unwrap());
        let format = identity[26];
        let index = usize::from((format & 15) | ((format & 0x60) >> 1));
        let lbaf = 128 + index * 4;
        if sectors < MIN_SECTORS
            || index > usize::from(identity[25])
            || identity[lbaf..lbaf + 3] != [0, 0, 9]
            || identity[29] & 7 != 0
        {
            return false;
        }
        self.namespace = nsid;
        self.sectors = sectors;
        let mut label = [0; SECTOR];
        let mut first = [0; SECTOR];
        self.transfer(MARKER_LBA, &mut label, false)
            && self.transfer(0, &mut first, false)
            && marked_volume(&label, &first)
    }

    unsafe fn find_namespace(&mut self) -> bool {
        let mut after = 0;
        // A bounded scan, with pagination for controllers with many namespaces.
        for _ in 0..4 {
            let Some(list) = self.identify(after, 2) else {
                // Active namespace lists were introduced after NVMe 1.0.
                return after == 0
                    && self.healthy
                    && (read(self.base, 8) >> 8 == 0x100)
                    && self.marked(1);
            };
            let mut ids = [0u32; PAGE / 4];
            for (id, bytes) in ids.iter_mut().zip(list.chunks_exact(4)) {
                *id = u32::from_le_bytes(bytes.try_into().unwrap());
            }
            for id in ids {
                if id <= after || id == u32::MAX || !self.healthy {
                    return false;
                }
                if self.marked(id) {
                    return true;
                }
                after = id;
            }
        }
        false
    }

    pub unsafe fn transfer(&mut self, lba: u32, data: &mut [u8], store: bool) -> bool {
        let end = u64::from(lba) + (data.len() / SECTOR) as u64;
        if !self.healthy
            || data.is_empty()
            || data.len() % SECTOR != 0
            || end > self.sectors
            || (store && (!WRITE_LBAS.contains(&lba) || end > u64::from(WRITE_LBAS.end)))
        {
            return false;
        }
        let buffer = (DMA + DATA) as *mut u8;
        let mut position = u64::from(lba);
        for chunk in data.chunks_mut(self.max_bytes) {
            let blocks = chunk.len() / SECTOR;
            let mut cmd = command(if store { 1 } else { 2 }, self.namespace);
            pointer(&mut cmd, 6, DMA + DATA);
            let pages = chunk.len().div_ceil(PAGE);
            if pages == 2 {
                pointer(&mut cmd, 8, DMA + DATA + PAGE);
            } else if pages > 2 {
                let list = (DMA + PRPS) as *mut u64;
                for i in 1..pages {
                    list.add(i - 1).write((DMA + DATA + i * PAGE) as u64);
                }
                pointer(&mut cmd, 8, DMA + PRPS);
            }
            cmd[10] = position as u32;
            cmd[11] = (position >> 32) as u32;
            cmd[12] = blocks as u32 - 1;
            if store {
                ptr::copy_nonoverlapping(chunk.as_ptr(), buffer, chunk.len());
            }
            if !self.submit(false, cmd) {
                return false;
            }
            if !store {
                ptr::copy_nonoverlapping(buffer, chunk.as_mut_ptr(), chunk.len());
            }
            position += blocks as u64;
        }
        !store || self.submit(false, command(0, self.namespace))
    }
}

trait AsciiModel {
    fn map_ascii(&self) -> Text<40>;
}
impl AsciiModel for [u8] {
    fn map_ascii(&self) -> Text<40> {
        let mut text = Text::new();
        for &byte in self {
            text.push(if (32..127).contains(&byte) {
                byte
            } else {
                b' '
            });
        }
        text
    }
}

pub unsafe fn discover() -> Option<Disk> {
    if DMA == 0 {
        return None;
    }
    for bus in 0..=u8::MAX {
        for slot in 0..32 {
            if pci(bus, slot, 0, 0) == u32::MAX {
                continue;
            }
            let count = if pci(bus, slot, 0, 0x0c) & (1 << 23) != 0 {
                8
            } else {
                1
            };
            for function in 0..count {
                if pci(bus, slot, function, 8) >> 8 != 0x010802 {
                    continue;
                }
                let bar = pci(bus, slot, function, 0x10);
                let mut base = (bar & !15) as usize;
                match bar & 7 {
                    0 => {}
                    4 => base |= (pci(bus, slot, function, 0x14) as usize) << 32,
                    _ => continue,
                }
                if base < 0x100000 || !identity_mmio(base) {
                    continue;
                }
                let pci_flags = pci(bus, slot, function, 4) & 0xffff;
                pci_write(bus, slot, function, 4, pci_flags | 2);
                let low = read(base, 0);
                let high = read(base, 4);
                let depth = (low & 0xffff).min(15) as u16 + 1;
                let stride = 4 << (high & 15);
                let Some(last) = base.checked_add(0x1000 + 3 * stride + 3) else {
                    continue;
                };
                if depth < 2
                    || high & (1 << 5) == 0
                    || high & (15 << 16) != 0
                    || !identity_mmio(base + 0x1000)
                    || !identity_mmio(base + 0x1000 + stride)
                    || !identity_mmio(base + 0x1000 + 2 * stride)
                    || !identity_mmio(last)
                {
                    continue;
                }
                let mut disk = Disk {
                    base,
                    stride,
                    ready_ms: u64::from(((low >> 24) & 255).clamp(1, 30)) * 500,
                    admin: Queue::new(0, depth),
                    io: Queue::new(1, depth),
                    max_bytes: MAX_BYTES,
                    healthy: true,
                    bdf: (bus, slot, function),
                    namespace: 0,
                    sectors: 0,
                    model: Text::new(),
                };
                pci_write(bus, slot, function, 4, pci_flags | 6);
                if disk.start() && disk.find_namespace() {
                    return Some(disk);
                }
                if !disk.stop() {
                    // Never recycle the DMA arena while a controller may still own it.
                    return None;
                }
                pci_write(bus, slot, function, 4, pci_flags);
            }
        }
    }
    None
}
