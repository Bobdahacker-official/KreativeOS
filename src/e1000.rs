//! Intel 8254x Ethernet with polled legacy descriptor rings.
//! Compatible PCI IDs cross-checked against iPXE (https://ipxe.org).
use crate::hardware::{millis, pci, pci_write};
use core::{
    ptr,
    sync::atomic::{fence, Ordering},
};

const COUNT: usize = 32;
const BUFFER: usize = 2048;
const RX_BUFFERS: usize = 4096;
const TX_BUFFERS: usize = RX_BUFFERS + COUNT * BUFFER;
const RX_RING: usize = 0;
const TX_RING: usize = 1024;

#[derive(Clone, Copy)]
pub enum Model {
    I82540Em,
    I82544Gc,
    I82545Em,
    I82574L,
}
impl Model {
    pub fn from_pci(id: u32) -> Option<Self> {
        match id {
            0x100e8086 => Some(Self::I82540Em),
            0x100c8086 => Some(Self::I82544Gc),
            0x100f8086 => Some(Self::I82545Em),
            0x10d38086 => Some(Self::I82574L),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::I82540Em => "Intel 82540EM",
            Self::I82544Gc => "Intel 82544GC",
            Self::I82545Em => "Intel 82545EM",
            Self::I82574L => "Intel 82574L",
        }
    }
}

pub struct Adapter {
    registers: usize,
    dma: usize,
    rx: usize,
    tx: usize,
    pub model: Model,
    pub mac: [u8; 6],
}

impl Adapter {
    fn reg(&self, register: usize) -> u32 {
        unsafe { ptr::read_volatile((self.registers + register) as *const u32) }
    }

    fn set(&self, register: usize, value: u32) {
        unsafe {
            ptr::write_volatile((self.registers + register) as *mut u32, value);
        }
    }

    fn descriptor(&self, ring: usize, index: usize) -> *mut u8 {
        (self.dma + ring + index * 16) as *mut u8
    }

    pub unsafe fn init(
        bus: u8,
        device: u8,
        function: u8,
        model: Model,
        dma: usize,
    ) -> Option<Self> {
        let bar = pci(bus, device, function, 0x10);
        let address = bar & !15;
        let mapped = match bar & 7 {
            0 => true,
            4 => pci(bus, device, function, 0x14) == 0,
            _ => false,
        };
        if !mapped || address == 0 || address == 0xffff_fff0 {
            return None;
        }
        let mut a = Self {
            registers: address as usize,
            model,
            dma,
            rx: 0,
            tx: 0,
            mac: [0; 6],
        };
        let command = pci(bus, device, function, 4);
        pci_write(bus, device, function, 4, (command & 0xffff) | 6);
        a.set(0x00d8, u32::MAX); // Disable all hardware interrupts.
        a.set(0, a.reg(0) | (1 << 26));
        let start = millis();
        while a.reg(0) & (1 << 26) != 0 {
            if millis() - start >= 120 {
                return None;
            }
        }
        a.set(0x00d8, u32::MAX);
        let _ = a.reg(0x00c0);
        // RAL/RAH contain the address loaded by hardware or firmware.
        let low = a.reg(0x5400).to_le_bytes();
        a.mac[..4].copy_from_slice(&low);
        let high = (a.reg(0x5404) as u16).to_le_bytes();
        a.mac[4..].copy_from_slice(&high);
        if !valid_mac(a.mac) {
            // PCIe 82574 uses a 2-bit EERD command field. PCI 8254x uses 8 bits.
            let (address_shift, done) = match model {
                Model::I82574L => (2, 1 << 1),
                _ => (8, 1 << 4),
            };
            for word in 0..3 {
                a.set(0x0014, 1 | ((word as u32) << address_shift));
                let start = millis();
                loop {
                    let value = a.reg(0x0014);
                    if value & done != 0 {
                        a.mac[word * 2..word * 2 + 2]
                            .copy_from_slice(&((value >> 16) as u16).to_le_bytes());
                        break;
                    }
                    if millis() - start >= 120 {
                        return None;
                    }
                }
            }
        }
        if !valid_mac(a.mac) {
            return None;
        }
        a.set(0x5400, u32::from_le_bytes(a.mac[..4].try_into().unwrap()));
        a.set(
            0x5404,
            (1 << 31) | u16::from_le_bytes(a.mac[4..].try_into().unwrap()) as u32,
        );
        for i in 0..128 {
            a.set(0x5200 + i * 4, 0);
        }
        a.set(0, a.reg(0) | (1 << 6) | (1 << 5));
        ptr::write_bytes(dma as *mut u8, 0, TX_BUFFERS + COUNT * BUFFER);
        for i in 0..COUNT {
            ptr::write_volatile(
                a.descriptor(RX_RING, i).cast::<u64>(),
                (dma + RX_BUFFERS + i * BUFFER) as u64,
            );
            ptr::write_volatile(
                a.descriptor(TX_RING, i).cast::<u64>(),
                (dma + TX_BUFFERS + i * BUFFER) as u64,
            );
            ptr::write_volatile(a.descriptor(TX_RING, i).add(12), 1);
        }
        fence(Ordering::SeqCst);
        a.set(0x2800, (dma + RX_RING) as u32);
        a.set(0x2804, 0);
        a.set(0x2808, (COUNT * 16) as u32);
        a.set(0x2810, 0);
        a.set(0x2818, (COUNT - 1) as u32);
        a.set(0x3800, (dma + TX_RING) as u32);
        a.set(0x3804, 0);
        a.set(0x3808, (COUNT * 16) as u32);
        a.set(0x3810, 0);
        a.set(0x3818, 0);
        a.set(0x0410, 10 | (8 << 10) | (6 << 20));
        a.set(0x0400, (1 << 1) | (1 << 3) | (15 << 4) | (64 << 12));
        a.set(0x0100, (1 << 1) | (1 << 15) | (1 << 26));
        Some(a)
    }

