use alloc::vec::Vec;
static mut FONT: Vec<u8> = Vec::new();
static mut WALLPAPER: Vec<u32> = Vec::new();
pub fn init() {
    unsafe {
        FONT = miniz_oxide::inflate::decompress_to_vec_zlib(include_bytes!("../assets/ui-font.z"))
            .expect("Font atlas");
    }
}
pub struct Screen {
    pub front: *mut u32,
    pub back: *mut u32,
    pub w: usize,
    pub h: usize,
    pub stride: usize,
    pub rgb: bool,
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
    pub fn pixel(&mut self, x: i32, y: i32, c: u32) {
        if x >= 0 && y >= 0 && (x as usize) < self.w && (y as usize) < self.h {
            unsafe { *self.back.add(y as usize * self.w + x as usize) = c }
        }
    }
    pub fn blend(&mut self, x: i32, y: i32, c: u32, a: u32) {
        if a == 0 {
            return;
        }
        if x >= 0 && y >= 0 && (x as usize) < self.w && (y as usize) < self.h {
            unsafe {
                let p = self.back.add(y as usize * self.w + x as usize);
                *p = mix(*p, c, a.min(255));
            }
        }
    }
    pub fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: u32) {
        for yy in y.max(0)..(y + h).min(self.h as i32) {
            for xx in x.max(0)..(x + w).min(self.w as i32) {
                self.pixel(xx, yy, c)
            }
        }
    }
    pub fn tint(&mut self, x: i32, y: i32, w: i32, h: i32, c: u32, a: u32) {
        for yy in y.max(0)..(y + h).min(self.h as i32) {
            for xx in x.max(0)..(x + w).min(self.w as i32) {
                self.blend(xx, yy, c, a)
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
    pub fn circle(&mut self, x: i32, y: i32, r: i32, c: u32) {
        for yy in -r..=r {
            for xx in -r..=r {
                let d = xx * xx + yy * yy;
                let edge = r * r - d;
                if edge >= 0 {
                    self.blend(
                        x + xx,
                        y + yy,
                        c,
                        ((edge * 255) / (2 * r).max(1)).min(255) as u32,
                    )
                }
            }
        }
    }
    pub fn rounded(&mut self, x: i32, y: i32, w: i32, h: i32, r: i32, c: u32, alpha: u32) {
        let r = r.min(w / 2).min(h / 2).max(1);
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
        for spread in (1..=6).rev() {
            let d = spread * 3;
            self.rounded(x - d, y - d + 9, w + d * 2, h + d * 2, 18 + d, 0x211b3a, 5)
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
    pub fn wallpaper(&mut self) {
        unsafe {
            if WALLPAPER.len() != self.w * self.h {
                WALLPAPER = alloc::vec![0;self.w*self.h];
                for y in 0..self.h {
                    for x in 0..self.w {
                        let fx = x as f32 / self.w as f32;
                        let fy = y as f32 / self.h as f32;
                        let mut c = mix(0x304c9e, 0xb9b7ed, (fy * 255.) as u32);
                        let glow = libm::expf(
                            -((fx - 0.88) * (fx - 0.88) * 3.5 + (fy - 0.08) * (fy - 0.08) * 3.0),
                        );
                        c = mix(c, 0xffd7b5, (glow * 240.) as u32);
                        let wave = 0.55 + 0.22 * libm::sinf(fx * 4.5 - 0.8);
                        let dist = fy - wave;
                        let ridge = libm::expf(-dist * dist * 110.);
                        c = mix(c, 0xe5c8ed, (ridge * 150.) as u32);
                        let fold = libm::expf(-(dist + 0.075) * (dist + 0.075) * 90.);
                        c = mix(c, 0x5b66ac, (fold * 110.) as u32);
                        let lower = (fy - 0.55 + fx * 0.3).clamp(0., 1.);
                        c = mix(c, 0x283c79, (lower * 170.) as u32);
                        WALLPAPER[y * self.w + x] = c;
                    }
                }
            }
            core::ptr::copy_nonoverlapping(WALLPAPER.as_ptr(), self.back, WALLPAPER.len());
        }
    }
    pub fn icon(&mut self, x: i32, y: i32, id: usize, size: i32) {
        let colors = [
            0x3c9ce9, 0xe9ba42, 0x343b50, 0x529ce9, 0xec945d, 0xd28cdb, 0x59b98b, 0xba91df,
            0x69b7c3, 0x8d99b0, 0xe792a4, 0x8279d8,
        ];
        let c = colors[id % 12];
        self.rounded(x, y, size, size, 12, c, 255);
        self.rounded(x + 1, y + 1, size - 2, size / 2, 11, 0xffffff, 28);
        let mid = x + size / 2;
        let cy = y + size / 2;
        let white = 0xfaffff;
        match id {
            0 => {
                self.rounded(x + 8, y + 14, size - 16, size - 23, 4, white, 235);
                self.rounded(x + 8, y + 10, size / 2 - 3, 10, 3, white, 235);
            }
            1 => {
                self.rounded(x + 10, y + 8, size - 20, size - 16, 3, white, 255);
                for k in 0..3 {
                    self.rect(x + 15, y + 17 + k * 6, size - 30, 2, 0xcfb471)
                }
            }
            2 => self.text(x + 9, y + 10, ">_", white, 3),
            3 => {
                self.circle(mid, cy, size / 2 - 7, white);
                self.circle(mid, cy, size / 2 - 10, 0x69b9f3);
                self.line(mid - 7, cy + 9, mid + 7, cy - 9, 0xe95b69);
                self.line(mid - 6, cy + 9, mid + 8, cy - 9, 0xe95b69);
            }
            4 => {
                self.rect(x + 10, y + 9, size - 20, 8, white);
                for yy in 0..2 {
                    for xx in 0..3 {
                        self.rounded(x + 10 + xx * 9, y + 23 + yy * 9, 6, 6, 2, white, 255)
                    }
                }
            }
            5 => {
                self.circle(mid, cy, 15, white);
                for (dx, dy, color) in [
                    (-5, -6, 0x76a9e4),
                    (6, -5, 0xea8585),
                    (-7, 5, 0x80c39c),
                    (6, 6, 0xeac464),
                ] {
                    self.circle(mid + dx, cy + dy, 4, color)
                }
            }
            6 => {
                for i in 0..3 {
                    self.rounded(x + 10 + i * 9, y + 19, 11, 12, 4, white, 255)
                }
                self.circle(x + 33, y + 21, 2, 0x45946f)
            }
            7 => {
                self.rounded(x + 10, y + 12, size - 20, size - 22, 3, white, 230);
                self.rect(mid - 2, y + 12, 4, size - 22, 0xb88fd9);
                self.line(x + 10, y + 12, mid, y + 7, white);
                self.line(mid, y + 7, x + size - 10, y + 12, white)
            }
            8 => {
                for i in 0..3 {
                    self.rounded(
                        x + 9 + i * 10,
                        y + size - 14 - i * 8,
                        7,
                        6 + i * 8,
                        2,
                        white,
                        255,
                    )
                }
            }
            9 => {
                self.circle(mid, cy, 15, white);
                self.circle(mid, cy, 8, 0x8d99b0);
                self.circle(mid, cy, 3, white)
            }
            10 => self.text(x + 8, y + 8, "xo", white, 3),
            _ => self.text(x + 14, y + 8, "K", white, 3),
        }
    }
    pub fn cursor(&mut self, x: i32, y: i32) {
        for yy in 0..24 {
            for xx in 0..17 {
                let edge = yy as f32 * 0.62 - xx as f32;
                let a = (edge * 255.).clamp(0., 255.) as u32;
                if xx <= yy * 2 / 3 && yy < 20 {
                    self.blend(x + xx + 1, y + yy + 2, 0x000000, a / 3);
                    self.blend(
                        x + xx,
                        y + yy,
                        if xx == 0 || edge < 1.4 || yy == 19 {
                            0xffffff
                        } else {
                            0x2c3041
                        },
                        a,
                    )
                }
            }
        }
        self.line(x + 6, y + 16, x + 11, y + 26, 0xffffff);
        self.line(x + 7, y + 16, x + 12, y + 26, 0x2c3041);
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
