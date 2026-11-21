//! USB register access and RTL8821C firmware startup for the RTL8811CU.
use crate::{hardware, text::Text, wifi::AccessPoint};
use alloc::vec::Vec;
use core::fmt::Write;
use sha2::{Digest, Sha256};

#[path = "rtl8811cu_scan.rs"]
mod scan;
pub use scan::passive_scan;

pub const VENDOR: u16 = 0x0bda;
pub const PRODUCT: u16 = 0xc811;
const SYSTEM_CONFIGURATION: u16 = 0x00f0;
const FW_CONTROL: u16 = 0x0080;
const FW_READY: u32 = 0xc078;
const TX_DESCRIPTOR: usize = 48;
const CHUNK: usize = 4096;
const DDMA_CONTROL: u16 = 0x1208;
const DDMA_BUSY: u32 = 1 << 31;
const EFUSE_CONTROL: u16 = 0x0030;
const EFUSE_READY: u32 = 1 << 31;
const EFUSE_MAP_BYTES: usize = 512;
const EFUSE_PROGRAMMED_BYTES: usize = 512 - 96;
const FEATURE_MAILBOX: u16 = 0x01a0;
const FEATURE_REQUEST: u32 = 0xfd;
const FEATURE_REPORT: u32 = 0x19;

pub static mut FIRMWARE: [u8; 256 * 1024] = [0; 256 * 1024];
pub static mut FIRMWARE_LEN: usize = 0;
// linux-firmware tag 20250708, rtw88/rtw8821c_fw.bin.
const FIRMWARE_SHA256: [u8; 32] = [
    0x2e, 0xf4, 0x09, 0xbc, 0x41, 0x85, 0x49, 0xfc, 0xf2, 0x94, 0x06, 0x1d, 0xd0, 0xca, 0xe1, 0xfc,
    0x22, 0xfd, 0x9d, 0xa7, 0x9b, 0x60, 0x52, 0x49, 0x50, 0xb2, 0x5d, 0xe1, 0x87, 0x32, 0xf3, 0xf0,
];

pub trait Usb {
    fn control(&mut self, setup: [u8; 8], data: &mut [u8]) -> bool;
    fn bulk_out(&mut self, data: &[u8]) -> bool;
    fn receive(&mut self, _data: &mut [u8]) -> Result<Option<usize>> {
        Err("USB radio receive unavailable")
    }
}

type Result<T = ()> = core::result::Result<T, &'static str>;

