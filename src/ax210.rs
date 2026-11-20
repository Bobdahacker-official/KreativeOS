//! Native AX210 discovery and Intel firmware staging. No device DMA is started.
use crate::{hardware, text::Text};
use core::fmt::Write;

pub const MAX_FIRMWARE: usize = 4 * 1024 * 1024;
pub static mut FIRMWARE: [u8; MAX_FIRMWARE] = [0; MAX_FIRMWARE];
pub static mut FIRMWARE_LEN: usize = 0;

pub struct Status {
    pub pci: Option<(u8, u8, u8)>,
    pub firmware: Option<FirmwareInfo>,
}

#[derive(Clone, Copy)]
pub struct FirmwareInfo {
    pub version: u32,
    pub runtime_sections: usize,
    pub init_sections: usize,
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
    let firmware = unsafe { parse(&FIRMWARE[..FIRMWARE_LEN]) };
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
    }
}

impl Status {
    pub fn describe(&self, message: &mut Text<128>) {
        match (self.pci, self.firmware) {
            (Some((bus, slot, function)), Some(_)) => {
                let _ = write!(
                    message,
                    "AX210 {:02x}:{:02x}.{} | firmware staged, radio not started",
                    bus, slot, function
                );
            }
            (Some(_), None) => message.set("AX210 detected | firmware missing or invalid"),
            (None, Some(_)) => message.set("AX210 firmware staged | no AX210 detected"),
            (None, None) => message.set("No AX210 detected"),
        }
    }
}
