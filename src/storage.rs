//! KFS: alternating, checksummed snapshots on dedicated NVMe, SATA or IDE media.
use crate::{hardware::*, text::Text};
pub const COUNT: usize = 24;
pub const CAP: usize = 2048;
const SNAP: usize = 512 * 100;
const SNAP_SECTORS: usize = SNAP / 512;
const SNAP_A: u32 = 2048;
const SNAP_B: u32 = SNAP_A + SNAP_SECTORS as u32;
const DRAFT_SECTORS: usize = 5;
const DRAFT_START: u32 = SNAP_B + SNAP_SECTORS as u32;
pub const MARKER_LBA: u32 = 16;
pub const WRITE_LBAS: core::ops::Range<u32> = SNAP_A..DRAFT_START + 2 * DRAFT_SECTORS as u32;
const LABELS: [&[u8]; 2] = [
    b"KreativeOS KFS AHCI data disk\0\x01",
    b"KreativeOS KFS data disk\0\x01",
];
const EXFAT_LABEL: &[u8] = b"KreativeOS dedicated exFAT\0\x01";

#[derive(Clone, Copy)]
pub enum Media {
    Legacy,
    Exfat(u32),
}

impl Media {
    pub fn writable(self, base: u32, end: u64) -> bool {
        let range = match self {
            Self::Legacy => WRITE_LBAS,
            Self::Exfat(start) => start..start + crate::exfat::LENGTH,
        };
        range.contains(&base) && end <= u64::from(range.end)
    }
}

pub fn marked_volume(label: &[u8; 512], first: &[u8; 512], sectors: u64) -> Option<Media> {
    if first.iter().all(|&b| b == 0)
        && LABELS.iter().any(|marker| {
            label
                .strip_prefix(*marker)
                .is_some_and(|rest| rest.iter().all(|&b| b == 0))
        })
    {
        return Some(Media::Legacy);
    }
    if first[510..] != [0x55, 0xaa]
        || !label
            .strip_prefix(EXFAT_LABEL)
            .is_some_and(|rest| rest.iter().all(|&b| b == 0))
    {
        return None;
    }
    let mut found = None;
    let mut boot = false;
    for entry in first[446..510].chunks_exact(16) {
        if entry.iter().all(|&byte| byte == 0) {
            continue;
        }
        let start = u32::from_le_bytes(entry[8..12].try_into().unwrap());
        let length = u32::from_le_bytes(entry[12..16].try_into().unwrap());
        if entry[4] == 0x07
            && matches!(start, 2048 | 131072)
            && length == crate::exfat::LENGTH
            && u64::from(start + length) <= sectors
            && found.is_none()
        {
            found = Some(start);
        } else if entry[4] == 0xef && start == 2048 && length == 129024 && !boot {
            boot = true;
        } else {
            return None;
        }
    }
    found
        .filter(|&start| boot == (start == 131072))
        .map(Media::Exfat)
}

// Kernel MMIO currently uses the firmware's identity map.
pub unsafe fn identity_mmio(address: usize) -> bool {
    use core::{arch::asm, ptr};
    let (cr3, cr4): (usize, usize);
    asm!("mov {}, cr3", "mov {}, cr4", out(reg) cr3, out(reg) cr4, options(nostack));
    const PHYS: usize = 0x000f_ffff_ffff_f000;
    let mut table = cr3 & PHYS;
    let levels = if cr4 & (1 << 12) != 0 { 5 } else { 4 };
    if address >= 1 << (12 + 9 * levels - 1) {
        return false;
    }
    for level in (0..levels).rev() {
        let shift = 12 + 9 * level;
        let entry = ptr::read_volatile((table as *const usize).add((address >> shift) & 511));
        if entry & 3 != 3 {
            return false;
        }
        table = entry & PHYS;
        if level == 0 || (level <= 2 && entry & (1 << 7) != 0) {
            let mask = (1 << shift) - 1;
            return (table & !mask) == (address & !mask);
        }
    }
    false
}
#[derive(Clone, Copy)]
pub struct File {
    pub name: Text<32>,
    pub data: Text<CAP>,
    pub used: bool,
}
impl File {
    const fn new() -> Self {
        Self {
            name: Text::new(),
            data: Text::new(),
            used: false,
        }
    }
}
pub static mut FILES: [File; COUNT] = [File::new(); COUNT];
pub static mut DISK: bool = false;
enum Driver {
    Ide(Media),
    Ahci(crate::ahci::Disk),
    Nvme(crate::nvme::Disk),
    Usb(crate::usb::Disk),
}
static mut DRIVER: Option<Driver> = None;
static mut EXFAT: Option<crate::exfat::Volume> = None;

