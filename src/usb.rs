//! Native xHCI root-port enumeration, HID boot reports and USB mass storage BOT.
use crate::{
    hardware::{self, Event},
    storage::{self, Media},
    text::Text,
};
use alloc::{collections::VecDeque, vec::Vec};
use core::{
    ptr,
    sync::atomic::{fence, Ordering},
};

const PAGE: usize = 4096;
const MAX_HOSTS: usize = 4;
const HOST_PAGES: usize = 144;
const DEVICE_PAGES: usize = 12;
const MAX_DEVICES: usize = 6;
const FIRST_DEVICE: usize = 70;
pub const PAGES: usize = MAX_HOSTS * HOST_PAGES;
pub static mut DMA: usize = 0;
static mut HOSTS: Vec<Host> = Vec::new();
static mut EVENTS: VecDeque<Event> = VecDeque::new();
static mut POINTER_ACTIVE: bool = false;
const IOC: u32 = 1 << 5;
const IDT: u32 = 1 << 6;
const FINISHED: u8 = 1;
const SHORT: u8 = 13;
type Trb = [u32; 4];

unsafe fn rd(base: usize, offset: usize) -> u32 {
    ptr::read_volatile((base + offset) as *const u32)
}
unsafe fn wr(base: usize, offset: usize, data: u32) {
    ptr::write_volatile((base + offset) as *mut u32, data)
}
unsafe fn wr64(base: usize, offset: usize, data: u64) {
    wr(base, offset, data as u32);
    wr(base, offset + 4, (data >> 32) as u32);
}
fn trb(pointer: usize, length: u32, flags: u32) -> Trb {
    [pointer as u32, (pointer >> 32) as u32, length, flags]
}
fn wait(ms: u64, mut until: impl FnMut() -> bool) -> bool {
    let start = hardware::millis();
    while !until() {
        if hardware::millis() - start >= ms {
            return false;
        }
        core::hint::spin_loop();
    }
    true
}