    pub fn receive(&mut self, frame: &mut [u8]) -> Option<usize> {
        for _ in 0..COUNT {
            let current = self.rx;
            let descriptor = self.descriptor(RX_RING, current);
            unsafe {
                let status = ptr::read_volatile(descriptor.add(12));
                if status & 1 == 0 {
                    return None;
                }
                fence(Ordering::Acquire);
                let errors = ptr::read_volatile(descriptor.add(13));
                let length = ptr::read_volatile(descriptor.add(8).cast::<u16>()) as usize;
                let valid = status & 2 != 0 && errors == 0 && (14..=frame.len()).contains(&length);
                if valid {
                    ptr::copy_nonoverlapping(
                        (self.dma + RX_BUFFERS + current * BUFFER) as *const u8,
                        frame.as_mut_ptr(),
                        length,
                    );
                }
                ptr::write_volatile(descriptor.add(12), 0);
                fence(Ordering::SeqCst);
                self.set(0x2818, current as u32);
                self.rx = (current + 1) % COUNT;
                if valid {
                    return Some(length);
                }
            }
        }
        None
    }

    pub fn transmit(&mut self, frame: &[u8]) -> bool {
        let index = self.tx;
        let descriptor = self.descriptor(TX_RING, index);
        unsafe {
            if ptr::read_volatile(descriptor.add(12)) & 1 == 0 {
                return false;
            }
            let target = (self.dma + TX_BUFFERS + index * BUFFER) as *mut u8;
            ptr::copy_nonoverlapping(frame.as_ptr(), target, frame.len());
            ptr::write_volatile(
                descriptor.add(8).cast::<u32>(),
                frame.len() as u32 | (0b1011 << 24),
            );
            ptr::write_volatile(descriptor.add(12).cast::<u32>(), 0);
        }
        fence(Ordering::SeqCst);
        self.tx = (index + 1) % COUNT;
        self.set(0x3818, self.tx as u32);
        true
    }
}

fn valid_mac(mac: [u8; 6]) -> bool {
    mac != [0; 6] && mac[0] & 1 == 0
}