pub fn blocks(base: u32, data: &mut [u8], write: bool) -> bool {
    if data.is_empty() || data.len() % 512 != 0 {
        return false;
    }
    unsafe {
        if let Some(result) = dma_transfer(base, data, write) {
            return result;
        }
        if let Some(Driver::Ide(media)) = &DRIVER {
            if write && !media.writable(base, u64::from(base) + (data.len() / 512) as u64) {
                return false;
            }
            return data
                .chunks_exact_mut(512)
                .enumerate()
                .all(|(offset, buffer)| sector(base + offset as u32, buffer, write));
        }
        false
    }
}

fn media() -> Option<Media> {
    unsafe {
        match &DRIVER {
            Some(Driver::Ide(media)) => Some(*media),
            Some(Driver::Ahci(disk)) => disk.media,
            Some(Driver::Nvme(disk)) => disk.media,
            Some(Driver::Usb(disk)) => disk.media,
            None => None,
        }
    }
}

pub fn filesystem() -> &'static str {
    unsafe {
        if EXFAT.is_some() {
            "exFAT"
        } else if DISK {
            "KFS (legacy)"
        } else {
            "Memory"
        }
    }
}

unsafe fn dma_transfer(base: u32, data: &mut [u8], write: bool) -> Option<bool> {
    match &mut DRIVER {
        Some(Driver::Ahci(disk)) => Some(disk.transfer(base, data, write)),
        Some(Driver::Nvme(disk)) => Some(disk.transfer(base, data, write)),
        Some(Driver::Usb(disk)) => Some(disk.transfer(base, data, write)),
        _ => None,
    }
}
static mut GEN: u32 = 0;
static mut BUFFER: [u8; SNAP] = [0; SNAP];
static mut OTHER: [u8; SNAP] = [0; SNAP];
static mut DRAFT_GEN: Option<u32> = None;
unsafe fn ready(drq: bool) -> bool {
    for _ in 0..1000000 {
        let s = in8(0x1f7);
        if s == 255 || s == 0 {
            return false;
        }
        if s & 0x80 == 0 {
            if s & 0x21 != 0 {
                return false;
            }
            if !drq || s & 8 != 0 {
                return true;
            }
        }
    }
    false
}
unsafe fn sector(lba: u32, buf: &mut [u8], write: bool) -> bool {
    if !ready(false) {
        return false;
    }
    out8(0x1f6, 0xe0 | ((lba >> 24) as u8 & 15));
    for _ in 0..4 {
        in8(0x3f6);
    }
    out8(0x1f2, 1);
    out8(0x1f3, lba as u8);
    out8(0x1f4, (lba >> 8) as u8);
    out8(0x1f5, (lba >> 16) as u8);
    out8(0x1f7, if write { 0x30 } else { 0x20 });
    if !ready(true) {
        return false;
    }
    for i in 0..256 {
        if write {
            out16(0x1f0, u16::from_le_bytes([buf[2 * i], buf[2 * i + 1]]))
        } else {
            let v = in16(0x1f0).to_le_bytes();
            buf[2 * i] = v[0];
            buf[2 * i + 1] = v[1]
        }
    }
    if write {
        out8(0x1f7, 0xe7);
        ready(false)
    } else {
        true
    }
}
pub fn checksum(data: &[u8]) -> u32 {
    let mut h = 2166136261u32;
    for b in data {
        h = (h ^ *b as u32).wrapping_mul(16777619)
    }
    h
}
fn get32(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}
fn valid(b: &[u8]) -> bool {
    &b[..4] == b"KFS1" && get32(b, 8) == checksum(&b[512..])
}
fn newer(a: u32, b: u32) -> bool {
    (a.wrapping_sub(b) as i32) > 0
}

fn snapshot_base(generation: u32) -> u32 {
    if generation % 2 == 0 {
        SNAP_A
    } else {
        SNAP_B
    }
}

unsafe fn read_record(base: u32, record: &mut [u8]) -> bool {
    if let Some(status) = dma_transfer(base, record, false) {
        return status;
    }
    record
        .chunks_exact_mut(512)
        .enumerate()
        .all(|(index, sector_data)| sector(base + index as u32, sector_data, false))
}

