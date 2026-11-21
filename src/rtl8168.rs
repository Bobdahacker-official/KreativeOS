//! Realtek RTL8168H/8111H PCIe Ethernet, using polled C+ descriptor rings.
//! Register layout cross-checked against the Realtek gigabit register guide
//! and OpenBSD's re(4) support for the RTL8168H revision.
use crate::{hardware, storage};
use core::{
    ptr,
    sync::atomic::{fence, Ordering},
};

pub const PCI_ID: u32 = 0x8168_10ec;
const ID: usize = 0x00;
const TX_RING: usize = 0x20;
const COMMAND: usize = 0x37;
const TX_POLL: usize = 0x38;
const INT_MASK: usize = 0x3c;
const INT_STATUS: usize = 0x3e;
const TX_CONFIG: usize = 0x40;
const RX_CONFIG: usize = 0x44;
const CONFIG_UNLOCK: usize = 0x50;
const CONFIG_1: usize = 0x52;
const PHY_ACCESS: usize = 0x60;
const PHY_STATUS: usize = 0x6c;
const RX_MAX_SIZE: usize = 0xda;
const CPLUS_COMMAND: usize = 0xe0;
const RX_RING: usize = 0xe4;
const TX_THRESHOLD: usize = 0xec;
const MISC: usize = 0xf0;
const RESET: u8 = 1 << 4;
const RX_TX_ENABLE: u8 = 0x0c;
const PHY_BUSY: u32 = 1 << 31;

const COUNT: usize = 32;
const BUFFER: usize = 2048;
const RX_BUFFERS: usize = 4096;
const TX_BUFFERS: usize = RX_BUFFERS + COUNT * BUFFER;
const RX_DESCRIPTORS: usize = 0;
const TX_DESCRIPTORS: usize = 1024;
const OWN: u32 = 1 << 31;
const END_OF_RING: u32 = 1 << 30;
const WHOLE_FRAME: u32 = (1 << 29) | (1 << 28);
const RX_ERROR: u32 = (1 << 24) | (1 << 23) | (1 << 22) | (1 << 21) | (1 << 20) | (1 << 19);
const RX_LENGTH: u32 = 0x3fff;
const _: () = assert!(TX_BUFFERS + COUNT * BUFFER <= crate::net::DMA_PAGES * 4096);

#[repr(C)]
struct Descriptor {
    flags: u32,
    options: u32,
    address: u64,
}

pub struct Adapter {
    registers: usize,
    dma: usize,
    rx: usize,
    tx: usize,
    pub mac: [u8; 6],
}

fn ring_end(index: usize) -> u32 {
    if index == COUNT - 1 {
        END_OF_RING
    } else {
        0
    }
}

fn received_length(flags: u32, capacity: usize) -> Option<usize> {
    let length = ((flags & RX_LENGTH) as usize).checked_sub(4)?;
    (flags & (OWN | WHOLE_FRAME | RX_ERROR) == WHOLE_FRAME
        && (14..=capacity.min(BUFFER - 4)).contains(&length))
    .then_some(length)
}

fn wait(timeout: u64, mut ready: impl FnMut() -> bool) -> bool {
    let start = hardware::millis();
    loop {
        if ready() {
            return true;
        }
        if hardware::millis().wrapping_sub(start) >= timeout {
            return false;
        }
        core::hint::spin_loop();
    }
}

fn delay_us(duration: u64) {
    let start = hardware::micros();
    while hardware::micros().wrapping_sub(start) < duration {
        core::hint::spin_loop();
    }
}

unsafe fn wake_pci(bus: u8, slot: u8, function: u8) -> bool {
    if hardware::pci(bus, slot, function, 4) & (1 << 20) == 0 {
        return true;
    }
    let mut next = (hardware::pci(bus, slot, function, 0x34) as u8) & !3;
    for _ in 0..48 {
        if !(0x40..=0xf8).contains(&next) {
            return true;
        }
        let capability = hardware::pci(bus, slot, function, next);
        if capability as u8 == 1 {
            let control = next + 4;
            let value = hardware::pci(bus, slot, function, control);
            if value & 3 != 0 {
                // PMCSR status is write-one-to-clear. Preserve the PME enable
                // and leave the unrelated upper half of the capability alone.
                hardware::pci_write(bus, slot, function, control, value & !(0x8000 | 3));
                delay_us(10_000);
            }
            return hardware::pci(bus, slot, function, control) & 3 == 0;
        }
        next = ((capability >> 8) as u8) & !3;
    }
    false
}

