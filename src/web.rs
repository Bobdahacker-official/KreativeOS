//! Bounded HTTP metadata and binary response handling for the native browser.
use crate::text::Text;
use alloc::vec::Vec;

pub const MAX_RESPONSE: usize = 768 * 1024;
const MAX_HEADER: usize = 16 * 1024;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Media {
    Text,
    Image,
    Audio,
}
pub struct Response {
    pub media: Media,
    pub bytes: Vec<u8>,
    pub location: Text<384>,
}
struct Header {
    offset: usize,
    status: u16,
    size: Option<usize>,
    chunked: bool,
    media: Media,
    location: Text<384>,
}

fn header(bytes: &[u8]) -> Option<Header> {
    let pos = bytes
        .windows(4)
        .take(MAX_HEADER)
        .position(|w| w == b"\r\n\r\n")?;
    let text = core::str::from_utf8(&bytes[..pos]).ok()?;
    let mut lines = text.split("\r\n");
    let start = lines.next()?;
    if !start.starts_with("HTTP/1.") {
        return None;
    }
    let status = start.split_ascii_whitespace().nth(1)?.parse().ok()?;
    let mut h = Header {
        offset: pos + 4,
        status,
        size: None,
        chunked: false,
        media: Media::Text,
        location: Text::new(),
    };
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        if name.eq_ignore_ascii_case("Content-Length") {
            let size = value.parse().ok()?;
            if h.size.is_some_and(|existing| existing != size) {
                return None;
            }
            h.size = Some(size);
        } else if name.eq_ignore_ascii_case("Transfer-Encoding") {
            if !value.eq_ignore_ascii_case("chunked") {
                return None;
            }
            h.chunked = true;
        } else if name.eq_ignore_ascii_case("Content-Encoding")
            && !value.eq_ignore_ascii_case("identity")
        {
            return None;
        } else if name.eq_ignore_ascii_case("Content-Type") {
            let value = value.split(';').next()?.trim();
            if value
                .get(..6)
                .is_some_and(|s| s.eq_ignore_ascii_case("image/"))
            {
                h.media = Media::Image;
            }
            if value
                .get(..6)
                .is_some_and(|s| s.eq_ignore_ascii_case("audio/"))
            {
                h.media = Media::Audio;
            }
        } else if name.eq_ignore_ascii_case("Location") && value.len() < 384 {
            h.location.set(value);
        }
    }
    Some(h)
}

fn consume_chunks(body: &[u8], mut out: Option<&mut Vec<u8>>) -> Option<()> {
    let mut offset = 0;
    while offset < body.len() {
        let line_end = body[offset..]
            .windows(2)
            .take(96)
            .position(|w| w == b"\r\n")?
            + offset;
        let text = core::str::from_utf8(&body[offset..line_end]).ok()?;
        let size = usize::from_str_radix(text.split(';').next()?.trim(), 16).ok()?;
        offset = line_end + 2;
        if size == 0 {
            return (body.get(offset..offset + 2) == Some(b"\r\n")
                || body[offset..].windows(4).any(|w| w == b"\r\n\r\n"))
            .then_some(());
        }
        let next = offset.checked_add(size)?;
        let payload = body.get(offset..next)?;
        if body.get(next..next.checked_add(2)?) != Some(b"\r\n") {
            return None;
        }
        if let Some(out) = out.as_deref_mut() {
            out.extend_from_slice(payload);
        }
        offset = next + 2;
    }
    None
}

pub fn complete(bytes: &[u8]) -> bool {
    let Some(h) = header(bytes) else { return false };
    if h.chunked {
        consume_chunks(&bytes[h.offset..], None).is_some()
    } else {
        h.size.is_some_and(|n| bytes.len() - h.offset >= n)
    }
}
pub fn parse(mut bytes: Vec<u8>) -> Option<Response> {
    let h = header(&bytes)?;
    if !(200..400).contains(&h.status) {
        return None;
    }
    if (300..400).contains(&h.status) && h.location.len == 0 {
        return None;
    }
    if h.chunked {
        let mut data = Vec::new();
        consume_chunks(&bytes[h.offset..], Some(&mut data))?;
        bytes = data;
    } else {
        let size = h.size.unwrap_or(bytes.len() - h.offset);
        let end = h.offset.checked_add(size)?;
        if end > bytes.len() {
            return None;
        }
        bytes.copy_within(h.offset..end, 0);
        bytes.truncate(size);
    }
    let media = if bytes.starts_with(b"\x89PNG\r\n\x1a\n")
        || bytes.starts_with(&[0xff, 0xd8])
        || bytes.starts_with(b"BM")
    {
        Media::Image
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WAVE") {
        Media::Audio
    } else {
        h.media
    };
    Some(Response {
        media,
        bytes,
        location: h.location,
    })
}

