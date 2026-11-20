//! Polled AHCI 1.3.1 SATA transport for explicitly provisioned KFS media.
use crate::{
    hardware::{millis, pci, pci_write},
    text::Text,
};
use core::{
    ptr,
    sync::atomic::{fence, Ordering},
};

pub const PAGES: usize = 16;
use crate::storage::{identity_mmio, marked_volume, MARKER_LBA, WRITE_LBAS};
const CL: usize = 0;
const RFIS: usize = 1024;
const TABLE: usize = 1280;
const DATA: usize = 4096;
const MAX_BYTES: usize = PAGES * 4096 - DATA;

const IS: usize = 0x10;
const IE: usize = 0x14;
const CMD: usize = 0x18;
const TFD: usize = 0x20;
const SIG: usize = 0x24;
const SSTS: usize = 0x28;
const SCTL: usize = 0x2c;
const SERR: usize = 0x30;
const SACT: usize = 0x34;
const CI: usize = 0x38;
const ST: u32 = 1;
const FRE: u32 = 1 << 4;
const FR: u32 = 1 << 14;
const CR: u32 = 1 << 15;
const FATAL: u32 = 0x7c00_0000;

pub static mut DMA: usize = 0;

pub struct Disk {
    regs: usize,
    pub port: u8,
    pub bdf: (u8, u8, u8),
    pub sectors: u64,
    pub model: Text<40>,
    usable: bool,
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

unsafe fn reg(address: usize) -> u32 {
    ptr::read_volatile(address as *const u32)
}
unsafe fn set(address: usize, value: u32) {
    ptr::write_volatile(address as *mut u32, value);
}

impl Disk {
    unsafe fn get(&self, offset: usize) -> u32 {
        reg(self.regs + offset)
    }
    unsafe fn put(&self, offset: usize, value: u32) {
        set(self.regs + offset, value);
    }
    unsafe fn stop(&self) -> bool {
        self.put(CMD, self.get(CMD) & !ST);
        if !wait(500, || self.get(CMD) & CR == 0) {
            return false;
        }
        self.put(CMD, self.get(CMD) & !FRE);
        wait(500, || self.get(CMD) & FR == 0)
    }
    unsafe fn start(&self) -> bool {
        if !self.stop() || self.get(CI) != 0 || self.get(SACT) != 0 {
            return false;
        }
        ptr::write_bytes(DMA as *mut u8, 0, PAGES * 4096);
        self.put(0x00, (DMA + CL) as u32);
        self.put(0x04, 0);
        self.put(0x08, (DMA + RFIS) as u32);
        self.put(0x0c, 0);
        self.put(IE, 0);
        self.put(IS, u32::MAX);
        self.put(SERR, u32::MAX);
        self.put(CMD, self.get(CMD) | FRE);
        if self.get(SIG) == u32::MAX || self.get(TFD) & 0x88 != 0 {
            // Without an initial D2H FIS the signature is undefined. Reset only this port.
            let control = self.get(SCTL) & !15;
            self.put(SCTL, control | 1);
            wait(2, || false);
            self.put(SCTL, control);
        } else if self.get(SSTS) & 0xf00 != 0x100 {
            self.put(CMD, (self.get(CMD) & 0x0fff_ffff) | (1 << 28));
        }
        if !wait(1000, || {
            self.get(SSTS) & 0xf0f == 0x103
                && self.get(SIG) == 0x0000_0101
                && self.get(TFD) & 0x88 == 0
        }) {
            return false;
        }
        self.put(SERR, u32::MAX);
        self.put(IS, u32::MAX);
        self.put(CMD, self.get(CMD) | ST);
        true
    }
    unsafe fn command(&mut self, opcode: u8, lba: u32, data: &mut [u8], write: bool) -> bool {
        if !self.usable || data.len() > MAX_BYTES || data.len() % 512 != 0 {
            return false;
        }
        if self.get(SSTS) & 15 != 3
            || self.get(SERR) & (1 << 16) != 0
            || !wait(500, || self.get(TFD) & 0x88 == 0 && self.get(CI) == 0)
        {
            self.usable = false;
            return false;
        }
        let header = (DMA + CL) as *mut u32;
        let table = (DMA + TABLE) as *mut u8;
        let buffer = (DMA + DATA) as *mut u8;
        ptr::write_bytes(header, 0, 8);
        ptr::write_bytes(table, 0, 144);
        let region = u32::from(!data.is_empty()) << 16;
        header.write(5 | (u32::from(write) << 6) | region);
        header.add(2).write((DMA + TABLE) as u32);
        let mut fis = [0u8; 20];
        fis[..3].copy_from_slice(&[0x27, 0x80, opcode]);
        if matches!(opcode, 0x25 | 0x35) {
            let bytes = lba.to_le_bytes();
            fis[4..7].copy_from_slice(&bytes[..3]);
            fis[7] = 0x40;
            fis[8] = bytes[3];
            fis[12..14].copy_from_slice(&((data.len() / 512) as u16).to_le_bytes());
        }
        ptr::copy_nonoverlapping(fis.as_ptr(), table, fis.len());
        if region != 0 {
            table.add(128).cast::<u32>().write((DMA + DATA) as u32);
            table.add(140).cast::<u32>().write(data.len() as u32 - 1);
            if write {
                ptr::copy_nonoverlapping(data.as_ptr(), buffer, data.len());
            }
        }
        self.put(IS, u32::MAX);
        fence(Ordering::SeqCst);
        self.put(CI, 1);
        let complete = wait(if opcode == 0xea { 5000 } else { 1500 }, || {
            self.get(CI) & 1 == 0 || self.get(IS) & FATAL != 0
        });
        fence(Ordering::SeqCst);
        if !complete
            || self.get(CI) & 1 != 0
            || self.get(IS) & FATAL != 0
            || self.get(TFD) & 0x21 != 0
            || header.add(1).read_volatile() as usize != data.len()
        {
            self.usable = false;
            // The arena is reserved permanently. An unsuccessful stop never releases it.
            self.stop();
            return false;
        }
        if !write && region != 0 {
            ptr::copy_nonoverlapping(buffer, data.as_mut_ptr(), data.len());
        }
        true
    }
    pub unsafe fn transfer(&mut self, lba: u32, bytes: &mut [u8], write: bool) -> bool {
        let end = u64::from(lba) + bytes.len() as u64 / 512;
        if bytes.is_empty()
            || end > self.sectors
            || (write && (!WRITE_LBAS.contains(&lba) || end > u64::from(WRITE_LBAS.end)))
        {
            return false;
        }
        self.command(if write { 0x35 } else { 0x25 }, lba, bytes, write)
            && (!write || self.command(0xea, 0, &mut [], false))
    }
    unsafe fn identify(&mut self) -> bool {
        let mut info = [0u8; 512];
        if !self.command(0xec, 0, &mut info, false) {
            return false;
        }
        let word = |i: usize| u16::from_le_bytes([info[i * 2], info[i * 2 + 1]]);
        let geometry = word(106);
        if word(49) & (1 << 8) == 0
            || word(83) & 0xe400 != 0x6400
            || (geometry & 0xd000 == 0x5000 && (word(117) != 256 || word(118) != 0))
        {
            return false;
        }
        self.sectors = u64::from_le_bytes(info[200..208].try_into().unwrap());
        if self.sectors < 16384 {
            return false;
        }
        let mut model = [b' '; 40];
        for (i, pair) in info[54..94].chunks_exact(2).enumerate() {
            for (j, byte) in pair.iter().rev().enumerate() {
                model[i * 2 + j] = if (32..127).contains(byte) {
                    *byte
                } else {
                    b'?'
                };
            }
        }
        self.model
            .set(core::str::from_utf8(&model).unwrap_or("").trim());
        true
    }
}

pub unsafe fn discover() -> Option<Disk> {
    if DMA == 0 {
        return None;
    }
    for b in 0..=u8::MAX {
        for d in 0..32 {
            if pci(b, d, 0, 0) == u32::MAX {
                continue;
            }
            let functions = if pci(b, d, 0, 0x0c) & (1 << 23) != 0 {
                8
            } else {
                1
            };
            for f in 0..functions {
                if pci(b, d, f, 8) >> 8 != 0x010601 {
                    continue;
                }
                let bar = pci(b, d, f, 0x24);
                let base = (bar & !15) as usize;
                if bar & 7 != 0 || base < 0x100000 || !identity_mmio(base) {
                    continue;
                }
                let command = pci(b, d, f, 4) & 0xffff;
                pci_write(b, d, f, 4, command | 2);
                let version = reg(base + 0x10);
                if version >> 16 != 1 {
                    continue;
                }
                if reg(base + 0x24) & 1 != 0 {
                    set(base + 0x28, (reg(base + 0x28) & !(1 << 3)) | 2);
                    if !wait(2100, || reg(base + 0x28) & 0x11 == 0) {
                        continue;
                    }
                }
                set(base + 4, (reg(base + 4) | (1 << 31)) & !2);
                pci_write(b, d, f, 4, command | 6);
                let ports = reg(base + 0x0c);
                for port in 0..32 {
                    let regs = base + 0x100 + port as usize * 0x80;
                    if ports & (1 << port) == 0
                        || !identity_mmio(regs + 0x7f)
                        || reg(regs + SSTS) & 15 != 3
                        || !matches!(reg(regs + SIG), 0x0000_0101 | u32::MAX)
                    {
                        continue;
                    }
                    let mut disk = Disk {
                        regs,
                        port,
                        bdf: (b, d, f),
                        sectors: 0,
                        model: Text::new(),
                        usable: true,
                    };
                    if !disk.start() {
                        if !disk.stop() {
                            return None;
                        }
                        continue;
                    }
                    let mut label = [0u8; 512];
                    let mut first = [0u8; 512];
                    if disk.identify()
                        && disk.transfer(MARKER_LBA, &mut label, false)
                        && disk.transfer(0, &mut first, false)
                        && marked_volume(&label, &first)
                    {
                        return Some(disk);
                    }
                    if !disk.stop() {
                        return None;
                    }
                }
            }
        }
    }
    None
}
