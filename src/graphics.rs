use alloc::vec::Vec;
use tiny_skia::{
    FillRule, FilterQuality, LineCap, LineJoin, Paint, PathBuilder, Pixmap, PixmapPaint, Stroke,
    Transform,
};
static mut SYMBOLS: [[u8; 400]; 16] = [[0; 400]; 16];
static mut POINTER: Option<Pixmap> = None;
static mut IMAGE_CACHE: Vec<(usize, i32, i32, Pixmap)> = Vec::new();

static mut FONT: Vec<u8> = Vec::new();
static mut INK_BOUNDS: [(i32, i32, i32, i32); 380] = [(0, 0, 0, 0); 380];
static mut WALLPAPER: Vec<u32> = Vec::new();
pub fn init() {
    unsafe {
        FONT = miniz_oxide::inflate::decompress_to_vec_zlib(include_bytes!("../assets/ui-font.z"))
            .expect("Font atlas");
        for glyph in 0..380 {
            let (mut left, mut top, mut right, mut bottom) = (40, 48, 0, 0);
            for row in 0..48 {
                for col in 0..40 {
                    if FONT[glyph * 1921 + 1 + row * 40 + col] != 0 {
                        left = left.min(col as i32);
                        top = top.min(row as i32);
                        right = right.max(col as i32 + 1);
                        bottom = bottom.max(row as i32 + 1);
                    }
                }
            }
            if right > left {
                INK_BOUNDS[glyph] = (left, top, right, bottom);
            }
        }
        for id in 0..SYMBOLS.len() {
            let mut pixmap = Pixmap::new(20, 20).expect("icon pixmap");
            let mut builder = PathBuilder::new();
            for path in symbol_paths(id) {
                let Some((&first, rest)) = path.split_first() else {
                    continue;
                };
                builder.move_to(first.0 as f32 + 0.5, first.1 as f32 + 0.5);
                for &(x, y) in rest {
                    builder.line_to(x as f32 + 0.5, y as f32 + 0.5);
                }
            }
            let mut paint = Paint::default();
            paint.set_color_rgba8(255, 255, 255, 255);
            let stroke = Stroke {
                width: 1.65,
                line_cap: LineCap::Round,
                line_join: LineJoin::Round,
                ..Stroke::default()
            };
            if let Some(path) = builder.finish() {
                pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
            }
            for (to, from) in SYMBOLS[id].iter_mut().zip(pixmap.pixels()) {
                *to = from.alpha();
            }
        }
        let mut pointer = Pixmap::new(16, 20).expect("cursor pixmap");
        let scale = Transform::from_scale(0.67, 0.67);
        let mut shape = PathBuilder::new();
        shape.move_to(1.5, 1.5);
        shape.line_to(2.0, 22.0);
        shape.line_to(7.3, 16.7);
        shape.line_to(11.7, 25.0);
        shape.line_to(15.5, 23.1);
        shape.line_to(11.5, 15.0);
        shape.line_to(19.0, 14.8);
        shape.close();
        if let Some(path) = shape.finish() {
            let mut paint = Paint::default();
            paint.set_color_rgba8(5, 15, 18, 108);
            pointer.fill_path(
                &path,
                &paint,
                FillRule::Winding,
                Transform::from_row(0.67, 0.0, 0.0, 0.67, 0.7, 0.8),
                None,
            );
            paint.set_color_rgba8(250, 253, 249, 255);
            pointer.fill_path(&path, &paint, FillRule::Winding, scale, None);
            paint.set_color_rgba8(18, 36, 40, 255);
            pointer.stroke_path(
                &path,
                &paint,
                &Stroke {
                    width: 1.35,
                    line_join: LineJoin::Round,
                    ..Stroke::default()
                },
                scale,
                None,
            );
        }
        POINTER = Some(pointer);
    }
}
#[derive(Clone, Copy)]
pub struct Clip(i32, i32, i32, i32);
pub struct Screen {
    pub front: *mut u32,
    pub back: *mut u32,
    pub w: usize,
    pub h: usize,
    pub stride: usize,
    pub rgb: bool,
    pub clip: Option<Clip>,
    pub theme: u8,
}
pub fn mix(a: u32, b: u32, t: u32) -> u32 {
    let u = 255 - t;
    ((((a >> 16) & 255) * u + ((b >> 16) & 255) * t) / 255 << 16)
        | ((((a >> 8) & 255) * u + ((b >> 8) & 255) * t) / 255 << 8)
        | (((a & 255) * u + (b & 255) * t) / 255)
}
fn font_index(scale: i32) -> usize {
    match scale {
        0 | 1 => 0,
        2 => 1,
        3 => 2,
        _ => 3,
    }
}
fn advance(b: u8, scale: i32) -> i32 {
    let c = if (32..127).contains(&b) { b } else { b'?' };
    unsafe { FONT[(font_index(scale) * 95 + (c - 32) as usize) * 1921] as i32 }
}
pub fn text_width(s: &str, scale: i32) -> i32 {
    s.bytes().map(|b| advance(b, scale)).sum()
}
impl Screen {
    pub fn magnify(&mut self, mx: i32, my: i32) {
        const WIDTH: usize = 120;
        const HEIGHT: usize = 72;
        if self.w < WIDTH * 2 + 20 || self.h < HEIGHT * 2 + 80 {
            return;
        }
        let left = (mx - WIDTH as i32 / 2).clamp(0, (self.w - WIDTH) as i32) as usize;
        let top = (my - HEIGHT as i32 / 2).clamp(0, (self.h - HEIGHT) as i32) as usize;
        let x = if mx < self.w as i32 / 2 {
            self.w - WIDTH * 2 - 12
        } else {
            12
        };
        let y = if my < self.h as i32 / 2 {
            self.h - HEIGHT * 2 - 64
        } else {
            12
        };
        let mut sample = [0u32; WIDTH * HEIGHT];
        unsafe {
            for row in 0..HEIGHT {
                core::ptr::copy_nonoverlapping(
                    self.back.add((top + row) * self.w + left),
                    sample.as_mut_ptr().add(row * WIDTH),
                    WIDTH,
                );
            }
            for row in 0..HEIGHT * 2 {
                let target = self.back.add((y + row) * self.w + x);
                for column in 0..WIDTH * 2 {
                    target
                        .add(column)
                        .write(sample[(row / 2) * WIDTH + column / 2]);
                }
            }
        }
        let (x, y) = (x as i32, y as i32);
        let (w, h) = ((WIDTH * 2) as i32, (HEIGHT * 2) as i32);
        for (left, top, width, height) in [
            (x - 2, y - 2, w + 4, 2),
            (x - 2, y + h, w + 4, 2),
            (x - 2, y, 2, h),
            (x + w, y, 2, h),
        ] {
            self.rect(left, top, width, height, 0x75c092);
        }
    }
    fn theme_color(&self, color: u32) -> u32 {
        if self.theme == 0 {
            return color;
        }
        let red = color >> 16 & 255;
        let green = color >> 8 & 255;
        let blue = color & 255;
        let lightness = (red * 3 + green * 6 + blue) / 10;
        if self.theme == 2 {
            return match lightness {
                0..=75 => 0xffffff,
                76..=140 => 0xffde61,
                _ => 0x000000,
            };
        }
        let target = 245u32.saturating_sub(lightness * 9 / 10);
        let channel = |value: u32| {
            (target as i32 + (value as i32 - lightness as i32) / 2).clamp(0, 255) as u32
        };
        channel(red) << 16 | channel(green) << 8 | channel(blue)
    }
    pub fn clip_rect(&mut self, x: i32, y: i32, w: i32, h: i32) -> Option<Clip> {
        let old = self.clip;
        let Clip(l, t, r, b) = old.unwrap_or(Clip(0, 0, self.w as i32, self.h as i32));
        self.clip = Some(Clip(l.max(x), t.max(y), r.min(x + w), b.min(y + h)));
        old
    }
    fn inside(&self, x: i32, y: i32) -> bool {
        let Clip(l, t, r, b) = self
            .clip
            .unwrap_or(Clip(0, 0, self.w as i32, self.h as i32));
        x >= l && x < r && y >= t && y < b
    }
    pub fn pixel(&mut self, x: i32, y: i32, c: u32) {
        if self.inside(x, y) {
            unsafe { *self.back.add(y as usize * self.w + x as usize) = c }
        }
    }
    pub fn blend(&mut self, x: i32, y: i32, c: u32, a: u32) {
        let c = self.theme_color(c);
        self.blend_image(x, y, c, a);
    }
    fn blend_image(&mut self, x: i32, y: i32, c: u32, a: u32) {
        if a == 0 {
            return;
        }
        if self.inside(x, y) {
            unsafe {
                let p = self.back.add(y as usize * self.w + x as usize);
                *p = mix(*p, c, a.min(255));
            }
        }
    }
    pub fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: u32) {
        let c = self.theme_color(c);
        let Clip(l, t, r, b) = self
            .clip
            .unwrap_or(Clip(0, 0, self.w as i32, self.h as i32));
        let left = x.max(l);
        let right = (x + w).min(r);
        if right <= left {
            return;
        }
        for yy in y.max(t)..(y + h).min(b) {
            unsafe {
                core::slice::from_raw_parts_mut(
                    self.back.add(yy as usize * self.w + left as usize),
                    (right - left) as usize,
                )
                .fill(c);
            }
        }
    }
    pub fn line(&mut self, mut x: i32, mut y: i32, x1: i32, y1: i32, c: u32) {
        let dx = (x1 - x).abs();
        let sx = if x < x1 { 1 } else { -1 };
        let dy = -(y1 - y).abs();
        let sy = if y < y1 { 1 } else { -1 };
        let mut e = dx + dy;
        loop {
            self.pixel(x, y, c);
            if x == x1 && y == y1 {
                break;
            }
            let e2 = 2 * e;
            if e2 >= dy {
                e += dy;
                x += sx
            }
            if e2 <= dx {
                e += dx;
                y += sy
            }
        }
    }
    pub fn rounded(&mut self, x: i32, y: i32, w: i32, h: i32, r: i32, c: u32, alpha: u32) {
        if w <= 0 || h <= 0 {
            return;
        }
        let r = r.min(w / 2).min(h / 2).max(1);
        if alpha == 255 && w > 2 * r && h > 2 * r {
            self.rect(x + r, y, w - 2 * r, h, c);
            self.rect(x, y + r, r, h - 2 * r, c);
            self.rect(x + w - r, y + r, r, h - 2 * r, c);
            for yy in 0..r {
                for xx in 0..r {
                    let dx = r - xx;
                    let dy = r - yy;
                    let a = ((r * r - dx * dx - dy * dy) * 255 / (2 * r)).clamp(0, 255) as u32;
                    self.blend(x + xx, y + yy, c, a);
                    self.blend(x + w - 1 - xx, y + yy, c, a);
                    self.blend(x + xx, y + h - 1 - yy, c, a);
                    self.blend(x + w - 1 - xx, y + h - 1 - yy, c, a);
                }
            }
            return;
        }
        for yy in 0..h {
            for xx in 0..w {
                let dx = if xx < r {
                    r - xx
                } else if xx >= w - r {
                    xx - (w - r - 1)
                } else {
                    0
                };
                let dy = if yy < r {
                    r - yy
                } else if yy >= h - r {
                    yy - (h - r - 1)
                } else {
                    0
                };
                let d = dx * dx + dy * dy;
                let cov = if dx == 0 || dy == 0 {
                    255
                } else {
                    ((r * r - d) * 255 / (2 * r)).clamp(0, 255)
                };
                if cov > 0 {
                    self.blend(x + xx, y + yy, c, alpha * cov as u32 / 255)
                }
            }
        }
    }
    pub fn round(&mut self, x: i32, y: i32, w: i32, h: i32, c: u32) {
        self.rounded(x, y, w, h, 10, c, 255)
    }
    pub fn shadow(&mut self, x: i32, y: i32, w: i32, h: i32) {
        for spread in 1..=10 {
            let alpha = (12 - spread) as u32;
            let left = x - spread;
            let right = x + w + spread;
            let top = y + 3 - spread;
            let bottom = y + h + 5 + spread;
            for px in left..right {
                self.blend(px, top, 0x071517, alpha);
                self.blend(px, bottom, 0x071517, alpha);
            }
            for py in top + 1..bottom {
                self.blend(left, py, 0x071517, alpha);
                self.blend(right - 1, py, 0x071517, alpha);
            }
        }
    }
    pub fn text(&mut self, x: i32, y: i32, s: &str, c: u32, scale: i32) {
        let mut xx = x;
        let mut yy = y;
        for b in s.bytes() {
            if b == b'\n' {
                xx = x;
                yy += if scale <= 2 { 23 } else { 34 };
                continue;
            }
            let ch = if (32..127).contains(&b) { b } else { b'?' };
            let off = (font_index(scale) * 95 + (ch - 32) as usize) * 1921;
            unsafe {
                for row in 0..48 {
                    for col in 0..40 {
                        let a = FONT[off + 1 + row * 40 + col];
                        if a != 0 {
                            self.blend(xx + col as i32, yy + row as i32, c, a as u32)
                        }
                    }
                }
            }
            xx += advance(ch, scale)
        }
    }
    pub fn wrapped(&mut self, x: i32, y: i32, w: i32, h: i32, s: &str, c: u32) {
        let mut xx = x;
        let mut yy = y;
        for word in s.split_inclusive(|c: char| c == ' ' || c == '\n') {
            let size = text_width(word.trim_end_matches('\n'), 2);
            if xx != x && xx + size > x + w {
                xx = x;
                yy += 23
            }
            for b in word.bytes() {
                if b == b'\n' {
                    xx = x;
                    yy += 23;
                    continue;
                }
                let adv = advance(b, 2);
                if xx + adv > x + w {
                    xx = x;
                    yy += 23
                }
                if yy + 21 > y + h {
                    return;
                }
                let a = [b];
                self.text(xx, yy, core::str::from_utf8(&a).unwrap_or("?"), c, 2);
                xx += adv;
            }
        }
    }
    pub fn centered(&mut self, x: i32, y: i32, w: i32, h: i32, label: &str, color: u32, size: i32) {
        let mut cursor = 0;
        let (mut left, mut top, mut right, mut bottom) = (i32::MAX, i32::MAX, 0, 0);
        for byte in label.bytes() {
            let ch = if (32..127).contains(&byte) {
                byte
            } else {
                b'?'
            };
            let (l, t, r, b) = unsafe { INK_BOUNDS[font_index(size) * 95 + (ch - 32) as usize] };
            if r > l {
                left = left.min(cursor + l);
                top = top.min(t);
                right = right.max(cursor + r);
                bottom = bottom.max(b);
            }
            cursor += advance(ch, size);
        }
        if right > left && bottom > top {
            self.text(
                x + (w - (right - left)) / 2 - left,
                y + (h - (bottom - top)) / 2 - top,
                label,
                color,
                size,
            );
        }
    }

    pub fn wallpaper(&mut self) {
        unsafe {
            if WALLPAPER.len() != self.w * self.h {
                WALLPAPER = alloc::vec![0;self.w*self.h];
                for y in 0..self.h {
                    for x in 0..self.w {
                        let fx = x as f32 / self.w as f32;
                        let fy = y as f32 / self.h as f32;
                        let dx = fx - 0.85;
                        let dy = fy - 0.70;
                        let light = (1.0 - (dx * dx * 1.8 + dy * dy * 3.4)).max(0.0);
                        let mut c = mix(0x101521, 0x19585d, (255. * light * light) as u32);
                        let wave = fy - (0.48 + fx * 0.40 - fx * fx * 0.74);
                        let haze = (1.0 - wave.abs() * 6.5).max(0.0);
                        c = mix(c, 0x53609d, (104.0 * haze * haze) as u32);
                        if x % 28 == 0 && y % 28 == 0 {
                            c = mix(c, 0x92b6c6, 29);
                        }
                        WALLPAPER[y * self.w + x] = c;
                    }
                }
            }
            core::ptr::copy_nonoverlapping(WALLPAPER.as_ptr(), self.back, WALLPAPER.len());
        }
    }

    pub fn stroke(&mut self, x: i32, y: i32, a: (i32, i32), b: (i32, i32), c: u32) {
        let (ax, ay) = (x + a.0, y + a.1);
        let (bx, by) = (x + b.0, y + b.1);
        self.line(ax, ay, bx, by, c);
        if (bx - ax).abs() >= (by - ay).abs() {
            self.line(ax, ay + 1, bx, by + 1, c);
        } else {
            self.line(ax + 1, ay, bx + 1, by, c);
        }
    }

    /// Each mark uses the same 20x20 grid and two-pixel stroke.
    pub fn symbol(&mut self, x: i32, y: i32, id: usize, c: u32) {
        for (i, a) in unsafe { SYMBOLS[id.min(15)] }.iter().enumerate() {
            self.blend(x + (i % 20) as i32, y + (i / 20) as i32, c, *a as u32);
        }
    }

    pub fn image(
        &mut self,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        picture: &crate::pictures::Picture,
        zoom: bool,
    ) {
        if w <= 0 || h <= 0 {
            return;
        }
        let (pw, ph) = (picture.width as i32, picture.height as i32);
        let (width, height) = if zoom {
            (pw, ph)
        } else if pw * h > ph * w {
            (w, (w * ph / pw).max(1))
        } else {
            ((h * pw / ph).max(1), h)
        };
        let (left, top) = (x + (w - width) / 2, y + (h - height) / 2);
        let old = self.clip_rect(x, y, w, h);
        if (width, height) == (pw, ph) {
            self.blit_pixmap(left, top, &picture.pixmap);
        } else {
            let cache = unsafe { &mut IMAGE_CACHE };
            let id = picture.id;
            let entry = if let Some(idx) = cache
                .iter()
                .position(|item| (item.0, item.1, item.2) == (id, width, height))
            {
                idx
            } else {
                if cache.len() == 3 {
                    cache.remove(0);
                }
                if let Some(mut scaled) = Pixmap::new(width as u32, height as u32) {
                    let paint = PixmapPaint {
                        quality: FilterQuality::Bicubic,
                        ..PixmapPaint::default()
                    };
                    scaled.draw_pixmap(
                        0,
                        0,
                        picture.pixmap.as_ref(),
                        &paint,
                        Transform::from_scale(width as f32 / pw as f32, height as f32 / ph as f32),
                        None,
                    );
                    cache.push((id, width, height, scaled));
                }
                cache.len().saturating_sub(1)
            };
            if let Some((_, _, _, scaled)) = cache
                .get(entry)
                .filter(|v| (v.0, v.1, v.2) == (id, width, height))
            {
                self.blit_pixmap(left, top, scaled);
            }
        }
        self.clip = old;
    }

    fn blit_pixmap(&mut self, x: i32, y: i32, pixmap: &Pixmap) {
        let width = pixmap.width() as usize;
        for (i, pixel) in pixmap.pixels().iter().enumerate() {
            let alpha = pixel.alpha() as u32;
            if alpha == 0 {
                continue;
            }
            let (xx, yy) = (x + (i % width) as i32, y + (i / width) as i32);
            let channel = |v: u8| v as u32 * 255 / alpha;
            let color = (channel(pixel.red()) << 16)
                | (channel(pixel.green()) << 8)
                | channel(pixel.blue());
            if alpha == 255 {
                self.pixel(xx, yy, color);
            } else {
                self.blend_image(xx, yy, color, alpha);
            }
        }
    }
    fn front_pixel(&self, x: i32, y: i32, c: u32) {
        let c = if self.rgb {
            (c & 0xff00) | ((c & 255) << 16) | ((c >> 16) & 255)
        } else {
            c
        };
        unsafe {
            core::ptr::write_volatile(self.front.add(y as usize * self.stride + x as usize), c);
        }
    }
    /// Restore the previous pointer from the unmodified scene, then composite its new position.
    pub fn pointer(&self, previous: Option<(i32, i32)>, at: (i32, i32)) {
        let Some(cursor) = (unsafe { POINTER.as_ref() }) else {
            return;
        };
        let width = cursor.width() as i32;
        let height = cursor.height() as i32;
        let valid = |x: i32, y: i32| x >= 0 && y >= 0 && x < self.w as i32 && y < self.h as i32;
        let scene = |x: i32, y: i32| unsafe { *self.back.add(y as usize * self.w + x as usize) };
        if let Some((x, y)) = previous {
            for dy in 0..height {
                for dx in 0..width {
                    let (px, py) = (x + dx - 1, y + dy - 1);
                    if valid(px, py) {
                        self.front_pixel(px, py, scene(px, py));
                    }
                }
            }
        }
        for (index, p) in cursor.pixels().iter().enumerate() {
            let (x, y) = (
                at.0 + index as i32 % width - 1,
                at.1 + index as i32 / width - 1,
            );
            let a = p.alpha() as u32;
            if !valid(x, y) || a == 0 {
                continue;
            }
            let src = ((p.red() as u32 * 255 / a) << 16)
                | ((p.green() as u32 * 255 / a) << 8)
                | (p.blue() as u32 * 255 / a);
            self.front_pixel(x, y, mix(scene(x, y), src, a));
        }
    }
    pub fn present(&mut self) {
        unsafe {
            for y in 0..self.h {
                for x in 0..self.w {
                    let c = *self.back.add(y * self.w + x);
                    let c = if self.rgb {
                        (c & 0xff00) | ((c & 255) << 16) | ((c >> 16) & 255)
                    } else {
                        c
                    };
                    core::ptr::write_volatile(self.front.add(y * self.stride + x), c)
                }
            }
        }
    }
}