unsafe fn publish_record(base: u32, record: &mut [u8]) -> bool {
    let (header, body) = record.split_at_mut(512);
    // Keep the other slot readable throughout this write. Publish the header last.
    if let Some(status) = dma_transfer(base, &mut [0; 512], true) {
        return status
            && dma_transfer(base + 1, body, true) == Some(true)
            && dma_transfer(base, header, true) == Some(true);
    }
    sector(base, &mut [0; 512], true)
        && body
            .chunks_exact_mut(512)
            .enumerate()
            .all(|(index, sector_data)| sector(base + index as u32 + 1, sector_data, true))
        && sector(base, header, true)
}
unsafe fn load(b: &[u8]) {
    GEN = get32(b, 4);
    for i in 0..COUNT {
        let p = 512 + i * 2088;
        let n = b[p] as usize;
        let len = u16::from_le_bytes([b[p + 1], b[p + 2]]) as usize;
        if n > 0 && n <= 32 && len <= CAP {
            let f = &mut FILES[i];
            f.used = true;
            f.name.len = n;
            f.name.bytes[..n].copy_from_slice(&b[p + 4..p + 4 + n]);
            f.data.len = len;
            f.data.bytes[..len].copy_from_slice(&b[p + 36..p + 36 + len]);
        }
    }
}
pub unsafe fn init() {
    DRIVER = crate::usb::discover()
        .map(Driver::Usb)
        .or_else(|| crate::nvme::discover().map(Driver::Nvme))
        .or_else(|| crate::ahci::discover().map(Driver::Ahci));
    if DRIVER.is_none() {
        // Legacy dedicated primary-master data disk.
        out8(0x1f6, 0xe0);
        let s = in8(0x1f7);
        if s != 0 && s != 255 {
            let mut first = [0; 512];
            let mut label = [0; 512];
            if sector(0, &mut first, false) && sector(MARKER_LBA, &mut label, false) {
                if let Some(kind) = marked_volume(&label, &first, u64::from(u32::MAX)) {
                    DRIVER = Some(Driver::Ide(kind));
                } else if first.iter().all(|&b| b == 0)
                    && [(SNAP_A, &mut BUFFER), (SNAP_B, &mut OTHER)]
                        .into_iter()
                        .any(|(lba, record)| {
                            record
                                .chunks_exact_mut(512)
                                .enumerate()
                                .all(|(i, buffer)| sector(lba + i as u32, buffer, false))
                                && valid(record)
                        })
                {
                    DRIVER = Some(Driver::Ide(Media::Legacy));
                }
            }
        }
    }
    DISK = DRIVER.is_some();
    if let Some(Media::Exfat(start)) = media() {
        EXFAT = crate::exfat::Volume::mount(start).filter(|volume| volume.load(&mut FILES));
        DISK = EXFAT.is_some();
        if !DISK {
            FILES.fill(File::new());
        }
    } else if DISK {
        let read_a = read_record(SNAP_A, &mut BUFFER);
        let read_b = read_record(SNAP_B, &mut OTHER);
        if read_a || read_b {
            let a = read_a && valid(&BUFFER);
            let b = read_b && valid(&OTHER);
            if a && (!b || !newer(get32(&OTHER, 4), get32(&BUFFER, 4))) {
                load(&BUFFER)
            } else if b {
                load(&OTHER)
            } else if !read_a
                || !read_b
                || BUFFER.iter().any(|&x| x != 0)
                || OTHER.iter().any(|&x| x != 0)
            {
                DISK = false
            }
        } else {
            DISK = false
        }
    }
    if find("Welcome.txt").is_none() {
        let _=put("Welcome.txt",b"Make something great.\n\nA quiet space for your next big idea.\n\nWrite a thought. Sketch a possibility. Open a new window.\n\nYour files stay on this computer, and your workspace is yours to make.\n\nWelcome to KreativeOS.");
    }
    if find("Guide.kapp").is_none() {
        let _ = put("Guide.kapp", include_bytes!("../packages/Guide.kapp"));
    }
    if find("Readme.html").is_none() {
        let _=put("Readme.html",b"<h1>KreativeOS Browser</h1><p>This page is stored locally.</p><p>Open http://example.com for an HTTP website.</p>");
    }
    if DISK {
        match DRIVER {
            Some(Driver::Nvme(_)) if EXFAT.is_some() => {
                log(b"KreativeOS: exFAT mounted through NVMe\r\n")
            }
            Some(Driver::Ahci(_)) if EXFAT.is_some() => {
                log(b"KreativeOS: exFAT mounted through AHCI\r\n")
            }
            Some(Driver::Ide(_)) if EXFAT.is_some() => {
                log(b"KreativeOS: exFAT mounted through IDE\r\n")
            }
            Some(Driver::Usb(_)) if EXFAT.is_some() => {
                log(b"KreativeOS: exFAT mounted through USB\r\n")
            }
            Some(Driver::Nvme(_)) => log(b"KreativeOS: KFS mounted through NVMe\r\n"),
            Some(Driver::Ahci(_)) => log(b"KreativeOS: KFS mounted through AHCI\r\n"),
            _ => {}
        }
    }
    log(b"KreativeOS: filesystem mounted\r\n");
}

