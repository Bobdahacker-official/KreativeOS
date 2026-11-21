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
    gzip: bool,
    unsupported_encoding: bool,
    media: Media,
    location: Text<384>,
}

fn header(bytes: &[u8]) -> Option<Header> {
    let mut offset = 0;
    for _ in 0..8 {
        let mut h = header_at(bytes.get(offset..)?)?;
        offset += h.offset;
        if offset > MAX_HEADER {
            return None;
        }
        if h.status >= 200 {
            h.offset = offset;
            return Some(h);
        }
        if h.status == 101 || h.status < 100 {
            return None;
        }
    }
    None
}

fn header_at(bytes: &[u8]) -> Option<Header> {
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
        gzip: false,
        unsupported_encoding: false,
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
            if h.chunked || !value.eq_ignore_ascii_case("chunked") {
                return None;
            }
            h.chunked = true;
        } else if name.eq_ignore_ascii_case("Content-Encoding") {
            if value.eq_ignore_ascii_case("gzip") && !h.gzip {
                h.gzip = true;
            } else if !value.eq_ignore_ascii_case("identity") {
                h.unsupported_encoding = true;
            }
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
    if h.chunked && h.size.is_some() {
        return None;
    }
    Some(h)
}

impl Header {
    fn redirect(&self) -> bool {
        matches!(self.status, 301 | 302 | 303 | 307 | 308)
    }

    fn has_no_body(&self) -> bool {
        self.redirect() || matches!(self.status, 204 | 205) || !(200..300).contains(&self.status)
    }
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
    if h.has_no_body() {
        true
    } else if h.chunked {
        consume_chunks(&bytes[h.offset..], None).is_some()
    } else {
        h.size.is_some_and(|n| bytes.len() - h.offset >= n)
    }
}
pub fn parse(mut bytes: Vec<u8>) -> Option<Response> {
    if bytes.len() > MAX_RESPONSE {
        return None;
    }
    let h = header(&bytes)?;
    if h.redirect() {
        return (h.location.len != 0).then_some(Response {
            media: Media::Text,
            bytes: Vec::new(),
            location: h.location,
        });
    }
    if !(200..300).contains(&h.status) || (h.unsupported_encoding && !h.has_no_body()) {
        return None;
    }
    if h.has_no_body() {
        bytes.clear();
    } else if h.chunked {
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
    if h.gzip && !h.has_no_body() {
        bytes = gunzip(&bytes)?;
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
        location: Text::new(),
    })
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

fn gunzip(bytes: &[u8]) -> Option<Vec<u8>> {
    use miniz_oxide::inflate::{
        core::{
            decompress, inflate_flags::TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF, DecompressorOxide,
        },
        TINFLStatus,
    };
    let [0x1f, 0x8b, 8, flags, ..] = *bytes else {
        return None;
    };
    if flags & 0xe0 != 0 || bytes.len() < 18 {
        return None;
    }
    let (data, footer) = bytes.split_at(bytes.len() - 8);
    let mut start = 10;
    if flags & 4 != 0 {
        start += 2 + u16::from_le_bytes(data.get(10..12)?.try_into().ok()?) as usize;
    }
    for flag in [8, 16] {
        if flags & flag != 0 {
            start += data.get(start..)?.iter().position(|&b| b == 0)? + 1;
        }
    }
    if flags & 2 != 0 {
        let checksum = u16::from_le_bytes(data.get(start..start + 2)?.try_into().ok()?);
        if crc32(&data[..start]) as u16 != checksum {
            return None;
        }
        start += 2;
    }
    let checksum = u32::from_le_bytes(footer[..4].try_into().ok()?);
    let size = u32::from_le_bytes(footer[4..].try_into().ok()?) as usize;
    if size > MAX_RESPONSE {
        return None;
    }
    let compressed = data.get(start..)?;
    let mut output = alloc::vec![0; size];
    let mut decoder = alloc::boxed::Box::<DecompressorOxide>::default();
    let (status, consumed, written) = decompress(
        &mut decoder,
        compressed,
        &mut output,
        0,
        TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF,
    );
    (status == TINFLStatus::Done
        && consumed == compressed.len()
        && written == size
        && crc32(&output) == checksum)
        .then_some(output)
}

pub fn request(host: &str, path: &str, port: u16, secure: bool) -> Text<512> {
    use core::fmt::Write;
    let mut request = Text::new();
    let _ = write!(request, "GET {} HTTP/1.1\r\nHost: {}", path, host);
    if port != if secure { 443 } else { 80 } {
        let _ = write!(request, ":{}", port);
    }
    request.append(
        "\r\nUser-Agent: KreativeOS/0.2\r\nAccept-Encoding: gzip\r\nConnection: close\r\n\r\n",
    );
    request
}

/// Resolve an HTTP reference. Secure documents may only use secure destinations.
pub fn resolve(base: &str, src: &str) -> Option<Text<384>> {
    let src = src.split('#').next()?.trim();
    if src.is_empty() || src.len() > 384 || !src.bytes().all(|b| (33..127).contains(&b)) {
        return None;
    }
    let (scheme, base) = base.split_once("://")?;
    if !matches!(scheme, "http" | "https") {
        return None;
    }
    let mut out = Text::new();
    if src.starts_with("https://") || (scheme == "http" && src.starts_with("http://")) {
        out.set(src);
        return Some(out);
    }
    if src.starts_with("//") {
        if scheme.len() + 1 + src.len() > out.bytes.len() {
            return None;
        }
        out.append(scheme);
        out.push(b':');
        out.append(src);
        return Some(out);
    }
    let (path, query) = src
        .split_once('?')
        .map_or((src, None), |(p, q)| (p, Some(q)));
    if path.split('/').next()?.contains(':') {
        return None;
    }
    let base = base.split(['?', '#']).next()?;
    let authority = base.split('/').next()?;
    out.set(scheme);
    out.append("://");
    out.append(authority);
    let root_len = out.len;
    let base_path = base.get(authority.len()..)?;
    if path.is_empty() {
        if out.len + base_path.len() > out.bytes.len() {
            return None;
        }
        out.append(base_path);
    } else if !path.starts_with('/') {
        if let Some((folder, _)) = base_path.rsplit_once('/') {
            out.append(folder);
        }
    }
    let mut segments = path
        .strip_prefix('/')
        .unwrap_or(path)
        .split('/')
        .filter(|_| !path.is_empty())
        .peekable();
    while let Some(segment) = segments.next() {
        match segment {
            ".." => {
                if let Some(folder) = out.str()[root_len..].rfind('/') {
                    out.len = root_len + folder;
                }
            }
            "." => {}
            value => {
                if out.len + value.len() + 1 > out.bytes.len() {
                    return None;
                }
                out.push(b'/');
                out.append(value);
            }
        }
        if segments.peek().is_none() && matches!(segment, "." | "..") {
            if out.len == out.bytes.len() {
                return None;
            }
            out.push(b'/');
        }
    }
    if out.len == root_len {
        if out.len == out.bytes.len() {
            return None;
        }
        out.push(b'/');
    }
    if let Some(query) = query {
        if out.len + 1 + query.len() > out.bytes.len() {
            return None;
        }
        out.push(b'?');
        out.append(query);
    }
    Some(out)
}

fn attribute<'a>(mut text: &'a str, key: &str) -> Option<&'a str> {
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
        if name.eq_ignore_ascii_case(key) {
            return Some(value);
        }
        text = tail;
    }
    None
}