fn symbol_paths(id: usize) -> &'static [&'static [(i32, i32)]] {
    match id {
        14 => &[
            &[(5, 5), (15, 5), (15, 15), (5, 15), (5, 5)],
            &[(8, 8), (12, 8), (12, 12), (8, 12), (8, 8)],
            &[(2, 8), (5, 8)],
            &[(2, 12), (5, 12)],
            &[(15, 8), (18, 8)],
            &[(15, 12), (18, 12)],
            &[(8, 2), (8, 5)],
            &[(12, 2), (12, 5)],
            &[(8, 15), (8, 18)],
            &[(12, 15), (12, 18)],
        ],
        13 => &[
            &[(3, 5), (17, 5), (17, 17), (3, 17), (3, 5)],
            &[(7, 2), (7, 7)],
            &[(13, 2), (13, 7)],
            &[(3, 9), (17, 9)],
            &[(7, 13), (9, 13)],
            &[(12, 13), (14, 13)],
        ],
        0 => &[&[(2, 15), (2, 5), (8, 5), (10, 7), (17, 7), (17, 15), (2, 15)]],
        1 => &[
            &[(4, 2), (15, 2), (15, 17), (4, 17), (4, 2)],
            &[(7, 6), (12, 6)],
            &[(7, 10), (12, 10)],
            &[(7, 14), (10, 14)],
        ],
        2 => &[&[(3, 5), (7, 9), (3, 13)], &[(10, 14), (16, 14)]],
        3 => &[
            &[
                (10, 2),
                (14, 3),
                (17, 6),
                (18, 10),
                (17, 14),
                (14, 17),
                (10, 18),
                (6, 17),
                (3, 14),
                (2, 10),
                (3, 6),
                (6, 3),
                (10, 2),
            ],
            &[
                (10, 2),
                (7, 6),
                (6, 10),
                (7, 14),
                (10, 18),
                (13, 14),
                (14, 10),
                (13, 6),
                (10, 2),
            ],
            &[(3, 10), (17, 10)],
        ],
        4 => &[
            &[(3, 2), (16, 2), (16, 17), (3, 17), (3, 2)],
            &[(6, 6), (13, 6)],
            &[(6, 10), (7, 10)],
            &[(11, 10), (12, 10)],
            &[(6, 14), (7, 14)],
            &[(11, 14), (12, 14)],
        ],
        5 => &[
            &[(4, 13), (13, 3), (16, 6), (7, 16), (3, 17), (4, 13)],
            &[(4, 13), (7, 16)],
            &[(11, 5), (14, 8)],
        ],
        6 => &[
            &[(3, 15), (3, 10), (6, 8), (13, 8), (15, 6), (13, 3), (9, 3)],
            &[(8, 4), (6, 3), (8, 2)],
        ],
        7 => &[
            &[
                (3, 6),
                (10, 3),
                (16, 6),
                (16, 14),
                (10, 17),
                (3, 14),
                (3, 6),
            ],
            &[(3, 6), (10, 9), (16, 6)],
            &[(10, 9), (10, 17)],
        ],
        8 => &[
            &[(3, 16), (3, 12)],
            &[(9, 16), (9, 8)],
            &[(15, 16), (15, 3)],
        ],
        9 => &[
            &[(3, 5), (16, 5)],
            &[(3, 10), (16, 10)],
            &[(3, 15), (16, 15)],
            &[(7, 3), (7, 7)],
            &[(12, 8), (12, 12)],
            &[(6, 13), (6, 17)],
        ],
        10 => &[
            &[(7, 3), (7, 16)],
            &[(12, 3), (12, 16)],
            &[(3, 7), (16, 7)],
            &[(3, 12), (16, 12)],
        ],
        11 => &[
            &[
                (2, 4),
                (8, 4),
                (10, 6),
                (12, 4),
                (17, 4),
                (17, 15),
                (12, 15),
                (10, 17),
                (8, 15),
                (2, 15),
                (2, 4),
            ],
            &[(10, 6), (10, 16)],
        ],
        12 => &[
            &[(2, 3), (17, 3), (17, 16), (2, 16), (2, 3)],
            &[(3, 13), (7, 9), (11, 13), (14, 10), (16, 12)],
            &[(12, 7), (13, 7)],
        ],
        _ => &[&[(4, 3), (4, 16)], &[(14, 3), (5, 10), (15, 16)]],
    }
}
