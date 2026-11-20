//! Host-authored documents and terminal scripts staged by UEFI.
use crate::storage;
use alloc::vec::Vec;
use core::{fmt::Write, str};

const LIMIT: usize = 1024 * 1024;
pub static mut PACK: [u8; LIMIT] = [0; LIMIT];
pub static mut STAGED: usize = 0;

fn entries() -> Result<Vec<(&'static str, &'static [u8])>, &'static str> {
    let data = unsafe { &PACK[..STAGED] };
    let (mut rest, wide) = if let Some(rest) = data.strip_prefix(b"KDP2") {
        (rest, true)
    } else {
        (
            data.strip_prefix(b"KDP1")
                .ok_or("No developer pack staged")?,
            false,
        )
    };
    let mut files = Vec::new();
    while !rest.is_empty() {
        let invalid = "Invalid developer pack header";
        let (name_len, size, payload) = if wide {
            let (&[name, a, b, c, d], payload) = rest.split_first_chunk::<5>().ok_or(invalid)?;
            (name, u32::from_le_bytes([a, b, c, d]) as usize, payload)
        } else {
            let (&[name, lo, hi], payload) = rest.split_first_chunk::<3>().ok_or(invalid)?;
            (name, u16::from_le_bytes([lo, hi]) as usize, payload)
        };
        if size
            > if wide {
                storage::CAP
            } else {
                storage::LEGACY_CAP
            }
        {
            return Err("Developer file is too large");
        }
        let len = name_len as usize;
        let name = payload
            .get(..len)
            .and_then(|bytes| str::from_utf8(bytes).ok())
            .filter(|name| {
                (1..=32).contains(&len)
                    && name
                        .bytes()
                        .all(|b| (32..127).contains(&b) && b != b'/' && b != b'\\')
                    && [".txt", ".html", ".ksh", ".kr3"]
                        .iter()
                        .any(|suffix| name.ends_with(suffix))
            })
            .ok_or("Invalid developer filename")?;
        let content = payload
            .get(len..len + size)
            .filter(|bytes| {
                if name.ends_with(".kr3") {
                    bytes.starts_with(b"KR3\x01") && (5..=65_540).contains(&bytes.len())
                } else {
                    str::from_utf8(bytes).is_ok()
                }
            })
            .ok_or("Invalid developer file content")?;
        if files.len() == storage::LEGACY_COUNT
            || files.iter().any(|&(existing, _)| existing == name)
        {
            return Err("Too many files or duplicate filenames");
        }
        files.push((name, content));
        rest = &payload[len + size..];
    }
    Ok(files)
}

pub fn command(arg: &str, out: &mut impl Write) {
    if arg != "list" && arg != "import" {
        let _ = writeln!(out, "Use dev list or dev import");
        return;
    }
    let files = match entries() {
        Ok(files) => files,
        Err(error) => {
            let _ = writeln!(out, "{error}");
            return;
        }
    };
    if arg == "list" {
        for (name, bytes) in files {
            let _ = writeln!(out, "{name}   {} bytes", bytes.len());
        }
        return;
    }
    let (capacity, max_size) = if storage::has_exfat() {
        (storage::COUNT, storage::CAP)
    } else {
        (storage::LEGACY_COUNT, storage::LEGACY_CAP)
    };
    let free = capacity.saturating_sub(storage::used_slots());
    if files
        .iter()
        .any(|(name, bytes)| storage::find(name).is_none() && bytes.len() > max_size)
    {
        let _ = writeln!(
            out,
            "A staged file exceeds the mounted volume's file limit."
        );
        return;
    }
    let additions = files
        .iter()
        .filter(|(name, _)| storage::find(name).is_none())
        .count();
    if additions > free {
        let _ = writeln!(
            out,
            "Import requires {additions} free file slots. Only {free} available."
        );
        return;
    }
    let skipped = files.len() - additions;
    if additions == 0 {
        let _ = writeln!(out, "No new files. Kept {skipped} existing files.");
        return;
    }
    for (name, bytes) in files {
        if storage::find(name).is_none() {
            if let Err(error) = storage::put(name, bytes) {
                let _ = writeln!(out, "{error}");
                return;
            }
        }
    }
    let location = if storage::sync() {
        "to disk"
    } else {
        "in memory (disk save failed)"
    };
    let _ = writeln!(
        out,
        "Imported {additions} files {location}. Kept {skipped} existing files."
    );
}