struct Ring {
    base: usize,
    next: usize,
    cycle: u32,
}
impl Ring {
    fn new(base: usize) -> Self {
        Self {
            base,
            next: 0,
            cycle: 1,
        }
    }
    unsafe fn push(&mut self, words: Trb) -> usize {
        let at = self.base + 16 * self.next;
        for (i, data) in words.into_iter().enumerate().take(3) {
            wr(at, i * 4, data);
        }
        fence(Ordering::Release);
        wr(at, 12, words[3] | self.cycle);
        self.next += 1;
        if self.next == 255 {
            let link = self.base + 255 * 16;
            wr64(link, 0, self.base as u64);
            wr(link, 8, 0);
            fence(Ordering::Release);
            wr(
                link,
                12,
                (6 << 10) | (1 << 1) | self.cycle | (words[3] & (1 << 4)),
            );
            self.next = 0;
            self.cycle ^= 1;
        }
        at
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Keyboard,
    Mouse,
    BulkIn,
    BulkOut,
}
struct Endpoint {
    dci: u8,
    packet: u16,
    interval: u8,
    interface: u8,
    kind: Kind,
    ring: Ring,
    buffer: usize,
    previous: [u8; 8],
    pending: usize,
}
struct Device {
    slot: u8,
    port: u8,
    area: usize,
    ep0: Ring,
    endpoints: Vec<Endpoint>,
    storage: bool,
}
struct Host {
    op: usize,
    run: usize,
    bells: usize,
    bdf: (u8, u8, u8),
    context: usize,
    ports: u8,
    dma: usize,
    command: Ring,
    event_head: usize,
    event_cycle: u32,
    tag: u32,
    devices: Vec<Device>,
}

impl Host {
    unsafe fn open(base: usize, dma: usize, bdf: (u8, u8, u8)) -> Option<Self> {
        let (b, d, f) = bdf;
        let cap = rd(base, 0);
        let op = base.checked_add((cap & 255) as usize)?;
        let hcs = rd(base, 4);
        let hcs2 = rd(base, 8);
        let hcc = rd(base, 16);
        let bells = base.checked_add((rd(base, 20) & !3) as usize)?;
        let run = base.checked_add((rd(base, 24) & !31) as usize)?;
        let slots = (hcs & 255) as u8;
        let ports = (hcs >> 24) as u8;
        let pads = ((hcs2 >> 27) & 31) | (((hcs2 >> 21) & 31) << 5);
        if cap >> 16 < 0x100
            || slots == 0
            || ports == 0
            || pads > 64
            || !storage::identity_mmio(op)
            || !storage::identity_mmio(op + 0x400 + usize::from(ports) * 16 - 1)
            || !storage::identity_mmio(bells + usize::from(slots) * 4 + 3)
            || !storage::identity_mmio(run + 0x3f)
        {
            return None;
        }
        let mut extended = ((hcc >> 16) as usize) * 4;
        for _ in 0..64 {
            if extended == 0 || !storage::identity_mmio(base + extended + 7) {
                break;
            }
            let entry = rd(base, extended);
            if entry & 255 == 1 {
                wr(base, extended, entry | (1 << 24));
                if !wait(1000, || rd(base, extended) & (1 << 16) == 0) {
                    return None;
                }
                wr(base, extended + 4, rd(base, extended + 4) & 0xe000_0000);
            }
            let next = ((entry >> 8) & 255) as usize * 4;
            if next == 0 {
                break;
            }
            extended += next;
        }
        let pci_flags = hardware::pci(b, d, f, 4) & 0xffff;
        hardware::pci_write(b, d, f, 4, pci_flags | 6 | (1 << 10));
        if !wait(1000, || rd(op, 4) & (1 << 11) == 0) {
            return None;
        }
        wr(op, 0, rd(op, 0) & !5);
        if !wait(500, || rd(op, 4) & 1 != 0) {
            return None;
        }
        wr(op, 0, 2);
        if !wait(1000, || rd(op, 0) & 2 == 0 && rd(op, 4) & (1 << 11) == 0) || rd(op, 8) & 1 == 0 {
            return None;
        }
        ptr::write_bytes(dma as *mut u8, 0, HOST_PAGES * PAGE);
        if pads != 0 {
            let list = dma + 4 * PAGE;
            wr64(dma, 0, list as u64);
            for i in 0..pads as usize {
                wr64(list, i * 8, (dma + (5 + i) * PAGE) as u64);
            }
        }
        wr64(op, 0x30, dma as u64);
        wr64(op, 0x18, (dma + PAGE) as u64 | 1);
        wr(op, 0x38, u32::from(slots));
        let erst = dma + 3 * PAGE;
        let event = dma + 2 * PAGE;
        wr64(erst, 0, event as u64);
        wr(erst, 8, 256);
        wr(run, 0x20, 1); // Clear pending and leave interrupt enable off.
        wr(run, 0x28, 1);
        wr64(run, 0x38, event as u64);
        wr64(run, 0x30, erst as u64);
        fence(Ordering::SeqCst);
        wr(op, 0, 1);
        if !wait(500, || rd(op, 4) & 1 == 0) {
            return None;
        }
        Some(Self {
            op,
            run,
            bells,
            bdf,
            context: if hcc & 4 != 0 { 64 } else { 32 },
            ports,
            dma,
            command: Ring::new(dma + PAGE),
            event_head: 0,
            event_cycle: 1,
            tag: 0,
            devices: Vec::new(),
        })
    }
    unsafe fn event(&mut self) -> Option<Trb> {
        let base = self.dma + 2 * PAGE;
        let address = base + self.event_head * 16;
        let control = rd(address, 12);
        if control & 1 != self.event_cycle {
            return None;
        }
        fence(Ordering::Acquire);
        let event = [rd(address, 0), rd(address, 4), rd(address, 8), control];
        self.event_head = (self.event_head + 1) % 256;
        if self.event_head == 0 {
            self.event_cycle ^= 1;
        }
        wr64(self.run, 0x38, (base + self.event_head * 16) as u64 | 8);
        Some(event)
    }
    unsafe fn handle(&mut self, data: Trb) {
        if (data[3] >> 10) & 63 != 32 {
            return;
        }
        let slot = (data[3] >> 24) as u8;
        let dci = ((data[3] >> 16) & 31) as u8;
        let address = (u64::from(data[1]) << 32 | u64::from(data[0])) as usize;
        let Some(device) = self.devices.iter_mut().find(|device| device.slot == slot) else {
            return;
        };
        let Some(ep) = device.endpoints.iter_mut().find(|ep| {
            ep.dci == dci
                && ep.pending == address
                && matches!(ep.kind, Kind::Mouse | Kind::Keyboard)
        }) else {
            return;
        };
        let code = (data[2] >> 24) as u8;
        if !matches!(code, FINISHED | SHORT) {
            ep.pending = 0;
            return;
        }
        let length = usize::from(ep.packet).saturating_sub((data[2] & 0x00ff_ffff) as usize);
        let mut bytes = [0u8; 8];
        for (i, byte) in bytes.iter_mut().take(length).enumerate() {
            *byte = ptr::read_volatile((ep.buffer + i) as *const u8);
        }
        if EVENTS.len() < 64 && ep.kind == Kind::Mouse && length >= 3 {
            if !POINTER_ACTIVE && (bytes[1] != 0 || bytes[2] != 0 || bytes[0] & 7 != 0) {
                POINTER_ACTIVE = true;
                hardware::log(b"KreativeOS: USB pointer input received\r\n");
            }
            EVENTS.push_back(Event::Mouse(
                i32::from(bytes[1] as i8),
                i32::from(bytes[2] as i8),
                bytes[0] & 7,
                0,
            ));
        } else if ep.kind == Kind::Keyboard
            && length >= 8
            && !bytes[2..8].iter().any(|key| (1..=3).contains(key))
        {
            for &key in &bytes[2..8] {
                if !ep.previous[2..].contains(&key) && EVENTS.len() < 64 {
                    if let Some(code) = hid_key(key, bytes[0]) {
                        EVENTS.push_back(Event::Key(code));
                    }
                }
            }
            ep.previous.copy_from_slice(&bytes[..8]);
        }
        Self::arm(self.bells, slot, ep);
    }
    unsafe fn arm(bells: usize, slot: u8, ep: &mut Endpoint) {
        ep.pending = ep.ring.push(trb(
            ep.buffer,
            u32::from(ep.packet),
            (1 << 10) | IOC | (1 << 2),
        ));
        fence(Ordering::SeqCst);
        wr(bells, usize::from(slot) * 4, u32::from(ep.dci));
    }
    unsafe fn complete(&mut self, kind: u32, ptr: usize, slot: u8, endpoint: u8) -> Option<Trb> {
        let start = hardware::millis();
        loop {
            if let Some(data) = self.event() {
                if (data[3] >> 10) & 63 == kind
                    && (u64::from(data[1]) << 32 | u64::from(data[0])) as usize == ptr
                    && (kind != 32
                        || ((data[3] >> 24) as u8 == slot
                            && ((data[3] >> 16) & 31) as u8 == endpoint))
                {
                    return Some(data);
                }
                self.handle(data);
            }
            if hardware::millis() - start >= 1800 {
                return None;
            }
            core::hint::spin_loop();
        }
    }
    unsafe fn issue(&mut self, command: Trb) -> Option<u8> {
        let pointer = self.command.push(command);
        fence(Ordering::SeqCst);
        wr(self.bells, 0, 0);
        let result = self.complete(33, pointer, 0, 0)?;
        ((result[2] >> 24) as u8 == FINISHED).then_some((result[3] >> 24) as u8)
    }
    unsafe fn control(&mut self, device: &mut Device, setup: [u8; 8]) -> bool {
        let length = u16::from_le_bytes([setup[6], setup[7]]);
        let input = setup[0] & 128 != 0;
        let trt = if length == 0 {
            0
        } else if input {
            3
        } else {
            2
        };
        let address = device.area + 7 * PAGE;
        let parameters = u64::from_le_bytes(setup) as usize;
        device
            .ep0
            .push(trb(parameters, 8, (2 << 10) | IDT | (trt << 16)));
        if length != 0 {
            device.ep0.push(trb(
                address,
                u32::from(length),
                (3 << 10) | (u32::from(input) << 16),
            ));
        }
        let status = device.ep0.push(trb(
            0,
            0,
            (4 << 10) | (u32::from(!input || length == 0) << 16) | IOC,
        ));
        fence(Ordering::SeqCst);
        wr(self.bells, usize::from(device.slot) * 4, 1);
        self.complete(32, status, device.slot, 1)
            .is_some_and(|event| (event[2] >> 24) as u8 == FINISHED)
    }
    unsafe fn enumerate(&mut self, port: u8, portsc: u32, slot: u8, area: usize) -> Option<Device> {
        let speed = (portsc >> 10) & 15;
        if !(1..=4).contains(&speed) {
            return None;
        }
        let max_packet = match speed {
            3 => 64,
            4 => 512,
            _ => 8,
        };
        ptr::write_bytes(area as *mut u8, 0, DEVICE_PAGES * PAGE);
        let input = area + PAGE;
        wr64(self.dma, usize::from(slot) * 8, area as u64);
        wr(input, 4, 3);
        wr(input, self.context, (speed << 20) | (1 << 27));
        wr(input, self.context + 4, u32::from(port) << 16);
        wr(
            input,
            self.context * 2 + 4,
            (3 << 1) | (4 << 3) | (max_packet << 16),
        );
        wr64(input, self.context * 2 + 8, (area + 2 * PAGE) as u64 | 1);
        wr(input, self.context * 2 + 16, 8);
        self.issue(trb(input, 0, (11 << 10) | (u32::from(slot) << 24)))?;
        let mut device = Device {
            slot,
            port,
            area,
            ep0: Ring::new(area + 2 * PAGE),
            endpoints: Vec::new(),
            storage: false,
        };
        let address = area + 7 * PAGE;
        if !self.control(&mut device, [0x80, 6, 0, 1, 0, 0, 8, 0]) {
            return None;
        }
        if speed == 1 {
            let unit = ptr::read_volatile((address + 7) as *const u8);
            if !matches!(unit, 8 | 16 | 32 | 64) {
                return None;
            }
            if unit != 8 {
                ptr::write_bytes(input as *mut u8, 0, PAGE);
                ptr::copy_nonoverlapping(
                    (area + self.context) as *const u8,
                    (input + 2 * self.context) as *mut u8,
                    self.context,
                );
                wr(input, 4, 2);
                let old = rd(input, self.context * 2 + 4);
                wr(
                    input,
                    self.context * 2 + 4,
                    (old & 0xffff) | (u32::from(unit) << 16),
                );
                self.issue(trb(input, 0, (13 << 10) | (u32::from(slot) << 24)))?;
            }
        }
        if !self.control(&mut device, [0x80, 6, 0, 2, 0, 0, 9, 0]) {
            return None;
        }
        let size = usize::from(u16::from_le_bytes([
            ptr::read_volatile((address + 2) as *const u8),
            ptr::read_volatile((address + 3) as *const u8),
        ]));
        if !(9..=PAGE).contains(&size) {
            return None;
        }
        let size_bytes = (size as u16).to_le_bytes();
        if !self.control(
            &mut device,
            [0x80, 6, 0, 2, 0, 0, size_bytes[0], size_bytes[1]],
        ) {
            return None;
        }
        let descriptor = core::slice::from_raw_parts(address as *const u8, size);
        let config = descriptor[5];
        let mut iface = (0, 0, 0, 0);
        let mut offset = 0;
        while offset + 2 <= size {
            let len = usize::from(descriptor[offset]);
            if len < 2 || offset + len > size {
                return None;
            }
            let entry = &descriptor[offset..offset + len];
            offset += len;
            if entry[1] == 4 && len >= 9 {
                iface = if entry[3] == 0 {
                    (entry[2], entry[5], entry[6], entry[7])
                } else {
                    (0, 0, 0, 0)
                };
            } else if entry[1] == 5 && len >= 7 && device.endpoints.len() < 4 {
                let is_in = entry[2] & 128 != 0;
                let Some(kind) = (match (iface.1, iface.2, iface.3, entry[3] & 3, is_in) {
                    (3, 1, 1, 3, true) => Some(Kind::Keyboard),
                    (3, 1, 2, 3, true) => Some(Kind::Mouse),
                    (8, 6, 0x50, 2, true) => Some(Kind::BulkIn),
                    (8, 6, 0x50, 2, false) => Some(Kind::BulkOut),
                    _ => None,
                }) else {
                    continue;
                };
                let id = (entry[2] & 15) * 2 + u8::from(is_in);
                let packet = u16::from_le_bytes([entry[4], entry[5]]) & 0x7ff;
                if id < 2
                    || packet == 0
                    || packet > 1024
                    || device.endpoints.iter().any(|ep| ep.dci == id)
                {
                    continue;
                }
                let i = device.endpoints.len();
                let interval = if matches!(kind, Kind::Keyboard | Kind::Mouse) {
                    if speed <= 2 {
                        entry[6].max(1).ilog2() as u8 + 3
                    } else {
                        entry[6].clamp(1, 16) - 1
                    }
                } else {
                    0
                };
                device.endpoints.push(Endpoint {
                    dci: id,
                    packet,
                    interval,
                    interface: iface.0,
                    kind,
                    ring: Ring::new(area + (3 + i) * PAGE),
                    buffer: area + 7 * PAGE + i * 1024,
                    previous: [0; 8],
                    pending: 0,
                });
            }
        }
        if config == 0 || device.endpoints.is_empty() {
            return None;
        }
        ptr::write_bytes(input as *mut u8, 0, PAGE);
        ptr::copy_nonoverlapping(
            area as *const u8,
            (input + self.context) as *mut u8,
            self.context,
        );
        let entries = device.endpoints.iter().map(|ep| ep.dci).max()?;
        wr(
            input,
            self.context,
            (rd(input, self.context) & 0x07ff_ffff) | (u32::from(entries) << 27),
        );
        let mut adds = 1;
        for ep in &device.endpoints {
            adds |= 1 << ep.dci;
            let ctx = input + self.context * (usize::from(ep.dci) + 1);
            let code = match ep.kind {
                Kind::BulkIn => 6,
                Kind::BulkOut => 2,
                _ => 7,
            };
            wr(ctx, 0, u32::from(ep.interval) << 16);
            wr(
                ctx,
                4,
                (3 << 1) | (code << 3) | (u32::from(ep.packet) << 16),
            );
            wr64(ctx, 8, ep.ring.base as u64 | 1);
            wr(
                ctx,
                16,
                u32::from(ep.packet)
                    | if code == 7 {
                        u32::from(ep.packet) << 16
                    } else {
                        0
                    },
            );
        }
        wr(input, 4, adds);
        self.issue(trb(input, 0, (12 << 10) | (u32::from(slot) << 24)))?;
        if !self.control(&mut device, [0, 9, config, 0, 0, 0, 0, 0]) {
            return None;
        }
        let mut i = 0;
        while i < device.endpoints.len() {
            let kind = device.endpoints[i].kind;
            let iface = device.endpoints[i].interface;
            if matches!(kind, Kind::Keyboard | Kind::Mouse) {
                if !self.control(&mut device, [0x21, 11, 0, 0, iface, 0, 0, 0]) {
                    device.endpoints.remove(i);
                    continue;
                }
            }
            i += 1;
        }
        device.storage = device.endpoints.iter().any(|ep| ep.kind == Kind::BulkIn)
            && device.endpoints.iter().any(|ep| ep.kind == Kind::BulkOut);
        for ep in &mut device.endpoints {
            if matches!(ep.kind, Kind::Keyboard | Kind::Mouse) {
                Self::arm(self.bells, slot, ep);
            }
        }
        Some(device)
    }
    unsafe fn scan(&mut self) {
        for port in 1..=self.ports {
            if self.devices.len() >= MAX_DEVICES {
                break;
            }
            let register = 0x400 + usize::from(port - 1) * 16;
            let state = rd(self.op, register);
            if state & 1 == 0 || self.devices.iter().any(|device| device.port == port) {
                continue;
            }
            if state & 2 == 0 {
                wr(self.op, register, (state & 0x0e00) | (1 << 4));
                if !wait(250, || rd(self.op, register) & 0x12 == 2) {
                    continue;
                }
            }
            let Some(slot) = self.issue(trb(0, 0, 9 << 10)).filter(|&slot| slot != 0) else {
                continue;
            };
            let area = self.dma + (FIRST_DEVICE + self.devices.len() * DEVICE_PAGES) * PAGE;
            if let Some(device) = self.enumerate(port, rd(self.op, register), slot, area) {
                self.devices.push(device);
            } else {
                // A failed disable must keep the reserved area out of subsequent use.
                if self
                    .issue(trb(0, 0, (10 << 10) | (u32::from(slot) << 24)))
                    .is_none()
                {
                    break;
                }
                wr64(self.dma, usize::from(slot) * 8, 0);
            }
        }
    }
    unsafe fn bulk(
        &mut self,
        device: &mut Device,
        input: bool,
        address: usize,
        length: usize,
    ) -> Option<usize> {
        let ep = device
            .endpoints
            .iter_mut()
            .find(|ep| ep.kind == if input { Kind::BulkIn } else { Kind::BulkOut })?;
        let first = length.min(0x10000 - (address & 0xffff));
        let mut td = 0;
        if first < length {
            ep.ring
                .push(trb(address, first as u32, (1 << 10) | (1 << 4)));
            td = ep.ring.push(trb(
                address + first,
                (length - first) as u32,
                (1 << 10) | IOC | if input { 1 << 2 } else { 0 },
            ));
        }
        if first == length {
            td = ep.ring.push(trb(
                address,
                length as u32,
                (1 << 10) | IOC | if input { 1 << 2 } else { 0 },
            ));
        }
        fence(Ordering::SeqCst);
        wr(self.bells, usize::from(device.slot) * 4, u32::from(ep.dci));
        let completion = self.complete(32, td, device.slot, ep.dci)?;
        let result = (completion[2] >> 24) as u8;
        if !matches!(result, FINISHED | SHORT) {
            return None;
        }
        length.checked_sub((completion[2] & 0x00ff_ffff) as usize)
    }
    unsafe fn scsi(
        &mut self,
        device: &mut Device,
        command: &[u8],
        bytes: &mut [u8],
        input: bool,
    ) -> bool {
        self.tag = self.tag.wrapping_add(1);
        let control = device.area + 8 * PAGE;
        let data = device.area + 9 * PAGE;
        if bytes.len() > 3 * PAGE || command.len() > 16 {
            return false;
        }
        let mut cbw = [0u8; 31];
        cbw[..4].copy_from_slice(b"USBC");
        cbw[4..8].copy_from_slice(&self.tag.to_le_bytes());
        cbw[8..12].copy_from_slice(&(bytes.len() as u32).to_le_bytes());
        cbw[12] = if input { 128 } else { 0 };
        cbw[14] = command.len() as u8;
        cbw[15..15 + command.len()].copy_from_slice(command);
        ptr::copy_nonoverlapping(cbw.as_ptr(), control as *mut u8, 31);
        if self.bulk(device, false, control, 31) != Some(31) {
            return false;
        }
        if !bytes.is_empty() {
            if !input {
                ptr::copy_nonoverlapping(bytes.as_ptr(), data as *mut u8, bytes.len());
            }
            if self.bulk(device, input, data, bytes.len()) != Some(bytes.len()) {
                return false;
            }
            if input {
                ptr::copy_nonoverlapping(data as *const u8, bytes.as_mut_ptr(), bytes.len());
            }
        }
        if self.bulk(device, true, control, 13) != Some(13) {
            return false;
        }
        let csw = core::slice::from_raw_parts(control as *const u8, 13);
        &csw[..4] == b"USBS" && csw[4..8] == self.tag.to_le_bytes() && csw[8..13] == [0; 5]
    }
}

pub unsafe fn init() {
    if DMA == 0 {
        return;
    }
    let mut arenas = 0;
    'pci: for bus in 0..=u8::MAX {
        for slot in 0..32 {
            if hardware::pci(bus, slot, 0, 0) == u32::MAX {
                continue;
            }
            let count = if hardware::pci(bus, slot, 0, 0x0c) & (1 << 23) != 0 {
                8
            } else {
                1
            };
            for function in 0..count {
                if hardware::pci(bus, slot, function, 8) >> 8 != 0x0c0330 {
                    continue;
                }
                if arenas == MAX_HOSTS {
                    break 'pci;
                }
                let bar = hardware::pci(bus, slot, function, 0x10);
                let mut base = (bar & !15) as usize;
                match bar & 7 {
                    0 => {}
                    4 => base |= (hardware::pci(bus, slot, function, 0x14) as usize) << 32,
                    _ => continue,
                }
                if base < 0x100000 || !storage::identity_mmio(base) {
                    continue;
                }
                let flags = hardware::pci(bus, slot, function, 4) & 0xffff;
                hardware::pci_write(bus, slot, function, 4, flags | 2);
                let arena = DMA + arenas * HOST_PAGES * PAGE;
                arenas += 1;
                if let Some(mut host) = Host::open(base, arena, (bus, slot, function)) {
                    host.scan();
                    hardware::log(b"KreativeOS: xHCI controller ready\r\n");
                    for device in &host.devices {
                        if device.endpoints.iter().any(|ep| ep.kind == Kind::Keyboard) {
                            hardware::log(b"KreativeOS: USB keyboard ready\r\n");
                        }
                        if device.endpoints.iter().any(|ep| ep.kind == Kind::Mouse) {
                            hardware::log(b"KreativeOS: USB mouse ready\r\n");
                        }
                        if device.storage {
                            hardware::log(b"KreativeOS: USB mass storage ready\r\n");
                        }
                    }
                    HOSTS.push(host);
                }
            }
        }
    }
}

