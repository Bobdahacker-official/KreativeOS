use alloc::vec::Vec;
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
                        let glow =
                            (1.0 - ((fx - 0.77).abs() * 0.6 + (fy - 0.67).abs())).clamp(0., 1.);
                        let sweep = (fx * 0.73 - fy * 0.68) + 0.16;
                        let mut c = mix(0x14252c, 0x2b5352, (255. * glow * glow) as u32);
                        if (0.20..0.40).contains(&sweep) {
                            c = mix(c, 0x518071, 75);
                        }
                        if (0.405..0.418).contains(&sweep) {
                            c = mix(c, 0x83a791, 84);
                        }
                        let grid = x % 32 == 0 && y % 32 == 0;
                        if grid {
                            c = mix(c, 0xa4c4b3, 48);
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
        let paths: &[&[(i32, i32)]] = match id {
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
                    (5, 3),
                    (14, 3),
                    (17, 6),
                    (17, 13),
                    (14, 16),
                    (5, 16),
                    (2, 13),
                    (2, 6),
                    (5, 3),
                ],
                &[(3, 9), (16, 9)],
                &[(10, 3), (7, 9), (10, 16), (13, 9), (10, 3)],
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
                &[(3, 14), (3, 7), (8, 7), (8, 13), (13, 13), (13, 5), (17, 5)],
                &[(15, 3), (17, 5), (15, 7)],
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
        };
        for path in paths {
            for points in path.windows(2) {
                self.stroke(x, y, points[0], points[1], c);
            }
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
        let (pw, ph) = (picture.width as i32, picture.height as i32);
        let (width, height) = if zoom {
            (pw, ph)
        } else if pw * h > ph * w {
            (w, (w * ph / pw).max(1))
        } else {
            ((h * pw / ph).max(1), h)
        };
        let (left, top) = (x + (w - width) / 2, y + (h - height) / 2);
        for yy in y.max(top)..(y + h).min(top + height) {
            let row = (yy - top) * ph / height;
            for xx in x.max(left)..(x + w).min(left + width) {
                let col = (xx - left) * pw / width;
                self.pixel(xx, yy, picture.pixels[(row * pw + col) as usize]);
            }
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
