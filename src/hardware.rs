use core::arch::asm;
pub unsafe fn out8(p: u16, v: u8) {
    asm!("out dx, al",in("dx")p,in("al")v,options(nomem,nostack))
}
pub unsafe fn in8(p: u16) -> u8 {
    let v;
    asm!("in al, dx",in("dx")p,out("al")v,options(nomem,nostack));
    v
}
pub unsafe fn out16(p: u16, v: u16) {
    asm!("out dx, ax",in("dx")p,in("ax")v,options(nomem,nostack))
}
pub unsafe fn in16(p: u16) -> u16 {
    let v;
    asm!("in ax, dx",in("dx")p,out("ax")v,options(nomem,nostack));
    v
}
pub unsafe fn out32(p: u16, v: u32) {
    asm!("out dx, eax",in("dx")p,in("eax")v,options(nomem,nostack))
}
pub unsafe fn in32(p: u16) -> u32 {
    let v;
    asm!("in eax, dx",in("dx")p,out("eax")v,options(nomem,nostack));
    v
}
pub unsafe fn serial_init() {
    out8(0x3f9, 0);
    out8(0x3fb, 0x80);
    out8(0x3f8, 1);
    out8(0x3f9, 0);
    out8(0x3fb, 3);
    out8(0x3fa, 0xc7)
}
pub unsafe fn log(s: &[u8]) {
    for &b in s {
        for _ in 0..100000 {
            if in8(0x3fd) & 32 != 0 {
                break;
            }
        }
        out8(0x3f8, b)
    }
}
pub fn ticks() -> u64 {
    unsafe { core::arch::x86_64::_rdtsc() }
}
static mut MHZ: u64 = 1000;
static mut START: u64 = 0;
pub fn millis() -> u64 {
    micros() / 1000
}
pub fn micros() -> u64 {
    unsafe { (ticks() - START) / MHZ }
}
fn wait_write() {
    unsafe {
        for _ in 0..100000 {
            if in8(0x64) & 2 == 0 {
                break;
            }
        }
    }
}
unsafe fn cmd(c: u8) {
    wait_write();
    out8(0x64, c)
}
unsafe fn data(c: u8) {
    wait_write();
    out8(0x60, c)
}
static mut MOUSE_BYTES: usize = 3;

unsafe fn mouse_reply() -> Option<u8> {
    for _ in 0..100000 {
        let status = in8(0x64);
        if status & 0x21 == 0x21 {
            return Some(in8(0x60));
        }
    }
    None
}

unsafe fn mouse(command: u8) -> bool {
    cmd(0xd4);
    data(command);
    mouse_reply() == Some(0xfa)
}