/// Resolve an HTTPS-only resource from the current document. Fragments stay client-side.
pub fn resolve(base: &str, src: &str) -> Option<Text<384>> {
    let src = src.split('#').next()?.trim();
    if src.is_empty() || src.len() > 350 || !src.bytes().all(|b| (33..127).contains(&b)) {
        return None;
    }
    let mut out = Text::new();
    if src.starts_with("https://") {
        out.set(src);
        return Some(out);
    }
    if src.starts_with("//") {
        out.append("https:");
        out.append(src);
        return Some(out);
    }
    if src.split('/').next()?.contains(':') {
        return None;
    }
    let protocol = base.strip_prefix("https://")?;
    let authority = protocol.split('/').next()?;
    out.set("https://");
    out.append(authority);
    let root_len = out.len;
    let base_path = protocol.strip_prefix(authority)?.split('?').next()?;
    if !src.starts_with('/') {
        if let Some((folder, _)) = base_path.rsplit_once('/') {
            out.append(folder);
        }
    }
    for segment in src.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                if let Some(folder) = out.str()[root_len..].rfind('/') {
                    out.len = root_len + folder;
                }
            }
            value => {
                if out.len + value.len() + 1 >= 384 {
                    return None;
                }
                out.push(b'/');
                out.append(value);
            }
        }
    }
    if out.len == root_len || src.ends_with('/') {
        out.push(b'/');
    }
    Some(out)
}

fn src_attr(mut text: &str) -> Option<&str> {
    while !text.is_empty() {
        text = text.trim_start_matches(|c: char| c.is_ascii_whitespace() || c == '/');
        let end = text
            .find(|c: char| c == '=' || c.is_ascii_whitespace())
            .unwrap_or(text.len());
        if end == 0 {
            return None;
        }
        let name = &text[..end];
        text = text[end..].trim_start();
        let Some(rest) = text.strip_prefix('=') else {
            continue;
        };
        text = rest.trim_start();
        let (value, tail) = if let Some(q) = text.chars().next().filter(|c| *c == '\'' || *c == '"')
        {
            let (value, tail) = text[1..].split_once(q)?;
            (value, tail)
        } else {
            let stop = text
                .find(|c: char| c.is_ascii_whitespace())
                .unwrap_or(text.len());
            (&text[..stop], &text[stop..])
        };
        if name.eq_ignore_ascii_case("src") {
            return Some(value);
        }
        text = tail;
    }
    None
}

pub fn media_urls(url: &str, html: &str) -> (Option<Text<384>>, Option<Text<384>>) {
    let mut image = None;
    let mut audio = None;
    let mut in_audio = false;
    for part in html.split('<').skip(1) {
        let Some((tag, _)) = part.split_once('>') else {
            continue;
        };
        let end = tag
            .find(|c: char| c.is_ascii_whitespace() || c == '/')
            .unwrap_or(tag.len());
        let name = &tag[..end];
        if tag.starts_with('/') && tag[1..].trim().eq_ignore_ascii_case("audio") {
            in_audio = false;
        }
        if name.eq_ignore_ascii_case("audio") {
            in_audio = true;
        }
        let src = src_attr(&tag[end..]).and_then(|src| resolve(url, src));
        if image.is_none() && name.eq_ignore_ascii_case("img") {
            image = src;
        }
        if audio.is_none()
            && (name.eq_ignore_ascii_case("audio")
                || (in_audio && name.eq_ignore_ascii_case("source")))
        {
            audio = src;
        }
        if image.is_some() && audio.is_some() {
            break;
        }
    }
    (image, audio)
}