struct Firmware<'a> {
    version: u16,
    revision: u8,
    sections: [(u32, &'a [u8]); 3],
}

impl<'a> Firmware<'a> {
    fn parse(bytes: &'a [u8]) -> Option<Self> {
        if bytes.len() < 64 || !bytes.starts_with(&[0x21, 0x88]) {
            return None;
        }
        let word = |at| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        let mut tail = &bytes[64..];
        let mut sections = [(0, &[][..]); 3];
        for (index, (address_offset, size_offset, range)) in [
            (32, 36, 0x20_0000..0x24_0000),
            (60, 48, 0x03_0000..0x05_0000),
            (56, 52, 0x10_0000..0x18_0000),
        ]
        .into_iter()
        .enumerate()
        {
            if index == 2 && bytes[24] & 16 == 0 {
                break;
            }
            let address = word(address_offset) & 0x7fff_ffff;
            let size = word(size_offset).checked_add(8)?;
            if size <= 8
                || !range.contains(&address)
                || address.checked_add(size)? > range.end
                || address & 3 != 0
                || size & 3 != 0
            {
                return None;
            }
            sections[index] = (address, tail.get(..size as usize)?);
            tail = tail.get(size as usize..)?;
        }
        tail.is_empty().then_some(Self {
            version: u16::from_le_bytes([bytes[4], bytes[5]]),
            revision: bytes[6],
            sections,
        })
    }
}

#[derive(Clone, Copy)]
pub enum Probe {
    RegisterUnavailable,
    FirmwareMissing(u8),
    FirmwareFailed(u8, &'static str),
    FirmwareRunning {
        cut: u8,
        version: u16,
        revision: u8,
        board: Result<Board>,
        capabilities: Result<Capabilities>,
    },
}

#[derive(Clone, Copy)]
pub struct Board {
    radio_front_end: u8,
    channel_plan: u8,
    crystal_trim: u8,
    package: u8,
    country: Option<[u8; 2]>,
}

#[derive(Clone, Copy)]
pub struct Capabilities {
    bandwidth: u8,
    antennas: u8,
}

fn decode_efuse(read: impl FnMut(usize) -> Result<u8>) -> Result<[u8; EFUSE_MAP_BYTES]> {
    let mut map = [u8::MAX; EFUSE_MAP_BYTES];
    let mut bytes = (0..EFUSE_PROGRAMMED_BYTES).map(read);
    while let Some(first) = bytes.next().transpose()? {
        if first == u8::MAX {
            break;
        }
        let (block, word_mask) = if first & 0x1f == 0x0f {
            let second = bytes
                .next()
                .ok_or("eFuse header crosses protected area")??;
            if second == u8::MAX {
                break;
            }
            ((second >> 4) << 3 | (first >> 5), second & 0x0f)
        } else {
            (first >> 4, first & 0x0f)
        };
        if word_mask == 0x0f {
            continue;
        }
        let start = usize::from(block) * 8;
        let words = map
            .get_mut(start..start + 8)
            .ok_or("eFuse block outside logical map")?;
        for (word, pair) in words.chunks_exact_mut(2).enumerate() {
            if word_mask & (1 << word) == 0 {
                for byte in pair {
                    *byte = bytes
                        .next()
                        .ok_or("eFuse record crosses protected area")??;
                }
            }
        }
    }
    Ok(map)
}

fn board_from_map(map: [u8; EFUSE_MAP_BYTES]) -> Result<Board> {
    let station: [u8; 6] = map[0x107..0x10d].try_into().unwrap();
    if station[0] & 1 != 0 || station == [0; 6] {
        return Err("factory station address invalid");
    }
    let country = [map[0xcb], map[0xcc]];
    if map[0xca] == u8::MAX {
        return Err("factory radio profile missing");
    }
    Ok(Board {
        radio_front_end: map[0xca] & 0x1f,
        channel_plan: map[0xb8],
        crystal_trim: map[0xb9],
        package: (map[0xca] >> 5) & 1,
        country: country
            .iter()
            .all(u8::is_ascii_uppercase)
            .then_some(country),
    })
}

pub fn start(usb: &mut impl Usb) -> Probe {
    let mut io = Registers(usb);
    let Ok(config) = io.read(SYSTEM_CONFIGURATION, 4) else {
        return Probe::RegisterUnavailable;
    };
    if config == 0 || config == u32::MAX {
        return Probe::RegisterUnavailable;
    }
    let cut = ((config >> 12) & 15) as u8;
    let bytes = unsafe { &FIRMWARE[..FIRMWARE_LEN] };
    if bytes.is_empty() {
        return Probe::FirmwareMissing(cut);
    }
    let Some(fw) = Firmware::parse(bytes).filter(|_| Sha256::digest(bytes)[..] == FIRMWARE_SHA256)
    else {
        return Probe::FirmwareFailed(cut, "unrecognized firmware image");
    };
    let startup = (|| {
        io.power_on()?;
        // The 3081 firmware publishes capabilities as it starts when requested here.
        io.write(FEATURE_MAILBOX, 1, FEATURE_REQUEST)?;
        io.download(&fw)
    })();
    match startup {
        Ok(()) => Probe::FirmwareRunning {
            cut,
            version: fw.version,
            revision: fw.revision,
            board: io.read_board(),
            capabilities: io.read_capabilities(),
        },
        Err(reason) => {
            let _ = io.cpu_enable(false);
            let _ = io.update(FW_CONTROL, 1, 1, 0);
            Probe::FirmwareFailed(cut, reason)
        }
    }
}

struct Registers<'a, T>(&'a mut T);

impl<T: Usb> Registers<'_, T> {
    fn register(&mut self, address: u16, buf: &mut [u8], input: bool) -> Result {
        let [low, high] = address.to_le_bytes();
        let direction = if input { 0xc0 } else { 0x40 };
        if !self
            .0
            .control([direction, 5, low, high, 0, 0, buf.len() as u8, 0], buf)
        {
            return Err("USB register transfer failed");
        }
        // A completion byte fences always-on register accesses on RTL8821C USB.
        if address <= 0xff || (0x1000..=0x10ff).contains(&address) {
            if !self
                .0
                .control([0x40, 5, 0xe0, 4, 0, 0, 1, 0], &mut buf[..1])
            {
                return Err("USB register fence failed");
            }
        }
        Ok(())
    }
    fn read(&mut self, address: u16, width: usize) -> Result<u32> {
        let mut bytes = [0; 4];
        self.register(address, &mut bytes[..width], true)?;
        Ok(u32::from_le_bytes(bytes))
    }
    fn write(&mut self, address: u16, width: usize, value: u32) -> Result {
        self.register(address, &mut value.to_le_bytes()[..width], false)
    }
    fn update(&mut self, address: u16, width: usize, mask: u32, value: u32) -> Result {
        let old = self.read(address, width)?;
        self.write(address, width, (old & !mask) | value)
    }
    fn poll(
        &mut self,
        address: u16,
        width: usize,
        mask: u32,
        expected: u32,
        error: &'static str,
    ) -> Result {
        self.wait_register(address, width, mask, expected, error)
            .map(|_| ())
    }
    fn wait_register(
        &mut self,
        address: u16,
        width: usize,
        mask: u32,
        expected: u32,
        error: &'static str,
    ) -> Result<u32> {
        let start = hardware::millis();
        loop {
            let value = self.read(address, width)?;
            if value & mask == expected {
                return Ok(value);
            }
            if hardware::millis().wrapping_sub(start) >= 250 {
                return Err(error);
            }
        }
    }
    fn read_board(&mut self) -> Result<Board> {
        let original_bank = self.read(0x0035, 1)? & 3;
        self.update(0x0035, 1, 3, 0)?;
        let result = (|| {
            // Select the Wi-Fi bank and keep the programming LDO off.
            self.update(0x0037, 1, 0x80, 0)?;
            let initial = self.read(EFUSE_CONTROL, 4)? & !(EFUSE_READY | 0x3ffff);
            let start = hardware::millis();
            let map = decode_efuse(|address| {
                if hardware::millis().wrapping_sub(start) >= 3000 {
                    return Err("eFuse read budget expired");
                }
                self.write(EFUSE_CONTROL, 4, initial | ((address as u32) << 8))?;
                let value = self.wait_register(
                    EFUSE_CONTROL,
                    4,
                    EFUSE_READY,
                    EFUSE_READY,
                    "eFuse read timed out",
                )?;
                Ok(value as u8)
            })?;
            board_from_map(map)
        })();
        let restore = self.update(0x0035, 1, 3, original_bank);
        let board = result?;
        restore?;
        Ok(board)
    }
    fn read_capabilities(&mut self) -> Result<Capabilities> {
        self.poll(
            FEATURE_MAILBOX,
            1,
            0xff,
            FEATURE_REPORT,
            "firmware capability report missing",
        )?;
        // The bandwidth and antenna fields share byte 6 of the event payload.
        let value = self.read(FEATURE_MAILBOX + 2 + 6, 1)?;
        self.write(FEATURE_MAILBOX, 1, 0)?;
        if value == 0xff {
            return Err("firmware capability report invalid");
        }
        Ok(Capabilities {
            bandwidth: match value & 7 {
                7 => 80,
                6 => 40,
                _ => 20,
            },
            antennas: ((value >> 5) & 7) as u8,
        })
    }
    fn power_on(&mut self) -> Result {
        self.write(0x001c, 1, 0)?; // Unlock system control.
        self.update(0x0064, 4, 0x3000_0000, 0x3000_0000)?;
        self.update(0x004c, 4, 0x0600_0000, 0)?;
        self.update(0x0040, 4, 4, 4)?;
        // Hold the baseband and RF blocks in reset during firmware loading.
        self.update(0x0002, 1, 3, 0)?;
        self.update(0x001f, 1, 7, 0)?;
        self.update(0x00ec, 4, 0x0700_0000, 0)?;
        if self.read(FW_CONTROL, 2)? == 0xc078 {
            let toggle = (self.read(0xfe58, 1)? ^ 0x80) & 0x80;
            self.write(0xfe58, 1, toggle)?;
        }
        if self.read(0x0100, 1)? == 0xea || self.read(0x00f5, 1)? & 1 != 0 {
            self.update(0x004a, 1, 1, 0)?;
            self.update(0x0005, 1, 0x98, 0)?;
            self.update(0x0020, 1, 1, 1)?;
            let start = hardware::micros();
            while hardware::micros().wrapping_sub(start) < 1000 {
                core::hint::spin_loop();
            }
            self.update(0x0000, 1, 0x20, 0)?;
            self.update(0x0005, 1, 0x1c, 0)?;
            self.poll(0x0006, 1, 2, 2, "MAC power did not stabilize")?;
            self.update(0x0006, 1, 1, 1)?;
            self.update(0x0005, 1, 0x98, 0)?;
            self.update(0x10c3, 1, 1, 1)?;
            self.update(0x0005, 1, 1, 1)?;
            self.poll(0x0005, 1, 1, 0, "MAC power-on did not finish")?;
            self.update(0x0020, 1, 8, 8)?;
            self.update(0x007c, 1, 2, 0)?;
            self.update(0x00f5, 1, 1, 0)?;
        }
        self.update(0x1080, 4, 0x0001_0100, 0x0001_0100)?;
        self.update(0x0003, 1, 0xd8, 0xd8)?;
        self.update(0x1103, 1, 0x0f, 0x0c)?;
        if self.read(FW_CONTROL, 4)? & (1 << 20) != 0 {
            self.update(FW_CONTROL, 4, 1 << 20, 0)?;
            self.update(0x0040, 4, 1 << 19, 0)?;
        }
        Ok(())
    }
    fn cpu_enable(&mut self, enabled: bool) -> Result {
        if enabled {
            self.update(0x001d, 1, 1, 1)?;
            self.update(0x0003, 1, 4, 4)
        } else {
            self.update(0x0003, 1, 4, 0)?;
            self.update(0x001d, 1, 1, 0)
        }
    }
    fn download(&mut self, firmware: &Firmware<'_>) -> Result {
        self.poll(0x1700, 4, 1 << 29, 1 << 29, "coexistence controller busy")?;
        self.write(0x1700, 4, 0x800f_0038)?;
        self.poll(0x1700, 4, 1 << 29, 1 << 29, "coexistence read timed out")?;
        let coexistence = self.read(0x1708, 4)?;
        self.cpu_enable(false)?;
        let saved = [
            (0x010d, 1, self.read(0x010d, 1)?),
            (0x0100, 2, self.read(0x0100, 2)?),
            (0x0204, 2, self.read(0x0204, 2)? | 0x8000),
            (0x0230, 2, self.read(0x0230, 2)?),
            (0x022c, 4, self.read(0x022c, 4)? | (1 << 31)),
            (0x0550, 1, self.read(0x0550, 1)?),
        ];
        let transfer = (|| {
            self.write(0x010d, 1, 0xc0)?; // Route the download to the high-priority FIFO.
            self.write(0x0100, 1, 5)?;
            self.write(0x1330, 4, 1 << 31)?;
            self.write(0x0230, 2, 0x200)?;
            self.write(0x022c, 4, saved[4].2)?;
            self.update(0x0550, 1, 8, 0x10)?;
            self.update(0x1082, 1, 1, 0)?;
            self.update(0x0009, 1, 0x40, 0)?;
            self.update(0x1082, 1, 1, 1)?;
            self.update(0x0009, 1, 0x40, 0x40)?;
            let fw_control = self.read(FW_CONTROL, 2)? & 0x3800;
            self.write(FW_CONTROL, 2, fw_control | 1)?;
            for &(address, section) in &firmware.sections {
                if !section.is_empty() {
                    self.upload_section(address, section)?;
                }
            }
            Ok(())
        })();
        let mut restore = Ok(());
        for (address, width, value) in saved {
            if let Err(error) = self.write(address, width, value) {
                restore = Err(error);
            }
        }
        transfer?;
        restore?;
        self.write(0x0210, 4, 4)?;
        self.update(FW_CONTROL, 2, 1, 1 << 14)?;
        self.cpu_enable(true)?;
        self.poll(0x1700, 4, 1 << 29, 1 << 29, "coexistence controller busy")?;
        self.write(0x1704, 4, coexistence)?;
        self.write(0x1700, 4, 0xc00f_0038)?;
        self.poll(
            FW_CONTROL,
            2,
            0xc078,
            FW_READY,
            "firmware did not acknowledge startup",
        )
    }
    fn upload_section(&mut self, address: u32, bytes: &[u8]) -> Result {
        let mut packet = [0u8; TX_DESCRIPTOR + CHUNK + 1];
        self.poll(DDMA_CONTROL, 4, DDMA_BUSY, 0, "firmware DMA busy")?;
        self.update(DDMA_CONTROL, 4, 1 << 25, 1 << 25)?;
        for (index, block) in bytes.chunks(CHUNK).enumerate() {
            let length = firmware_packet(&mut packet, block);
            self.write(0x0204, 2, 0x8000)?; // Clear the reserved-page completion bit.
            self.update(0x0101, 1, 1, 1)?;
            if !self.0.bulk_out(&packet[..length]) {
                return Err("USB firmware upload failed");
            }
            self.poll(
                0x0204,
                2,
                0x8000,
                0x8000,
                "firmware FIFO did not accept data",
            )?;
            self.write(0x1200, 4, 0x1878_0000 + TX_DESCRIPTOR as u32)?;
            self.write(0x1204, 4, address + (index * CHUNK) as u32)?;
            let continuation = if index == 0 { 0 } else { 1 << 24 };
            self.write(
                DDMA_CONTROL,
                4,
                DDMA_BUSY | (1 << 29) | continuation | block.len() as u32,
            )?;
            self.poll(DDMA_CONTROL, 4, DDMA_BUSY, 0, "firmware DMA timed out")?;
        }
        if self.read(DDMA_CONTROL, 4)? & (1 << 27) != 0 {
            return Err("firmware DMA checksum failed");
        }
        let flags = if address < 0x20_0000 { 0x18 } else { 0x60 };
        self.update(FW_CONTROL, 1, flags, flags)
    }
}

