//! Native AX210 discovery, firmware staging, and PCI ownership probing.
use crate::{hardware, text::Text};
use core::fmt::Write;

pub const MAX_FIRMWARE: usize = 4 * 1024 * 1024;
pub static mut FIRMWARE: [u8; MAX_FIRMWARE] = [0; MAX_FIRMWARE];
pub static mut FIRMWARE_LEN: usize = 0;

pub struct Status {
    pub pci: Option<(u8, u8, u8)>,
    pub firmware: Option<FirmwareInfo>,
    pub hardware: HardwareState,
}

#[derive(Clone, Copy)]
pub struct FirmwareInfo {
    pub version: u32,
    pub runtime_sections: usize,
    pub init_sections: usize,
}

#[derive(Clone, Copy)]
pub enum HardwareState {
    NotAttempted,
    BarUnavailable,
    UnrecognizedRadio,
    OwnershipTimeout,
    OwnershipReady,
}

pub static mut HARDWARE: HardwareState = HardwareState::NotAttempted;

pub unsafe fn staged_firmware() -> Option<FirmwareInfo> {
    parse(&FIRMWARE[..FIRMWARE_LEN])
}

/// Read identity and take the PCI ownership handshake from Intel's AX210
/// control registers, as used by FreeBSD iwlwifi. Release ownership before
/// returning because no firmware transport or interrupt handler is installed.
pub fn prepare_bar(
    read: impl Fn(u64) -> Option<u32>,
    write: impl Fn(u64, u32) -> bool,
    mut delay: impl FnMut(),
) -> HardwareState {
    const IF_CONFIG: u64 = 0x000;
    const HW_REV: u64 = 0x028;
    const RF_ID: u64 = 0x09c;
    const PCI_OWN: u32 = 1 << 22;

    let Some(revision) = read(HW_REV) else {
        return HardwareState::BarUnavailable;
    };
    let Some(radio) = read(RF_ID) else {
        return HardwareState::BarUnavailable;
    };
    // Typhoon Peak with a Garfield radio, matching the staged TY/GF image family.
    if revision & 0xfff0 != 0x0420 || radio & 0x00fff000 != 0x0010d000 {
        return HardwareState::UnrecognizedRadio;
    }
    let Some(original) = read(IF_CONFIG) else {
        return HardwareState::BarUnavailable;
    };
    if !write(IF_CONFIG, original | PCI_OWN) {
        return HardwareState::BarUnavailable;
    }
    let mut ready = false;
    for _ in 0..10 {
        if read(IF_CONFIG).is_some_and(|value| value & PCI_OWN != 0) {
            ready = true;
            break;
        }
        delay();
    }
    // Restore only the bit we set. Preserve any other hardware-updated bits.
    if original & PCI_OWN == 0 {
        if let Some(current) = read(IF_CONFIG) {
            write(IF_CONFIG, current & !PCI_OWN);
        }
    }
    if ready {
        HardwareState::OwnershipReady
    } else {
        HardwareState::OwnershipTimeout
    }
}

fn parse(bytes: &[u8]) -> Option<FirmwareInfo> {
    let word = |position: usize| {
        Some(u32::from_le_bytes(
            bytes.get(position..position + 4)?.try_into().ok()?,
        ))
    };
    if bytes.len() < 88 || word(0)? != 0 || word(4)? != 0x0a4c5749 {
        return None;
    }
    let mut info = FirmwareInfo {
        version: word(72)?,
        runtime_sections: 0,
        init_sections: 0,
    };
    let mut position = 88;
    while position < bytes.len() {
        let kind = word(position)?;
        let length = word(position + 4)? as usize;
        let start = position + 8;
        bytes.get(start..start.checked_add(length)?)?;
        let padded = length.checked_add(3)? & !3;
        position = start.checked_add(padded)?;
        if position > bytes.len() {
            return None;
        }
        match kind {
            19 | 24 if length > 4 => info.runtime_sections += 1,
            20 | 25 if length > 4 => info.init_sections += 1,
            _ => {}
        }
    }
    (info.runtime_sections != 0 && info.init_sections != 0).then_some(info)
}

pub fn probe() -> Status {
    let mut found = None;
    'scan: for bus in 0..=255 {
        for slot in 0..32 {
            for function in 0..8 {
                if unsafe { hardware::pci(bus, slot, function, 0) } == 0x2725_8086 {
                    found = Some((bus, slot, function));
                    break 'scan;
                }
            }
        }
    }
    let firmware = unsafe { staged_firmware() };
    if let Some(info) = firmware {
        let mut line: Text<128> = Text::new();
        let _ = write!(
            line,
            "KreativeOS: AX210 firmware staged ({:08x}, {} runtime, {} init)\r\n",
            info.version, info.runtime_sections, info.init_sections
        );
        unsafe { hardware::log(&line.bytes[..line.len]) };
    } else if unsafe { FIRMWARE_LEN != 0 } {
        unsafe { hardware::log(b"KreativeOS: AX210 firmware rejected\r\n") };
    }
    Status {
        pci: found,
        firmware,
        hardware: unsafe { HARDWARE },
    }
}

impl Status {
    pub fn describe(&self, message: &mut Text<128>) {
        match (self.pci, self.firmware) {
            (Some((bus, slot, function)), Some(_)) => {
                let _ = write!(
                    message,
                    "AX210 {:02x}:{:02x}.{} | {}",
                    bus,
                    slot,
                    function,
                    match self.hardware {
                        HardwareState::NotAttempted =>
                            "firmware staged, PCI handshake not attempted",
                        HardwareState::BarUnavailable => "firmware staged, BAR0 unavailable",
                        HardwareState::UnrecognizedRadio => "firmware staged, unmatched radio",
                        HardwareState::OwnershipTimeout =>
                            "firmware staged, PCI ownership timed out",
                        HardwareState::OwnershipReady =>
                            "PCI handshake passed, firmware not uploaded",
                    }
                );
            }
            (Some(_), None) => message.set("AX210 detected | firmware missing or invalid"),
            (None, Some(_)) => message.set("AX210 firmware staged | no AX210 detected"),
            (None, None) => message.set("No AX210 detected"),
        }
    }
}
