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
    unsafe { (ticks() - START) / (MHZ * 1000) }
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
unsafe fn mouse(c: u8) {
    cmd(0xd4);
    data(c);
    for _ in 0..100000 {
        if in8(0x64) & 1 != 0 {
            let _ = in8(0x60);
            break;
        }
    }
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
    mouse(0xf4);
    log(b"KreativeOS: input ready\r\n");
}
#[derive(Clone, Copy)]
pub enum Event {
    Key(u8),
    Mouse(i32, i32, u8),
}
pub struct Input {
    packet: [u8; 3],
    pos: usize,
    shift: bool,
    extended: bool,
}
impl Input {
    pub fn new() -> Self {
        Self {
            packet: [0; 3],
            pos: 0,
            shift: false,
            extended: false,
        }
    }
    pub fn poll(&mut self) -> Option<Event> {
        unsafe {
            let s = in8(0x64);
            if s & 1 == 0 {
                return None;
            }
            let b = in8(0x60);
            if s & 0x20 != 0 {
                if self.pos == 0 && b & 8 == 0 {
                    return None;
                }
                self.packet[self.pos] = b;
                self.pos += 1;
                if self.pos == 3 {
                    self.pos = 0;
                    if self.packet[0] & 0xc0 != 0 {
                        return None;
                    }
                    let dx = self.packet[1] as i32 - if self.packet[0] & 16 != 0 { 256 } else { 0 };
                    let dy = self.packet[2] as i32 - if self.packet[0] & 32 != 0 { 256 } else { 0 };
                    return Some(Event::Mouse(dx, -dy, self.packet[0] & 7));
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
