//! Small, single-disk ZIP archives using stored or raw DEFLATE entries.
use crate::storage;
use alloc::{string::String, vec::Vec};

pub struct Entry {
    pub name: String,
    bytes: Vec<u8>,
}

fn word(bytes: &[u8], offset: usize) -> Option<usize> {
    Some(u16::from_le_bytes(bytes.get(offset..offset + 2)?.try_into().ok()?) as usize)
}
fn dword(bytes: &[u8], offset: usize) -> Option<usize> {
    Some(u32::from_le_bytes(bytes.get(offset..offset + 4)?.try_into().ok()?) as usize)
}
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb88320 & 0u32.wrapping_sub(crc & 1));
        }
    }
    !crc
}

pub fn read(data: &[u8]) -> Option<Vec<Entry>> {
    let end = (0..=data.len().checked_sub(22)?).rev().find(|&offset| {
        data.get(offset..offset + 4) == Some(b"PK\x05\x06")
            && word(data, offset + 20) == Some(data.len() - offset - 22)
    })?;
    let footer = &data[end..];
    let count = word(footer, 10)?;
    if count > storage::COUNT || dword(footer, 4)? != 0 || word(footer, 8)? != count {
        return None;
    }
    let start = dword(footer, 16)?;
    if start.checked_add(dword(footer, 12)?)? != end {
        return None;
    }
    let mut directory = data.get(start..end)?;
    let mut entries = Vec::<Entry>::new();
    for _ in 0..count {
        if directory.get(..4)? != b"PK\x01\x02" || word(directory, 34)? != 0 {
            return None;
        }
        let flags = word(directory, 8)?;
        let method = word(directory, 10)?;
        let packed = dword(directory, 20)?;
        let size = dword(directory, 24)?;
        let name_len = word(directory, 28)?;
        let name_end = 46 + name_len;
        let name = core::str::from_utf8(directory.get(46..name_end)?).ok()?;
        if !(1..=32).contains(&name.len())
            || !name.bytes().all(|b| (32..127).contains(&b))
            || name.contains(['/', '\\', ':'])
            || [".", ".."].contains(&name)
            || size > storage::CAP
            || entries.iter().any(|entry| entry.name == name)
            || flags & !0x080e != 0
        {
            return None;
        }
        let local_offset = dword(directory, 42)?;
        let local = data.get(local_offset..start)?;
        if local.get(..4)? != b"PK\x03\x04"
            || word(local, 6)? != flags
            || word(local, 8)? != method
            || word(local, 26)? != name_len
            || local.get(30..30 + name_len)? != name.as_bytes()
        {
            return None;
        }
        let offset = 30 + name_len + word(local, 28)?;
        let compressed = local.get(offset..offset.checked_add(packed)?)?;
        let bytes = match method {
            0 if packed == size => compressed.to_vec(),
            8 => miniz_oxide::inflate::decompress_to_vec_with_limit(compressed, size).ok()?,
            _ => return None,
        };
        if bytes.len() != size || crc32(&bytes) as usize != dword(directory, 16)? {
            return None;
        }
        entries.push(Entry {
            name: String::from(name),
            bytes,
        });
        directory = directory.get(name_end + word(directory, 30)? + word(directory, 32)?..)?;
    }
    directory.is_empty().then_some(entries)
}

pub fn extract(entries: &[Entry]) -> Result<bool, &'static str> {
    let available = unsafe { storage::FILES.iter().filter(|file| !file.used).count() };
    if entries.len() > available
        || entries
            .iter()
            .any(|entry| storage::find(&entry.name).is_some())
    {
        return Err("Extraction needs free file slots and unique names");
    }
    for entry in entries {
        storage::put(&entry.name, &entry.bytes)?;
    }
    Ok(storage::sync())
}