pub unsafe fn init() {
    out8(0x21, 255);
    out8(0xa1, 255);
    // Calibrate the TSC against PIT channel 2 for roughly 10 ms.
    let gate = in8(0x61);
    out8(0x61, gate & !3);
    out8(0x43, 0xb0);
    out8(0x42, 0x9c);
    out8(0x42, 0x2e);
    let t = ticks();
    out8(0x61, (gate & !2) | 1);
    for _ in 0..10000000 {
        if in8(0x61) & 32 != 0 {
            break;
        }
    }
    let elapsed = ticks() - t;
    if elapsed > 100000 {
        MHZ = (elapsed / 10000).max(1)
    }
    out8(0x61, gate);
    START = ticks();
    if in8(0x64) == 0xff {
        log(b"KreativeOS: PS/2 controller absent\r\n");
        return;
    }
    for _ in 0..1024 {
        if in8(0x64) & 1 == 0 {
            break;
        }
        in8(0x60);
    }
    cmd(0xa8);
    cmd(0x20);
    let mut config = 0u8;
    for _ in 0..100000 {
        if in8(0x64) & 1 != 0 {
            config = in8(0x60);
            break;
        }
    }
    cmd(0x60);
    data((config | 0x40) & !0x33);
    cmd(0xae);
    mouse(0xf6);
    let wheel = [200, 100, 80]
        .into_iter()
        .all(|rate| mouse(0xf3) && mouse(rate))
        && mouse(0xf2)
        && mouse_reply() == Some(3);
    if wheel {
        MOUSE_BYTES = 4;
        log(b"KreativeOS: PS/2 wheel enabled\r\n");
    }
    mouse(0xf4);
    log(b"KreativeOS: input ready\r\n");
}
#[derive(Clone, Copy)]
pub enum Event {
    Key(u8),
    Mouse(i32, i32, u8, i32),
}
pub struct Input {
    packet: [u8; 4],
    pos: usize,
    shift: bool,
    control: bool,
    extended: bool,
}
impl Input {
    pub fn new() -> Self {
        Self {
            packet: [0; 4],
            pos: 0,
            shift: false,
            control: false,
            extended: false,
        }
    }
    pub fn poll(&mut self) -> Option<Event> {
        if let Some(event) = crate::usb::poll() {
            return Some(event);
        }
        unsafe {
            let s = in8(0x64);
            if s == 0xff || s & 1 == 0 {
                return None;
            }
            let b = in8(0x60);
            if s & 0x20 != 0 {
                if self.pos == 0 && b & 8 == 0 {
                    return None;
                }
                self.packet[self.pos] = b;
                self.pos += 1;
                if self.pos == MOUSE_BYTES {
                    self.pos = 0;
                    let axis = |byte: u8, sign: u8, overflow: u8| {
                        let negative = self.packet[0] & sign != 0;
                        if self.packet[0] & overflow != 0 {
                            if negative {
                                -255
                            } else {
                                255
                            }
                        } else {
                            i32::from(byte) - if negative { 256 } else { 0 }
                        }
                    };
                    let dx = axis(self.packet[1], 16, 64);
                    let dy = axis(self.packet[2], 32, 128);
                    let wheel = if MOUSE_BYTES == 4 {
                        -(((self.packet[3] << 4) as i8 >> 4) as i32)
                    } else {
                        0
                    };
                    return Some(Event::Mouse(dx, -dy, self.packet[0] & 7, wheel));
                }
                return None;
            }
            if b == 0xe0 {
                self.extended = true;
                return None;
            }
            if b == 42 || b == 54 {
                self.shift = true;
                return None;
            }
            if b == 170 || b == 182 {
                self.shift = false;
                return None;
            }
            if b == 0x1d || b == 0x9d {
                self.control = b == 0x1d;
                self.extended = false;
                return None;
            }
            if self.control && !self.extended {
                match b {
                    0x26 => return Some(Event::Key(0x84)),
                    0x14 => return Some(Event::Key(0xa0)),
                    0x20 => return Some(Event::Key(0xa1)),
                    0x39 => return Some(Event::Key(0xa2)),
                    0x29 => return Some(Event::Key(0xaa)),
                    0x32 => return Some(Event::Key(0xab)),
                    0x1f => return Some(Event::Key(0xac)),
                    0x02..=0x05 => return Some(Event::Key(0xa3 + b - 0x02)),
                    0x2e => return Some(Event::Key(0xa7)),
                    0x2f => return Some(Event::Key(if self.shift { 0xa9 } else { 0xa8 })),
                    _ => {}
                }
            }
            if b & 128 != 0 {
                self.extended = false;
                return None;
            }
            if self.extended {
                self.extended = false;
                return Some(Event::Key(match b {
                    72 => 0x80,
                    80 => 0x81,
                    75 => 0x82,
                    77 => 0x83,
                    _ => 0,
                }));
            }
            let normal =
                b"\0\x1b1234567890-=\x08\tqwertyuiop[]\n\0asdfghjkl;'`\0\\zxcvbnm,./\0*\0 ";
            let shifted =
                b"\0\x1b!@#$%^&*()_+\x08\tQWERTYUIOP{}\n\0ASDFGHJKL:\"~\0|ZXCVBNM<>?\0*\0 ";
            let ch = if (b as usize) < normal.len() {
                if self.shift {
                    shifted[b as usize]
                } else {
                    normal[b as usize]
                }
            } else {
                match b {
                    59..=68 => 0x90 + b - 59,
                    _ => 0,
                }
            };
            if ch == 0 {
                None
            } else {
                Some(Event::Key(ch))
            }
        }
    }
}
pub unsafe fn pci(bus: u8, slot: u8, func: u8, reg: u8) -> u32 {
    out32(
        0xcf8,
        0x80000000
            | ((bus as u32) << 16)
            | ((slot as u32) << 11)
            | ((func as u32) << 8)
            | (reg as u32 & 0xfc),
    );
    in32(0xcfc)
}
pub unsafe fn pci_write(bus: u8, slot: u8, func: u8, reg: u8, v: u32) {
    out32(
        0xcf8,
        0x80000000
            | ((bus as u32) << 16)
            | ((slot as u32) << 11)
            | ((func as u32) << 8)
            | (reg as u32 & 0xfc),
    );
    out32(0xcfc, v)
}
pub fn clock() -> (u8, u8, u8) {
    fn read(r: u8) -> u8 {
        unsafe {
            out8(0x70, r);
            in8(0x71)
        }
    }
    let mode = read(0xb);
    let cv = |v: u8| {
        if mode & 4 != 0 {
            v
        } else {
            (v >> 4) * 10 + (v & 15)
        }
    };
    let h = read(4);
    let mut hour = cv(h & 127);
    if mode & 2 == 0 {
        hour = hour % 12 + if h & 128 != 0 { 12 } else { 0 }
    }
    (hour, cv(read(2)), cv(read(0)))
}
pub fn reboot() -> ! {
    unsafe { cmd(0xfe) }
    loop {
        core::hint::spin_loop()
    }
}

/// Snapshot the RTC twice to avoid a date rollover; require a plausible UTC date.
pub fn unix_time() -> Option<u64> {
    unsafe fn read(r: u8) -> u8 {
        out8(0x70, r);
        in8(0x71)
    }
    unsafe fn snapshot() -> Option<[u8; 7]> {
        let start = millis();
        while read(0x0a) & 0x80 != 0 {
            if millis() - start > 100 {
                return None;
            }
        }
        Some([
            read(0),
            read(2),
            read(4),
            read(7),
            read(8),
            read(9),
            read(0x0b),
        ])
    }
    let mut a = unsafe { snapshot()? };
    for _ in 0..5 {
        let b = unsafe { snapshot()? };
        if a == b {
            let cv = |v: u8| {
                if b[6] & 4 != 0 {
                    v as u64
                } else {
                    ((v >> 4) * 10 + (v & 15)) as u64
                }
            };
            let year = 2000 + cv(b[5]);
            let month = cv(b[4]);
            let day = cv(b[3]);
            if !(2024..=2099).contains(&year)
                || !(1..=12).contains(&month)
                || !(1..=31).contains(&day)
            {
                return None;
            }
            let leap = |y: u64| y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
            let mut days = 0;
            for y in 1970..year {
                days += if leap(y) { 366 } else { 365 };
            }
            let months = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
            for m in 1..month {
                days += months[m as usize - 1] + if m == 2 && leap(year) { 1 } else { 0 };
            }
            let mut hour = cv(b[2] & 127);
            if b[6] & 2 == 0 {
                hour = hour % 12 + if b[2] & 128 != 0 { 12 } else { 0 };
            }
            return Some((days + day - 1) * 86400 + hour * 3600 + cv(b[1]) * 60 + cv(b[0]));
        }
        a = b;
    }
    None
}
