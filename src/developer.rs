//! Host-authored documents and terminal scripts staged by UEFI.
use crate::storage;
use alloc::vec::Vec;
use core::{fmt::Write, str};

const LIMIT: usize = 4 + storage::COUNT * (3 + 32 + storage::CAP);
pub static mut PACK: [u8; LIMIT] = [0; LIMIT];
pub static mut STAGED: usize = 0;

fn entries() -> Result<Vec<(&'static str, &'static [u8])>, &'static str> {
    let data = unsafe { &PACK[..STAGED] };
    let mut rest = data
        .strip_prefix(b"KDP1")
        .ok_or("No developer pack staged")?;
    let mut files = Vec::new();
    while !rest.is_empty() {
        let Some((&[name_len, lo, hi], payload)) = rest.split_first_chunk::<3>() else {
            return Err("Invalid developer pack header");
        };
        let size = u16::from_le_bytes([lo, hi]) as usize;
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
                size <= storage::CAP
                    && if name.ends_with(".kr3") {
                        bytes.starts_with(b"KR3\x01") && bytes.len() > 4
                    } else {
                        str::from_utf8(bytes).is_ok()
                    }
            })
            .ok_or("Invalid developer file content")?;
        if files.len() == storage::COUNT || files.iter().any(|&(existing, _)| existing == name) {
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
    let free = unsafe { storage::FILES.iter().filter(|file| !file.used).count() };
    let additions = files
        .iter()
        .filter(|(name, _)| storage::find(name).is_none())
        .count();
    if additions > free {
        let _ = writeln!(
            out,
            "Import requires {additions} free KFS slots. Only {free} available."
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