impl Adapter {
    fn read8(&self, offset: usize) -> u8 {
        unsafe { ptr::read_volatile((self.registers + offset) as *const u8) }
    }
    fn read32(&self, offset: usize) -> u32 {
        unsafe { ptr::read_volatile((self.registers + offset) as *const u32) }
    }
    fn write8(&self, offset: usize, value: u8) {
        unsafe { ptr::write_volatile((self.registers + offset) as *mut u8, value) }
    }
    fn write16(&self, offset: usize, value: u16) {
        unsafe { ptr::write_volatile((self.registers + offset) as *mut u16, value) }
    }
    fn write32(&self, offset: usize, value: u32) {
        unsafe { ptr::write_volatile((self.registers + offset) as *mut u32, value) }
    }
    fn write_ring(&self, offset: usize, address: usize) {
        self.write32(offset + 4, ((address as u64) >> 32) as u32);
        self.write32(offset, address as u32);
    }
    fn descriptor(&self, ring: usize, index: usize) -> *mut Descriptor {
        unsafe { ((self.dma + ring) as *mut Descriptor).add(index) }
    }

    pub unsafe fn init(bus: u8, slot: u8, function: u8, dma: usize) -> Result<Self, &'static str> {
        // RTL8168H exposes I/O at BAR0 and its PCIe MMIO registers at BAR2.
        let bar = hardware::pci(bus, slot, function, 0x18);
        let registers = match bar & 7 {
            0 => (bar & !15) as usize,
            4 => (bar & !15) as usize | ((hardware::pci(bus, slot, function, 0x1c) as usize) << 32),
            _ => return Err("RTL8168H MMIO BAR unavailable"),
        };
        if registers < 0x100000
            || !storage::identity_mmio(registers)
            || !storage::identity_mmio(registers.saturating_add(0xff))
            || dma == 0
            || dma & 4095 != 0
        {
            return Err("RTL8168H MMIO or DMA unavailable");
        }
        let command = hardware::pci(bus, slot, function, 4) & 0xffff;
        // Keep bus mastering disabled until our own DMA rings are published.
        hardware::pci_write(bus, slot, function, 4, (command | 2 | (1 << 10)) & !4);
        if !wake_pci(bus, slot, function) {
            return Err("RTL8168H PCI power-up failed");
        }
        let mut adapter = Self {
            registers,
            dma,
            rx: 0,
            tx: 0,
            mac: [0; 6],
        };
        // 0x5400 identifies the H generation. PCI device IDs are shared by
        // multiple incompatible RTL8168 generations.
        if adapter.read32(TX_CONFIG) & 0x7c80_0000 != 0x5400_0000 {
            hardware::pci_write(bus, slot, function, 4, command);
            return Err("RTL8168H MAC revision not found");
        }
        adapter.write16(INT_MASK, 0);
        adapter.write8(COMMAND, RESET);
        if !wait(100, || adapter.read8(COMMAND) & RESET == 0) {
            return Err("RTL8168H reset timed out");
        }
        adapter.write16(INT_MASK, 0);
        adapter.write16(INT_STATUS, u16::MAX);
        for index in 0..adapter.mac.len() {
            adapter.mac[index] = adapter.read8(ID + index);
        }
        if adapter.mac == [0; 6] || adapter.mac[0] & 1 != 0 {
            return Err("RTL8168H station address unavailable");
        }
        if !adapter.configure_phy() {
            return Err("RTL8168H PHY initialization failed");
        }
        adapter.write16(CPLUS_COMMAND, (1 << 7) | (1 << 3) | 1);
        adapter.write8(CONFIG_UNLOCK, 0xc0);
        adapter.write8(CONFIG_1, adapter.read8(CONFIG_1) | (1 << 5));
        adapter.write8(CONFIG_UNLOCK, 0);
        adapter.prepare_rings();
        adapter.write_ring(RX_RING, dma + RX_DESCRIPTORS);
        adapter.write_ring(TX_RING, dma + TX_DESCRIPTORS);
        adapter.write16(RX_MAX_SIZE, 1522);
        adapter.write32(TX_CONFIG, (3 << 24) | (7 << 8));
        adapter.write8(TX_THRESHOLD, 16);
        // Accept this station and broadcast traffic. Leave VLAN stripping and
        // checksum offload off so the existing Ethernet layer sees raw frames.
        adapter.write32(RX_CONFIG, (7 << 13) | (1 << 11) | (7 << 8) | 0x0a);
        adapter.write32(MISC, adapter.read32(MISC) & !(1 << 19));
        fence(Ordering::SeqCst);
        hardware::pci_write(bus, slot, function, 4, command | 6 | (1 << 10));
        adapter.write8(COMMAND, RX_TX_ENABLE);
        if adapter.read8(COMMAND) & RX_TX_ENABLE != RX_TX_ENABLE {
            adapter.write8(COMMAND, 0);
            hardware::pci_write(bus, slot, function, 4, (command | 2 | (1 << 10)) & !4);
            return Err("RTL8168H transmit and receive did not start");
        }
        Ok(adapter)
    }

    fn phy_read(&self, register: u8) -> Option<u16> {
        self.write32(PHY_ACCESS, u32::from(register) << 16);
        if !wait(10, || self.read32(PHY_ACCESS) & PHY_BUSY != 0) {
            return None;
        }
        let value = self.read32(PHY_ACCESS) as u16;
        delay_us(25);
        Some(value)
    }

    fn phy_write(&self, register: u8, value: u16) -> bool {
        self.write32(
            PHY_ACCESS,
            PHY_BUSY | (u32::from(register) << 16) | u32::from(value),
        );
        if !wait(10, || self.read32(PHY_ACCESS) & PHY_BUSY == 0) {
            return false;
        }
        delay_us(25);
        true
    }

    fn configure_phy(&self) -> bool {
        // Select the standard MII page and wake the integrated transceiver.
        if !self.phy_write(31, 0) {
            return false;
        }
        let Some(control) = self.phy_read(0).filter(|&v| v != u16::MAX) else {
            return false;
        };
        let Some(advertisement) = self.phy_read(4).filter(|&v| v != u16::MAX) else {
            return false;
        };
        let Some(gigabit) = self.phy_read(9).filter(|&v| v != u16::MAX) else {
            return false;
        };
        self.phy_write(4, (advertisement & 0x0c00) | 0x01e1)
            && self.phy_write(9, (gigabit & !0x1b00) | (1 << 9))
            && self.phy_write(0, (control & !0xcc00) | 0x1200)
    }

    unsafe fn prepare_rings(&self) {
        for index in 0..COUNT {
            ptr::write_volatile(
                self.descriptor(RX_DESCRIPTORS, index),
                Descriptor {
                    flags: OWN | ring_end(index) | BUFFER as u32,
                    options: 0,
                    address: (self.dma + RX_BUFFERS + index * BUFFER) as u64,
                },
            );
            ptr::write_volatile(
                self.descriptor(TX_DESCRIPTORS, index),
                Descriptor {
                    flags: ring_end(index),
                    options: 0,
                    address: (self.dma + TX_BUFFERS + index * BUFFER) as u64,
                },
            );
        }
    }

    pub fn link_speed(&self) -> Option<u16> {
        match self.read8(PHY_STATUS) & 0x1e {
            0x12 => Some(1000),
            0x0a => Some(100),
            0x06 => Some(10),
            _ => None,
        }
    }

    pub fn receive(&mut self, frame: &mut [u8]) -> Option<usize> {
        for _ in 0..COUNT {
            let index = self.rx;
            let descriptor = self.descriptor(RX_DESCRIPTORS, index);
            unsafe {
                let flags = ptr::read_volatile(ptr::addr_of!((*descriptor).flags));
                if flags & OWN != 0 {
                    return None;
                }
                fence(Ordering::Acquire);
                let length = received_length(flags, frame.len());
                if let Some(length) = length {
                    ptr::copy_nonoverlapping(
                        (self.dma + RX_BUFFERS + index * BUFFER) as *const u8,
                        frame.as_mut_ptr(),
                        length,
                    );
                }
                ptr::write_volatile(ptr::addr_of_mut!((*descriptor).options), 0);
                fence(Ordering::Release);
                ptr::write_volatile(
                    ptr::addr_of_mut!((*descriptor).flags),
                    OWN | ring_end(index) | BUFFER as u32,
                );
                self.rx = (index + 1) % COUNT;
                if length.is_some() {
                    return length;
                }
            }
        }
        None
    }

    pub fn transmit(&mut self, frame: &[u8]) -> bool {
        if !(14..=1522).contains(&frame.len()) {
            return false;
        }
        let index = self.tx;
        let descriptor = self.descriptor(TX_DESCRIPTORS, index);
        unsafe {
            if ptr::read_volatile(ptr::addr_of!((*descriptor).flags)) & OWN != 0 {
                return false;
            }
            fence(Ordering::Acquire);
            let buffer = (self.dma + TX_BUFFERS + index * BUFFER) as *mut u8;
            let length = frame.len().max(60);
            ptr::copy_nonoverlapping(frame.as_ptr(), buffer, frame.len());
            ptr::write_bytes(buffer.add(frame.len()), 0, length - frame.len());
            ptr::write_volatile(ptr::addr_of_mut!((*descriptor).options), 0);
            fence(Ordering::Release);
            ptr::write_volatile(
                ptr::addr_of_mut!((*descriptor).flags),
                OWN | ring_end(index) | WHOLE_FRAME | length as u32,
            );
        }
        fence(Ordering::SeqCst);
        self.write8(TX_POLL, 0x40);
        self.tx = (index + 1) % COUNT;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_receive_status_before_copying() {
        assert_eq!(received_length(WHOLE_FRAME | 64, 1536), Some(60));
        assert_eq!(
            received_length(WHOLE_FRAME | END_OF_RING | 1522, 1536),
            Some(1518)
        );
        for flags in [
            OWN | WHOLE_FRAME | 64,
            WHOLE_FRAME | RX_ERROR | 64,
            64,
            WHOLE_FRAME | 3,
        ] {
            assert_eq!(received_length(flags, 1536), None);
        }
        assert_eq!(received_length(WHOLE_FRAME | 1522, 1500), None);
        assert_eq!(received_length(WHOLE_FRAME | 8192, 8192), None);
    }

    #[test]
    fn wraps_dma_rings_and_preserves_padding() {
        let mut arena = vec![0u64; (TX_BUFFERS + COUNT * BUFFER) / 8];
        let mut registers = [0u32; 64];
        let mut adapter = Adapter {
            registers: registers.as_mut_ptr() as usize,
            dma: arena.as_mut_ptr() as usize,
            mac: [2, 0, 0, 0, 0, 1],
            rx: COUNT - 1,
            tx: COUNT - 1,
        };
        unsafe { adapter.prepare_rings() };
        let data = [0x5a; 42];
        assert!(adapter.transmit(&data));
        unsafe {
            let tx = &*adapter.descriptor(TX_DESCRIPTORS, COUNT - 1);
            assert_eq!(tx.flags, OWN | WHOLE_FRAME | END_OF_RING | 60);
            let tx_buffer = core::slice::from_raw_parts(tx.address as *const u8, 60);
            assert_eq!(&tx_buffer[..42], &data);
            assert_eq!(&tx_buffer[42..], &[0; 18]);
            let rx = &mut *adapter.descriptor(RX_DESCRIPTORS, COUNT - 1);
            ptr::write_bytes(rx.address as *mut u8, 0xa5, 60);
            rx.flags = WHOLE_FRAME | END_OF_RING | 64;
        }
        assert_eq!(adapter.read8(TX_POLL), 0x40);
        assert_eq!(adapter.tx, 0);
        let mut frame = [0u8; 1536];
        assert_eq!(adapter.receive(&mut frame), Some(60));
        assert_eq!(frame[..60], [0xa5; 60]);
        assert_eq!(adapter.rx, 0);
        unsafe {
            assert_eq!(
                (*adapter.descriptor(RX_DESCRIPTORS, COUNT - 1)).flags,
                OWN | END_OF_RING | BUFFER as u32
            );
        }
    }
}