pub fn poll() -> Option<Event> {
    unsafe {
        for host in &mut HOSTS {
            for _ in 0..16 {
                let Some(event) = host.event() else { break };
                host.handle(event);
            }
        }
        EVENTS.pop_front()
    }
}

pub struct Disk {
    host: usize,
    slot: u8,
    pub sectors: u64,
    pub sector_size: usize,
    pub media: Option<Media>,
    pub model: Text<40>,
    pub port: u8,
    pub bdf: (u8, u8, u8),
}
impl Disk {
    fn run(&self, command: &[u8], bytes: &mut [u8], input: bool) -> bool {
        unsafe {
            let host = &mut HOSTS[self.host];
            let Some(index) = host
                .devices
                .iter()
                .position(|device| device.slot == self.slot && device.storage)
            else {
                return false;
            };
            let port = host.devices[index].port;
            if rd(host.op, 0x400 + usize::from(port - 1) * 16) & 3 != 3 {
                return false;
            }
            let mut device = host.devices.swap_remove(index);
            let result = host.scsi(&mut device, command, bytes, input);
            host.devices.push(device);
            result
        }
    }
    pub fn transfer(&self, lba: u64, bytes: &mut [u8], write: bool) -> bool {
        let Some(end) = lba.checked_add((bytes.len() / self.sector_size) as u64) else {
            return false;
        };
        if bytes.is_empty()
            || bytes.len() % self.sector_size != 0
            || end > self.sectors
            || (write && !self.media.is_some_and(|media| media.writable(lba, end)))
        {
            return false;
        }
        let mut position = lba;
        for chunk in bytes.chunks_mut(3 * PAGE) {
            let sectors = (chunk.len() / self.sector_size) as u32;
            let mut cmd = [0u8; 16];
            let length = if position + u64::from(sectors) <= (1 << 32) {
                cmd[0] = if write { 0x2a } else { 0x28 };
                cmd[2..6].copy_from_slice(&(position as u32).to_be_bytes());
                cmd[7..9].copy_from_slice(&(sectors as u16).to_be_bytes());
                10
            } else {
                cmd[0] = if write { 0x8a } else { 0x88 };
                cmd[2..10].copy_from_slice(&position.to_be_bytes());
                cmd[10..14].copy_from_slice(&sectors.to_be_bytes());
                16
            };
            if !self.run(&cmd[..length], chunk, !write) {
                return false;
            }
            position += u64::from(sectors);
        }
        !write || self.run(&[0x35, 0, 0, 0, 0, 0, 0, 0, 0, 0], &mut [], false)
    }
}