pub fn backend() -> &'static str {
    unsafe {
        match (DISK, &DRIVER) {
            (true, Some(Driver::Nvme(_))) => "PCIe / NVMe",
            (true, Some(Driver::Ahci(_))) => "SATA / AHCI DMA",
            (true, Some(Driver::Ide(_))) => "IDE / ATA PIO",
            (true, Some(Driver::Usb(_))) => "USB / xHCI",
            (false, Some(Driver::Usb(_))) => "USB volume rejected",
            (false, Some(Driver::Nvme(_))) => "NVMe volume rejected",
            (false, Some(Driver::Ahci(_))) => "AHCI volume rejected",
            _ => "Memory only",
        }
    }
}

pub struct DriveDetails {
    pub bdf: (u8, u8, u8),
    pub location: &'static str,
    pub unit: u32,
    pub sectors: u64,
    pub model: Text<40>,
}

pub fn drive_details() -> Option<DriveDetails> {
    unsafe {
        match &DRIVER {
            Some(Driver::Nvme(disk)) => Some(DriveDetails {
                bdf: disk.bdf,
                location: "Namespace",
                unit: disk.namespace,
                sectors: disk.sectors,
                model: disk.model,
            }),
            Some(Driver::Ahci(disk)) => Some(DriveDetails {
                bdf: disk.bdf,
                location: "Port",
                unit: u32::from(disk.port),
                sectors: disk.sectors,
                model: disk.model,
            }),
            Some(Driver::Usb(disk)) => Some(DriveDetails {
                bdf: disk.bdf,
                location: "USB port",
                unit: u32::from(disk.port),
                sectors: disk.sectors,
                model: disk.model,
            }),
            _ => None,
        }
    }
}

pub fn used_slots() -> usize {
    unsafe { FILES.iter().filter(|file| file.used).count() }
}

pub fn previous_generation() -> Option<u32> {
    unsafe {
        if !DISK
            || EXFAT.is_some()
            || !read_record(snapshot_base(GEN.wrapping_add(1)), &mut OTHER)
            || !valid(&OTHER)
        {
            return None;
        }
        let generation = get32(&OTHER, 4);
        newer(GEN, generation).then_some(generation)
    }
}

pub fn restore_previous(expected: u32) -> bool {
    if previous_generation() != Some(expected) {
        return false;
    }
    unsafe {
        let next = GEN.wrapping_add(1);
        OTHER[4..8].copy_from_slice(&next.to_le_bytes());
        if !publish_record(snapshot_base(next), &mut OTHER) {
            return false;
        }
        GEN = next;
        true
    }
}

type DraftRecord = [u8; 512 * DRAFT_SECTORS];

fn draft_base(generation: u32) -> u32 {
    DRAFT_START + (generation % 2) * DRAFT_SECTORS as u32
}

fn draft_valid(record: &DraftRecord) -> bool {
    &record[..4] == b"KDR1"
        && get32(record, 8) == checksum(&record[12..])
        && record[12] <= 32
        && get32(record, 13) <= CAP as u32
}

pub fn load_draft() -> Option<(Text<32>, Text<CAP>)> {
    unsafe {
        if !DISK {
            return None;
        }
        let mut first = [0; 512 * DRAFT_SECTORS];
        let mut second = [0; 512 * DRAFT_SECTORS];
        let (a, b) = if let Some(volume) = &EXFAT {
            (volume.read_draft(&mut first) && draft_valid(&first), false)
        } else {
            (
                read_record(draft_base(0), &mut first) && draft_valid(&first),
                read_record(draft_base(1), &mut second) && draft_valid(&second),
            )
        };
        let record = if a && (!b || !newer(get32(&second, 4), get32(&first, 4))) {
            first
        } else if b {
            second
        } else {
            return None;
        };
        DRAFT_GEN = Some(get32(&record, 4));
        let mut name = Text::<32>::new();
        name.len = record[12] as usize;
        name.bytes[..name.len].copy_from_slice(&record[17..17 + name.len]);
        if name.str().is_empty() {
            return None;
        }
        let mut data = Text::<CAP>::new();
        data.len = get32(&record, 13) as usize;
        data.bytes[..data.len].copy_from_slice(&record[512..512 + data.len]);
        Some((name, data))
    }
}