pub struct Link {
    pub label: Text<96>,
    pub url: Text<384>,
}

pub fn links(base: &str, html: &str) -> Vec<Link> {
    let mut links: Vec<Link> = Vec::new();
    let mut hidden = None;
    for part in html.split('<').skip(1) {
        let Some((tag, tail)) = part.split_once('>') else {
            continue;
        };
        let end = tag.find(char::is_whitespace).unwrap_or(tag.len());
        let name = &tag[..end];
        if let Some(closing) = hidden {
            if name.eq_ignore_ascii_case(closing) {
                hidden = None;
            }
            continue;
        }
        if name.eq_ignore_ascii_case("script") || name.eq_ignore_ascii_case("style") {
            hidden = Some(if name.eq_ignore_ascii_case("script") {
                "/script"
            } else {
                "/style"
            });
            continue;
        }
        if !name.eq_ignore_ascii_case("a") {
            continue;
        }
        let Some(href) = attribute(&tag[end..], "href").filter(|href| href.len() <= 384) else {
            continue;
        };
        // HTML uses &amp; for query separators.
        let Some(url) = resolve(base, &href.replace("&amp;", "&")) else {
            continue;
        };
        if links.iter().any(|link| link.url.str() == url.str()) {
            continue;
        }
        let mut label = Text::new();
        for byte in tail.trim().bytes().filter(u8::is_ascii) {
            label.push(if byte.is_ascii_control() { b' ' } else { byte });
        }
        links.push(Link { label, url });
        if links.len() == 48 {
            break;
        }
    }
    links
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
        let src = attribute(&tag[end..], "src").and_then(|src| resolve(url, src));
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