pub fn discover() -> Option<Disk> {
    let ports: Vec<_> = unsafe {
        HOSTS
            .iter()
            .enumerate()
            .flat_map(|(index, host)| {
                host.devices
                    .iter()
                    .filter(|device| device.storage)
                    .map(move |device| (index, device.slot, device.port))
            })
            .collect()
    };
    for (host, slot, port) in ports {
        let mut disk = Disk {
            host,
            slot,
            sectors: 0,
            sector_size: 512,
            media: None,
            model: Text::new(),
            port,
            bdf: unsafe { HOSTS[host].bdf },
        };
        let mut inquiry = [0; 36];
        if disk.run(&[0x12, 0, 0, 0, 36, 0], &mut inquiry, true) {
            for &byte in &inquiry[8..32] {
                if (32..127).contains(&byte) {
                    disk.model.push(byte);
                }
            }
        }
        let mut cap = [0; 8];
        if !disk.run(&[0x25, 0, 0, 0, 0, 0, 0, 0, 0, 0], &mut cap, true) {
            continue;
        }
        let mut block = u32::from_be_bytes(cap[4..].try_into().unwrap());
        let mut last = u64::from(u32::from_be_bytes(cap[..4].try_into().unwrap()));
        if last == u64::from(u32::MAX) {
            let mut cap16 = [0; 32];
            let mut cmd = [0; 16];
            cmd[..2].copy_from_slice(&[0x9e, 0x10]);
            cmd[13] = 32;
            if !disk.run(&cmd, &mut cap16, true) || cap16[12] & 1 != 0 {
                continue;
            }
            last = u64::from_be_bytes(cap16[..8].try_into().unwrap());
            block = u32::from_be_bytes(cap16[8..12].try_into().unwrap());
        }
        if !matches!(block, 512 | 4096) || last == u64::MAX {
            continue;
        }
        disk.sectors = last + 1;
        disk.sector_size = block as usize;
        disk.media = crate::partition::probe(disk.sectors, disk.sector_size, |lba, bytes| {
            disk.transfer(lba, bytes, false)
        });
        if matches!(disk.media, Some(Media::Exfat { .. })) {
            return Some(disk);
        }
    }
    None
}

