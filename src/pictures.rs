use alloc::vec::Vec;
use zune_core::{bytestream::ZCursor, colorspace::ColorSpace, options::DecoderOptions};

pub const IMPORT_LIMIT: usize = 6 * 1024 * 1024;
pub static mut BOOT_IMAGES: [u8; IMPORT_LIMIT] = [0; IMPORT_LIMIT];
pub static mut VOLUMES: [(usize, usize); 12] = [(0, 0); 12];

pub struct Picture {
    pub width: usize,
    pub height: usize,
    pub pixmap: tiny_skia::Pixmap,
    pub id: usize,
}

pub struct Catalog {
    pub files: Vec<(&'static str, &'static [u8])>,
    pub selected: usize,
    pub picture: Option<Picture>,
    pub zoom: bool,
}

impl Catalog {
    pub fn new() -> Self {
        let mut files = Vec::new();
        for (offset, len) in unsafe { VOLUMES } {
            if len < 4 {
                continue;
            }
            let mut rest = unsafe { &BOOT_IMAGES[offset..offset + len] };
            if !rest.starts_with(b"KIP1") {
                continue;
            }
            rest = &rest[4..];
            while rest.len() >= 5 && files.len() < 36 {
                let name_len = rest[0] as usize;
                let image_len = u32::from_le_bytes(rest[1..5].try_into().unwrap()) as usize;
                let name_end = 5 + name_len;
                let end = name_end + image_len;
                if name_len == 0 || end > rest.len() {
                    break;
                }
                if let Ok(name) = core::str::from_utf8(&rest[5..name_end]) {
                    if name.bytes().all(|c| (32..127).contains(&c)) {
                        files.push((name, &rest[name_end..end]));
                    }
                }
                rest = &rest[end..];
            }
        }
        files.push(("Estuary.png", include_bytes!("../assets/estuary.png")));
        files.push(("Copper.jpg", include_bytes!("../assets/copper.jpg")));
        let mut catalog = Self {
            files,
            selected: 0,
            picture: None,
            zoom: false,
        };
        catalog.select(0);
        catalog
    }

    pub fn select(&mut self, index: usize) {
        self.selected = index.min(self.files.len() - 1);
        self.picture = None;
        self.picture = decode(self.files[self.selected].1);
        self.zoom = false;
    }
}

fn fits(w: usize, h: usize) -> bool {
    w > 0 && h > 0 && w <= 4096 && h <= 4096 && w * h <= 2_621_440
}

fn rgb(r: u8, g: u8, b: u8) -> u32 {
    ((r as u32) << 16) | ((g as u32) << 8) | b as u32
}

pub fn decode(data: &[u8]) -> Option<Picture> {
    if data.starts_with(b"BM") {
        return bmp(data);
    }
    let options = DecoderOptions::default()
        .set_max_width(4096)
        .set_max_height(4096);
    let (width, height, color, bytes) = if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        let options = options.png_set_strip_to_8bit(true);
        let mut png = zune_png::PngDecoder::new_with_options(ZCursor::new(data), options);
        png.decode_headers().ok()?;
        let (w, h) = png.dimensions()?;
        if !fits(w, h) {
            return None;
        }
        let bytes = png.decode_raw().ok()?;
        (w, h, png.colorspace()?, bytes)
    } else if data.starts_with(&[0xff, 0xd8]) {
        let options = options.jpeg_set_out_colorspace(ColorSpace::RGB);
        let mut jpg = zune_jpeg::JpegDecoder::new_with_options(ZCursor::new(data), options);
        jpg.decode_headers().ok()?;
        let info = jpg.info()?;
        let (w, h) = (info.width as usize, info.height as usize);
        if !fits(w, h) {
            return None;
        }
        let bytes = jpg.decode().ok()?;
        (w, h, jpg.output_colorspace()?, bytes)
    } else {
        return None;
    };
    let channels = match color {
        ColorSpace::RGB => 3,
        ColorSpace::RGBA => 4,
        ColorSpace::Luma => 1,
        ColorSpace::LumaA => 2,
        _ => return None,
    };
    if bytes.len() != width * height * channels {
        return None;
    }
    let pixels = bytes
        .chunks_exact(channels)
        .enumerate()
        .map(|(i, p)| {
            let (pixel, alpha) = match channels {
                1 => (rgb(p[0], p[0], p[0]), 255),
                2 => (rgb(p[0], p[0], p[0]), p[1]),
                3 => (rgb(p[0], p[1], p[2]), 255),
                _ => (rgb(p[0], p[1], p[2]), p[3]),
            };
            let checker = if ((i % width) / 12 + (i / width) / 12) % 2 == 0 {
                0xe0ddd5
            } else {
                0xbfbab0
            };
            crate::graphics::mix(checker, pixel, alpha as u32)
        })
        .collect();
    finish(width, height, pixels)
}

fn bmp(data: &[u8]) -> Option<Picture> {
    let u32_at = |n| Some(u32::from_le_bytes(data.get(n..n + 4)?.try_into().ok()?));
    let u16_at = |n| Some(u16::from_le_bytes(data.get(n..n + 2)?.try_into().ok()?));
    let offset = u32_at(10)? as usize;
    let header = u32_at(14)? as usize;
    let signed_width = u32_at(18)? as i32;
    let signed_height = u32_at(22)? as i32;
    let width = usize::try_from(signed_width).ok()?;
    let height = signed_height.unsigned_abs() as usize;
    if header < 40
        || offset < 14usize.checked_add(header)?
        || !fits(width, height)
        || u16_at(26)? != 1
        || u32_at(30)? != 0
    {
        return None;
    }
    let channels = match u16_at(28)? {
        24 => 3,
        32 => 4,
        _ => return None,
    };
    let stride = (width * channels + 3) & !3;
    let pixels_in = data.get(offset..offset.checked_add(stride * height)?)?;
    let mut pixels = Vec::with_capacity(width * height);
    for y in 0..height {
        let row = stride * if signed_height > 0 { height - 1 - y } else { y };
        for p in pixels_in[row..row + width * channels].chunks_exact(channels) {
            pixels.push(rgb(p[2], p[1], p[0]));
        }
    }
    finish(width, height, pixels)
}

fn finish(width: usize, height: usize, pixels: Vec<u32>) -> Option<Picture> {
    use core::sync::atomic::{AtomicUsize, Ordering};
    static ID: AtomicUsize = AtomicUsize::new(1);
    let mut pixmap = tiny_skia::Pixmap::new(width as u32, height as u32)?;
    for (to, c) in pixmap.data_mut().chunks_exact_mut(4).zip(pixels) {
        to.copy_from_slice(&[(c >> 16) as u8, (c >> 8) as u8, c as u8, 255]);
    }
    Some(Picture {
        width,
        height,
        pixmap,
        id: ID.fetch_add(1, Ordering::Relaxed),
    })
}
