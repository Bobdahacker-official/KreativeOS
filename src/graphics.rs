pub struct Screen {
    pub front: *mut u32,
    pub back: *mut u32,
    pub w: usize,
    pub h: usize,
    pub stride: usize,
    pub rgb: bool,
}
impl Screen {
    pub fn pixel(&mut self, x: i32, y: i32, c: u32) {
        if x >= 0 && y >= 0 && (x as usize) < self.w && (y as usize) < self.h {
            unsafe { *self.back.add(y as usize * self.w + x as usize) = c }
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
    pub fn round(&mut self, x: i32, y: i32, w: i32, h: i32, c: u32) {
        self.rect(x + 5, y, w - 10, h, c);
        self.rect(x, y + 5, w, h - 10, c);
        self.rect(x + 2, y + 2, w - 4, h - 4, c)
    }
    pub fn text(&mut self, x: i32, y: i32, s: &str, c: u32, scale: i32) {
        let mut xx = x;
        let mut yy = y;
        for b in s.bytes() {
            if b == b'\n' {
                xx = x;
                yy += 9 * scale;
                continue;
            }
            let rows = glyph(b);
            for (r, v) in rows.iter().enumerate() {
                for col in 0..5 {
                    if v & (1 << (4 - col)) != 0 {
                        self.rect(xx + col * scale, yy + r as i32 * scale, scale, scale, c)
                    }
                }
            }
            xx += 6 * scale;
        }
    }
    pub fn wrapped(&mut self, x: i32, y: i32, w: i32, h: i32, s: &str, c: u32) {
        let mut xx = x;
        let mut yy = y;
        for b in s.bytes() {
            if b == b'\n' || xx + 12 > x + w {
                xx = x;
                yy += 18;
                if b == b'\n' {
                    continue;
                }
            }
            if yy + 14 > y + h {
                break;
            }
            let a = [b];
            self.text(xx, yy, core::str::from_utf8(&a).unwrap_or("?"), c, 2);
            xx += 12
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
fn glyph(b: u8) -> [u8; 7] {
    match b.to_ascii_uppercase() {
        b'A' => [14, 17, 17, 31, 17, 17, 17],
        b'B' => [30, 17, 17, 30, 17, 17, 30],
        b'C' => [14, 17, 16, 16, 16, 17, 14],
        b'D' => [30, 17, 17, 17, 17, 17, 30],
        b'E' => [31, 16, 16, 30, 16, 16, 31],
        b'F' => [31, 16, 16, 30, 16, 16, 16],
        b'G' => [14, 17, 16, 23, 17, 17, 15],
        b'H' => [17, 17, 17, 31, 17, 17, 17],
        b'I' => [14, 4, 4, 4, 4, 4, 14],
        b'J' => [7, 2, 2, 2, 18, 18, 12],
        b'K' => [17, 18, 20, 24, 20, 18, 17],
        b'L' => [16, 16, 16, 16, 16, 16, 31],
        b'M' => [17, 27, 21, 21, 17, 17, 17],
        b'N' => [17, 25, 25, 21, 19, 19, 17],
        b'O' => [14, 17, 17, 17, 17, 17, 14],
        b'P' => [30, 17, 17, 30, 16, 16, 16],
        b'Q' => [14, 17, 17, 17, 21, 18, 13],
        b'R' => [30, 17, 17, 30, 20, 18, 17],
        b'S' => [15, 16, 16, 14, 1, 1, 30],
        b'T' => [31, 4, 4, 4, 4, 4, 4],
        b'U' => [17, 17, 17, 17, 17, 17, 14],
        b'V' => [17, 17, 17, 17, 17, 10, 4],
        b'W' => [17, 17, 17, 21, 21, 21, 10],
        b'X' => [17, 17, 10, 4, 10, 17, 17],
        b'Y' => [17, 17, 10, 4, 4, 4, 4],
        b'Z' => [31, 1, 2, 4, 8, 16, 31],
        b'0' => [14, 17, 19, 21, 25, 17, 14],
        b'1' => [4, 12, 4, 4, 4, 4, 14],
        b'2' => [14, 17, 1, 2, 4, 8, 31],
        b'3' => [30, 1, 1, 14, 1, 1, 30],
        b'4' => [2, 6, 10, 18, 31, 2, 2],
        b'5' => [31, 16, 16, 30, 1, 1, 30],
        b'6' => [14, 16, 16, 30, 17, 17, 14],
        b'7' => [31, 1, 2, 4, 8, 8, 8],
        b'8' => [14, 17, 17, 14, 17, 17, 14],
        b'9' => [14, 17, 17, 15, 1, 1, 14],
        b'.' => [0, 0, 0, 0, 0, 6, 6],
        b',' => [0, 0, 0, 0, 6, 6, 4],
        b':' => [0, 6, 6, 0, 6, 6, 0],
        b';' => [0, 6, 6, 0, 6, 6, 4],
        b'-' => [0, 0, 0, 31, 0, 0, 0],
        b'_' => [0, 0, 0, 0, 0, 0, 31],
        b'+' => [0, 4, 4, 31, 4, 4, 0],
        b'=' => [0, 0, 31, 0, 31, 0, 0],
        b'/' => [1, 1, 2, 4, 8, 16, 16],
        b'\\' => [16, 16, 8, 4, 2, 1, 1],
        b'(' => [2, 4, 8, 8, 8, 4, 2],
        b')' => [8, 4, 2, 2, 2, 4, 8],
        b'[' => [14, 8, 8, 8, 8, 8, 14],
        b']' => [14, 2, 2, 2, 2, 2, 14],
        b'<' => [1, 2, 4, 8, 4, 2, 1],
        b'>' => [16, 8, 4, 2, 4, 8, 16],
        b'!' => [4, 4, 4, 4, 4, 0, 4],
        b'?' => [14, 17, 1, 2, 4, 0, 4],
        b'*' => [0, 21, 14, 31, 14, 21, 0],
        b'#' => [10, 31, 10, 10, 31, 10, 0],
        b'%' => [25, 26, 2, 4, 8, 11, 19],
        b'"' => [10, 10, 10, 0, 0, 0, 0],
        b'\'' => [4, 4, 8, 0, 0, 0, 0],
        b'@' => [14, 17, 23, 21, 23, 16, 14],
        b'|' => [4, 4, 4, 4, 4, 4, 4],
        b' ' => [0; 7],
        _ => [31, 17, 5, 4, 4, 0, 4],
    }
}
