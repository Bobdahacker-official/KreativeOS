//! Read-only discovery of partitioned and whole-device exFAT data volumes.
use crate::storage::Media;

const BASIC_DATA: [u8; 16] = [
    0xa2, 0xa0, 0xd0, 0xeb, 0xe5, 0xb9, 0x33, 0x44, 0x87, 0xc0, 0x68, 0xb6, 0xb7, 0x26, 0x99, 0xc7,
];
const LEGACY: [&[u8]; 2] = [
    b"KreativeOS KFS AHCI data disk\0\x01",
    b"KreativeOS KFS data disk\0\x01",
];

fn dword(data: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(data[at..at + 4].try_into().unwrap())
}
fn qword(data: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(data[at..at + 8].try_into().unwrap())
}

fn crc32(data: &[u8]) -> u32 {
    !data.iter().fold(!0u32, |mut crc, &byte| {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & (0u32.wrapping_sub(crc & 1)));
        }
        crc
    })
}

pub fn probe(
    sectors: u64,
    sector: usize,
    mut read: impl FnMut(u64, &mut [u8]) -> bool,
) -> Option<Media> {
    let mut block = alloc::vec![0; sector];
    if !read(0, &mut block) {
        return None;
    }
    if sector == 512
        && sectors >= u64::from(crate::storage::WRITE_LBAS.end)
        && block.iter().all(|&byte| byte == 0)
        && read(16, &mut block)
        && LEGACY.iter().any(|marker| {
            block
                .strip_prefix(*marker)
                .is_some_and(|tail| tail.iter().all(|&byte| byte == 0))
        })
    {
        return Some(Media::Legacy);
    }
    if !read(0, &mut block) {
        return None;
    }
    if &block[3..11] == b"EXFAT   " {
        return choose([(0, sectors)], sector, &mut read);
    }
    if block[510..512] != [0x55, 0xaa] {
        return None;
    }
    let records = &block[446..510];
    if records.chunks_exact(16).any(|record| record[4] == 0xee) {
        for header in [1, sectors.checked_sub(1)?] {
            if let Some(entries) = gpt(header, sectors, sector, &mut read) {
                return choose(entries, sector, &mut read);
            }
        }
    } else {
        let mut ranges = alloc::vec::Vec::new();
        for record in records.chunks_exact(16).filter(|record| record[4] != 0) {
            let start = u64::from(dword(record, 8));
            let count = u64::from(dword(record, 12));
            if start == 0 || count == 0 || start + count > sectors {
                return None;
            }
            ranges.push((start, count, record[4]));
        }
        ranges.sort_unstable_by_key(|entry| entry.0);
        if ranges
            .windows(2)
            .any(|pair| pair[0].0 + pair[0].1 > pair[1].0)
        {
            return None;
        }
        return choose(
            ranges
                .into_iter()
                .filter_map(|(start, count, kind)| (kind == 7).then_some((start, count))),
            sector,
            &mut read,
        );
    }
    None
}

fn choose(
    ranges: impl IntoIterator<Item = (u64, u64)>,
    sector: usize,
    read: &mut impl FnMut(u64, &mut [u8]) -> bool,
) -> Option<Media> {
    let mut fallback = None;
    for (start, count) in ranges {
        if let Some(volume) = crate::exfat::Geometry::probe(start, count, sector, read) {
            if volume.opted_in(read) {
                return Some(Media::Exfat {
                    volume,
                    writable: true,
                });
            }
            fallback.get_or_insert(Media::Exfat {
                volume,
                writable: false,
            });
        }
    }
    fallback
}

fn gpt(
    lba: u64,
    sectors: u64,
    sector: usize,
    read: &mut impl FnMut(u64, &mut [u8]) -> bool,
) -> Option<alloc::vec::Vec<(u64, u64)>> {
    let mut header = alloc::vec![0; sector];
    if !read(lba, &mut header) || &header[..8] != b"EFI PART" {
        return None;
    }
    let length = dword(&header, 12) as usize;
    let expected = dword(&header, 16);
    let peer = qword(&header, 32);
    let first = qword(&header, 40);
    let last = qword(&header, 48);
    let array = qword(&header, 72);
    let count = dword(&header, 80) as usize;
    let size = dword(&header, 84) as usize;
    let bytes = count.checked_mul(size)?;
    let array_blocks = bytes.div_ceil(sector);
    let array_end = array.checked_add(array_blocks as u64)?;
    if !(92..=sector).contains(&length)
        || dword(&header, 8) != 0x0001_0000
        || qword(&header, 24) != lba
        || peer != if lba == 1 { sectors - 1 } else { 1 }
        || first < 2
        || last < first
        || last >= sectors - 1
        || !(128..=4096).contains(&size)
        || !(size / 128).is_power_of_two()
        || size % 128 != 0
        || !(1..=1024 * 1024).contains(&bytes)
        || (lba == 1 && (array <= lba || array_end > first))
        || (lba != 1 && (array <= last || array_end > lba))
    {
        return None;
    }
    header[16..20].fill(0);
    if crc32(&header[..length]) != expected {
        return None;
    }
    let mut table = alloc::vec![0; array_blocks * sector];
    if !table
        .chunks_exact_mut(sector)
        .enumerate()
        .all(|(index, block)| read(array + index as u64, block))
        || crc32(&table[..bytes]) != dword(&header, 88)
    {
        return None;
    }
    let mut ranges = alloc::vec::Vec::new();
    for entry in table[..bytes]
        .chunks_exact(size)
        .filter(|entry| entry[..16] != [0; 16])
    {
        let start = qword(entry, 32);
        let end = qword(entry, 40);
        if start < first || end > last || start > end {
            return None;
        }
        ranges.push((
            start,
            end,
            entry[..16] == BASIC_DATA && qword(entry, 48) & (1 << 60) == 0,
        ));
    }
    ranges.sort_unstable_by_key(|entry| entry.0);
    if ranges.windows(2).any(|pair| pair[0].1 >= pair[1].0) {
        return None;
    }
    Some(
        ranges
            .into_iter()
            .filter_map(|(start, end, data)| data.then_some((start, end - start + 1)))
            .collect(),
    )
}