fn firmware_packet(packet: &mut [u8; TX_DESCRIPTOR + CHUNK + 1], block: &[u8]) -> usize {
    let padded = block.len() + usize::from((block.len() + TX_DESCRIPTOR) % 512 == 0);
    packet[..TX_DESCRIPTOR].fill(0);
    let header = (1u32 << 26) | ((TX_DESCRIPTOR as u32) << 16) | padded as u32;
    packet[..4].copy_from_slice(&header.to_le_bytes());
    packet[5] = 16; // Reserved page (beacon queue), with over-the-air beaconing disabled.
    let checksum = packet[..32].chunks_exact(2).fold(0u16, |sum, pair| {
        sum ^ u16::from_le_bytes([pair[0], pair[1]])
    });
    packet[28..30].copy_from_slice(&checksum.to_le_bytes());
    packet[TX_DESCRIPTOR..TX_DESCRIPTOR + block.len()].copy_from_slice(block);
    if padded != block.len() {
        packet[TX_DESCRIPTOR + block.len()] = 0;
    }
    TX_DESCRIPTOR + padded
}

impl Probe {
    pub fn describe(self, text: &mut Text<128>) {
        text.set("Realtek RTL8811CU");
        match self {
            Self::FirmwareMissing(cut) => {
                let _ = write!(text, " | cut {cut}, RTL8821C.BIN missing");
            }
            Self::FirmwareFailed(cut, error) => {
                let _ = write!(text, " | cut {cut}, {error}");
            }
            Self::FirmwareRunning {
                cut,
                version,
                revision,
                board,
                capabilities,
            } => {
                let _ = write!(text, " | cut {cut}, firmware {version}.{revision} started");
                if let Err(error) = board {
                    let _ = write!(text, " | {error}");
                } else if capabilities.is_err() {
                    text.append(" | capabilities unavailable");
                } else {
                    text.append(" | board data read");
                }
            }
            Self::RegisterUnavailable => text.append(" | register read failed"),
        }
    }
    pub fn board_details(self, text: &mut Text<128>) {
        if let Self::FirmwareRunning {
            board: Ok(board),
            capabilities,
            ..
        } = self
        {
            let _ = write!(
                text,
                "RTL8811CU board: RFE {}, channel plan {:#04x}, crystal trim {:#04x}",
                board.radio_front_end, board.channel_plan, board.crystal_trim
            );
            if let Some(country) = board.country {
                let _ = write!(
                    text,
                    ", country {}{}",
                    char::from(country[0]),
                    char::from(country[1])
                );
            }
            if let Ok(caps) = capabilities {
                let _ = write!(text, ", {} MHz", caps.bandwidth);
                if caps.antennas != 0 {
                    let _ = write!(text, ", {} antennas", caps.antennas);
                }
            }
        }
    }
}