fn hid_key(usage: u8, modifier: u8) -> Option<u8> {
    let shift = modifier & 0x22 != 0;
    let control = modifier & 0x11 != 0;
    if control {
        let special = match usage {
            15 => 0x84,
            23 => 0xa0,
            7 => 0xa1,
            44 => 0xa2,
            53 => 0xaa,
            16 => 0xab,
            30..=33 => 0xa3 + usage - 30,
            6 => 0xa7,
            25 => {
                if shift {
                    0xa9
                } else {
                    0xa8
                }
            }
            _ => 0,
        };
        if special != 0 {
            return Some(special);
        }
    }
    match usage {
        4..=29 => Some(if shift { b'A' } else { b'a' } + usage - 4),
        30..=39 => Some((if shift { b"!@#$%^&*()" } else { b"1234567890" })[(usage - 30) as usize]),
        40 => Some(b'\n'),
        41 => Some(27),
        42 => Some(8),
        43 => Some(b'\t'),
        44 => Some(b' '),
        45..=56 if usage != 50 => Some(
            (if shift {
                b"_+{}|\0:\"~<>?"
            } else {
                b"-=[]\\\0;'\x60,./"
            })[(usage - 45) as usize],
        ),
        58..=67 => Some(0x90 + usage - 58),
        79 => Some(0x83),
        80 => Some(0x82),
        81 => Some(0x81),
        82 => Some(0x80),
        _ => None,
    }
}
