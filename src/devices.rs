use crate::{hardware::pci, text::Text};
use alloc::vec::Vec;
use core::fmt::Write;

pub fn inventory() -> Vec<Text<80>> {
    let mut devices = Vec::new();
    for bus in 0..=255 {
        for slot in 0..32 {
            if unsafe { pci(bus, slot, 0, 0) } == u32::MAX {
                continue;
            }
            let header = unsafe { pci(bus, slot, 0, 0x0c) };
            let functions = if header & (1 << 23) != 0 { 8 } else { 1 };
            for function in 0..functions {
                let id = unsafe { pci(bus, slot, function, 0) };
                if id == u32::MAX {
                    continue;
                }
                let class = unsafe { pci(bus, slot, function, 8) };
                let category = match (class >> 16) as u16 {
                    0x0101 => "IDE storage",
                    0x0106 => "SATA controller",
                    0x0108 => "NVMe controller",
                    0x0200 => "Ethernet controller",
                    0x0280 => "Wireless controller",
                    0x0300..=0x0380 => "Display controller",
                    0x0401 | 0x0403 => "Audio controller",
                    0x0400 => "Video controller",
                    0x0604 => "PCI-to-PCI bridge",
                    0x0701 => "Parallel port",
                    0x0c03 => "USB controller",
                    0x0d11 => "Bluetooth controller",
                    _ => "PCI device",
                };
                let mut row = Text::new();
                let _ = write!(
                    row,
                    "{bus:02x}:{slot:02x}.{function}  {:04x}:{:04x}  {category}",
                    id as u16,
                    id >> 16
                );
                devices.push(row);
            }
        }
    }
    devices
}
