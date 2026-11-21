//! Identification of the Realtek RTL8811CU (USB 0bda:c811) at its native register interface.
use crate::text::Text;
use core::fmt::Write;

pub const VENDOR: u16 = 0x0bda;
pub const PRODUCT: u16 = 0xc811;
const SYSTEM_CONFIGURATION: u16 = 0x00f0;

#[derive(Clone, Copy)]
pub enum Probe {
    RegisterUnavailable,
    ChipCut(u8),
}

pub fn probe(mut control_read: impl FnMut([u8; 8]) -> Option<u32>) -> Probe {
    // Realtek vendor register read, request 5. Read the system configuration
    // register in the always-on register block before touching the RF.
    let [low, high] = SYSTEM_CONFIGURATION.to_le_bytes();
    let value = control_read([0xc0, 5, low, high, 0, 0, 4, 0]);
    match value {
        Some(value) if value != 0 && value != u32::MAX => {
            Probe::ChipCut(((value >> 12) & 15) as u8)
        }
        _ => Probe::RegisterUnavailable,
    }
}

impl Probe {
    pub fn describe(self, text: &mut Text<128>) {
        text.set("Realtek RTL8811CU");
        match self {
            Self::ChipCut(cut) => {
                let _ = write!(text, " | chip cut {cut}, Wi-Fi unavailable");
            }
            Self::RegisterUnavailable => text.append(" | register read failed"),
        }
    }
}