pub fn store_draft(name: &Text<32>, data: &Text<CAP>) -> bool {
    unsafe {
        if !DISK {
            return false;
        }
        let next = DRAFT_GEN.map_or(0, |generation| generation.wrapping_add(1));
        let mut record = [0; 512 * DRAFT_SECTORS];
        record[..4].copy_from_slice(b"KDR1");
        record[4..8].copy_from_slice(&next.to_le_bytes());
        record[12] = name.len as u8;
        record[13..17].copy_from_slice(&(data.len as u32).to_le_bytes());
        record[17..17 + name.len].copy_from_slice(&name.bytes[..name.len]);
        record[512..512 + data.len].copy_from_slice(&data.bytes[..data.len]);
        let hash = checksum(&record[12..]);
        record[8..12].copy_from_slice(&hash.to_le_bytes());
        let success = if let Some(volume) = &mut EXFAT {
            volume.save_draft(&record)
        } else {
            publish_record(draft_base(next), &mut record)
        };
        if !success {
            return false;
        }
        DRAFT_GEN = Some(next);
        true
    }
}
pub fn find(name: &str) -> Option<usize> {
    unsafe {
        FILES.iter().position(|f| {
            f.used
                && (f.name.str() == name
                    || (EXFAT.is_some() && f.name.str().eq_ignore_ascii_case(name)))
        })
    }
}
pub fn put(name: &str, data: &[u8]) -> Result<(), &'static str> {
    if name.is_empty()
        || name.len() > 32
        || data.len() > CAP
        || !name.is_ascii()
        || name.contains('/')
        || name.contains('\\')
        || unsafe {
            EXFAT.is_some()
                && (!crate::exfat::valid_name(name)
                    || name.eq_ignore_ascii_case(crate::exfat::DRAFT))
        }
    {
        return Err("Invalid file name or size");
    }
    unsafe {
        let i = find(name)
            .or_else(|| FILES.iter().position(|f| !f.used))
            .ok_or("Disk directory full")?;
        let f = &mut FILES[i];
        f.name.set(name);
        f.data.clear();
        for &b in data {
            f.data.push(b)
        }
        f.used = true;
    }
    Ok(())
}
pub fn store_file(name: &str, data: &[u8]) -> bool {
    if put(name, data).is_err() {
        return false;
    }
    unsafe {
        if let Some(volume) = &mut EXFAT {
            return volume.update(name, Some(data));
        }
    }
    sync()
}

pub fn remove(name: &str) -> bool {
    if let Some(i) = find(name) {
        unsafe { FILES[i] = File::new() }
        true
    } else {
        false
    }
}
pub fn sync() -> bool {
    unsafe {
        if !DISK {
            return false;
        }
        if let Some(volume) = &mut EXFAT {
            return volume.save(&FILES);
        }
        BUFFER.fill(0);
        for i in 0..COUNT {
            let f = &FILES[i];
            if !f.used {
                continue;
            }
            let p = 512 + i * 2088;
            BUFFER[p] = f.name.len as u8;
            BUFFER[p + 1..p + 3].copy_from_slice(&(f.data.len as u16).to_le_bytes());
            BUFFER[p + 4..p + 4 + f.name.len].copy_from_slice(&f.name.bytes[..f.name.len]);
            BUFFER[p + 36..p + 36 + f.data.len].copy_from_slice(&f.data.bytes[..f.data.len]);
        }
        let next = GEN.wrapping_add(1);
        BUFFER[..4].copy_from_slice(b"KFS1");
        BUFFER[4..8].copy_from_slice(&next.to_le_bytes());
        let hash = checksum(&BUFFER[512..]);
        BUFFER[8..12].copy_from_slice(&hash.to_le_bytes());
        if !publish_record(snapshot_base(next), &mut BUFFER) {
            return false;
        }
        GEN = next;
        true
    }
}
