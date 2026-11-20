//! Bounded DNS A lookup for the Ethernet resolver.
use crate::dhcp::{self, Address};

pub enum Answer {
    Address(Address),
    Alias([u8; 253], usize),
    Missing,
    Truncated,
    ServerError,
}

fn word(packet: &[u8], pos: usize) -> Option<u16> {
    Some(u16::from_be_bytes(
        packet.get(pos..pos + 2)?.try_into().ok()?,
    ))
}

fn name(packet: &[u8], mut pos: usize, out: &mut [u8; 253]) -> Option<(usize, usize)> {
    let mut end = None;
    let mut len = 0;
    for _ in 0..128 {
        let label = *packet.get(pos)?;
        if label == 0 {
            return Some((end.unwrap_or(pos + 1), len));
        }
        if label & 0xc0 == 0xc0 {
            let offset = (word(packet, pos)? & 0x3fff) as usize;
            if offset < 12 || offset >= pos {
                return None;
            }
            end.get_or_insert(pos + 2);
            pos = offset;
            continue;
        }
        if label > 63 {
            return None;
        }
        if len != 0 {
            *out.get_mut(len)? = b'.';
            len += 1;
        }
        let size = label as usize;
        let label = packet.get(pos + 1..pos + 1 + size)?;
        if !label
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || *b == b'-')
        {
            return None;
        }
        out.get_mut(len..len + size)?.copy_from_slice(label);
        len += size;
        pos += 1 + size;
    }
    None
}

pub fn response(packet: &[u8], id: u16, hostname: &[u8]) -> Option<Answer> {
    if word(packet, 0)? != id || word(packet, 4)? != 1 {
        return None;
    }
    let flags = word(packet, 2)?;
    if flags & 0xf800 != 0x8000 {
        return None;
    }
    let mut label = [0; 253];
    let (after, len) = name(packet, 12, &mut label)?;
    if !label[..len].eq_ignore_ascii_case(hostname)
        || word(packet, after)? != 1
        || word(packet, after + 2)? != 1
    {
        return None;
    }
    if flags & 0x0200 != 0 {
        return Some(Answer::Truncated);
    }
    match flags & 15 {
        0 => {}
        3 => return Some(Answer::Missing),
        _ => return Some(Answer::ServerError),
    }
    let count = word(packet, 6)? as usize;
    if count > packet.len() / 11 {
        return None;
    }
    let mut target = label;
    let mut target_len = len;
    for step in 0..=8 {
        let mut pos = after + 4;
        let mut alias = None;
        for _ in 0..count {
            let (header, owner_len) = name(packet, pos, &mut label)?;
            let kind = word(packet, header)?;
            let class = word(packet, header + 2)?;
            let size = word(packet, header + 8)? as usize;
            let data = header + 10;
            pos = data + size;
            let value = packet.get(data..pos)?;
            if class != 1 || !label[..owner_len].eq_ignore_ascii_case(&target[..target_len]) {
                continue;
            }
            if kind == 1 && size == 4 {
                let address = value.try_into().ok()?;
                if dhcp::unicast(address) {
                    return Some(Answer::Address(address));
                }
            }
            if kind == 5 && alias.is_none() {
                let (end, len) = name(packet, data, &mut label)?;
                if end != pos || len == 0 {
                    return None;
                }
                alias = Some((label, len));
            }
        }
        match alias {
            Some((next, len)) if step < 8 => {
                target = next;
                target_len = len;
            }
            None if step == 0 => return Some(Answer::Missing),
            None => return Some(Answer::Alias(target, target_len)),
            _ => return None,
        }
    }
    None
}
