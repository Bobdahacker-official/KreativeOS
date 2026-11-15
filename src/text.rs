use core::fmt;
#[derive(Clone, Copy)]
pub struct Text<const N: usize> {
    pub bytes: [u8; N],
    pub len: usize,
}
impl<const N: usize> Text<N> {
    pub const fn new() -> Self {
        Self {
            bytes: [0; N],
            len: 0,
        }
    }
    pub fn clear(&mut self) {
        self.len = 0;
    }
    pub fn push(&mut self, b: u8) {
        if self.len < N {
            self.bytes[self.len] = b;
            self.len += 1
        }
    }
    pub fn pop(&mut self) {
        self.len = self.len.saturating_sub(1)
    }
    pub fn set(&mut self, s: &str) {
        self.clear();
        self.append(s)
    }
    pub fn append(&mut self, s: &str) {
        for b in s.bytes() {
            self.push(b)
        }
    }
    pub fn str(&self) -> &str {
        core::str::from_utf8(&self.bytes[..self.len]).unwrap_or("")
    }
}
impl<const N: usize> fmt::Write for Text<N> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.append(s);
        Ok(())
    }
}
