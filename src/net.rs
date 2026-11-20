//! RTL8139 + Ethernet/ARP/IPv4/UDP/DNS/TCP/HTTP. All wire reads are length checked.
use crate::{hardware::*, text::Text};
use alloc::collections::VecDeque;
pub static mut DMA: usize = 0;
const IP: [u8; 4] = [10, 0, 2, 15];
const GATE: [u8; 4] = [10, 0, 2, 2];
const DNS: [u8; 4] = [10, 0, 2, 3];
fn u16be(b: &[u8], n: usize) -> u16 {
    u16::from_be_bytes([b[n], b[n + 1]])
}
fn u32be(b: &[u8], n: usize) -> u32 {
    u32::from_be_bytes([b[n], b[n + 1], b[n + 2], b[n + 3]])
}
fn w16(b: &mut [u8], n: usize, v: u16) {
    b[n..n + 2].copy_from_slice(&v.to_be_bytes())
}
fn w32(b: &mut [u8], n: usize, v: u32) {
    b[n..n + 4].copy_from_slice(&v.to_be_bytes())
}
fn sum(b: &[u8]) -> u32 {
    let mut s = 0;
    for c in b.chunks(2) {
        s += ((c[0] as u32) << 8) + if c.len() > 1 { c[1] as u32 } else { 0 }
    }
    s
}
fn fold(mut s: u32) -> u16 {
    while s >> 16 != 0 {
        s = (s & 65535) + (s >> 16)
    }
    !(s as u16)
}
pub struct Network {
    io: u16,
    intel: Option<crate::e1000::Adapter>,
    pub wifi: crate::wifi::Module,
    pub response: Option<crate::web::Response>,
    pub mac: [u8; 6],
    pub received: u64,
    pub sent: u64,
    pub status: &'static str,
    pub body: Text<12288>,
    pub changed: bool,
    rx: usize,
    tx: usize,
    gateway: [u8; 6],
    have_gateway: bool,
    remote: [u8; 4],
    pub host: Text<128>,
    pub path: Text<256>,
    pub state: u8,
    secure: bool,
    pub remote_port: u16,
    pub tls_pending: bool,
    pub verified: bool,
    pub cert_rejected: bool,
    pub raw: VecDeque<u8>,
    pub raw_eof: bool,
    pub raw_waiting: bool,
    seq: u32,
    ack: u32,
    port: u16,
    deadline: u64,
    retries: u8,
    last: [u8; 1536],
    last_len: usize,
}
impl Network {
    pub unsafe fn new() -> Self {
        let mut n = Self {
            io: 0,
            wifi: crate::wifi::Module::new(),
            response: None,
            intel: None,
            mac: [0; 6],
            received: 0,
            sent: 0,
            status: "No supported Ethernet adapter",
            body: Text::new(),
            changed: true,
            rx: 0,
            tx: 0,
            gateway: [0; 6],
            have_gateway: false,
            remote: [0; 4],
            host: Text::new(),
            path: Text::new(),
            state: 0,
            secure: false,
            remote_port: 80,
            tls_pending: false,
            verified: false,
            cert_rejected: false,
            raw: VecDeque::with_capacity(32768),
            raw_eof: false,
            raw_waiting: false,
            seq: 0,
            ack: 0,
            port: 49152,
            deadline: 0,
            retries: 0,
            last: [0; 1536],
            last_len: 0,
        };
        let mut intel_slot = None;
        for bus in 0..=255 {
            for slot in 0..32 {
                for func in 0..8 {
                    let id = pci(bus, slot, func, 0);
                    if id == 0xffff_ffff {
                        continue;
                    }
                    if id == 0x100e8086 && intel_slot.is_none() {
                        intel_slot = Some((bus, slot, func));
                    }
                    if id == 0x813910ec && n.io == 0 {
                        let bar = pci(bus, slot, func, 0x10);
                        if bar & 1 == 0 {
                            continue;
                        }
                        n.io = (bar & 0xfffc) as u16;
                        let c = pci(bus, slot, func, 4);
                        pci_write(bus, slot, func, 4, (c & 0xffff) | 5)
                    }
                }
            }
        }
        if n.io == 0 {
            if let Some((bus, slot, func)) = intel_slot {
                if let Some(adapter) = crate::e1000::Adapter::init(bus, slot, func, DMA) {
                    n.mac = adapter.mac;
                    n.intel = Some(adapter);
                    n.status = "Ethernet ready: 10.0.2.15";
                    log(b"KreativeOS: Intel 82540EM ready\r\n");
                }
            }
            return n;
        }
        let p = n.io;
        out8(p + 0x52, 0);
        out8(p + 0x37, 0x10);
        let start = millis();
        while in8(p + 0x37) & 0x10 != 0 {
            if millis() - start > 1000 {
                n.io = 0;
                return n;
            }
        }
        core::ptr::write_bytes(DMA as *mut u8, 0, 32768);
        for i in 0..6 {
            n.mac[i] = in8(p + i as u16)
        }
        out32(p + 0x30, DMA as u32);
        out16(p + 0x3c, 0);
        out32(p + 0x44, 0x8f);
        out32(p + 0x40, 0x03000700);
        out8(p + 0x37, 0x0c);
        for i in 0..4 {
            out32(p + 0x20 + i * 4, (DMA + 16384 + i as usize * 2048) as u32)
        }
        n.status = "Ethernet ready: 10.0.2.15";
        log(b"KreativeOS: RTL8139 ready\r\n");
        n
    }
    pub fn adapter_name(&self) -> &'static str {
        if self.intel.is_some() {
            "Intel 82540EM"
        } else if self.io != 0 {
            "RTL8139"
        } else {
            "None"
        }
    }
    pub fn online(&self) -> bool {
        self.io != 0 || self.intel.is_some()
    }
    fn transmit(&mut self, frame: &[u8]) {
        if !self.online() || frame.len() > 1536 {
            return;
        }
        if let Some(adapter) = &mut self.intel {
            if adapter.transmit(frame) {
                self.sent += 1;
            }
            return;
        }
        unsafe {
            let slot = self.tx % 4;
            let t = millis();
            while in32(self.io + 0x10 + slot as u16 * 4) & (1 << 13) == 0 {
                if millis() - t > 50 {
                    return;
                }
            }
            let p = (DMA + 16384 + slot * 2048) as *mut u8;
            core::ptr::write_bytes(p, 0, frame.len().max(60));
            core::ptr::copy_nonoverlapping(frame.as_ptr(), p, frame.len());
            core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
            out32(self.io + 0x10 + slot as u16 * 4, frame.len().max(60) as u32);
            self.tx += 1;
            self.sent += 1;
        }
    }
    fn arp(&mut self, reply: bool, target_mac: [u8; 6], target_ip: [u8; 4]) {
        let mut p = [0u8; 42];
        p[..6].copy_from_slice(&if reply { target_mac } else { [255; 6] });
        p[6..12].copy_from_slice(&self.mac);
        w16(&mut p, 12, 0x0806);
        w16(&mut p, 14, 1);
        w16(&mut p, 16, 0x0800);
        p[18] = 6;
        p[19] = 4;
        w16(&mut p, 20, if reply { 2 } else { 1 });
        p[22..28].copy_from_slice(&self.mac);
        p[28..32].copy_from_slice(&IP);
        p[32..38].copy_from_slice(&target_mac);
        p[38..42].copy_from_slice(&target_ip);
        self.transmit(&p)
    }
    fn ip_send(&mut self, dest: [u8; 4], protocol: u8, payload: &[u8], remember: bool) {
        if payload.len() > 1460 {
            return;
        }
        let mut f = [0u8; 1536];
        f[..6].copy_from_slice(&self.gateway);
        f[6..12].copy_from_slice(&self.mac);
        w16(&mut f, 12, 0x0800);
        f[14] = 0x45;
        w16(&mut f, 16, (20 + payload.len()) as u16);
        f[22] = 64;
        f[23] = protocol;
        f[26..30].copy_from_slice(&IP);
        f[30..34].copy_from_slice(&dest);
        let c = fold(sum(&f[14..34]));
        w16(&mut f, 24, c);
        f[34..34 + payload.len()].copy_from_slice(payload);
        let len = 34 + payload.len();
        if remember {
            self.last[..len].copy_from_slice(&f[..len]);
            self.last_len = len;
            self.deadline = millis() + 1500;
            self.retries = 0
        }
        self.transmit(&f[..len])
    }
    fn dns(&mut self) {
        let mut p = [0u8; 512];
        w16(&mut p, 0, 53000);
        w16(&mut p, 2, 53);
        w16(&mut p, 8, self.port);
        w16(&mut p, 10, 0x0100);
        w16(&mut p, 12, 1);
        let mut len = 20;
        for label in self.host.str().split('.') {
            if label.is_empty() || label.len() > 63 {
                self.fail("Invalid DNS name");
                return;
            }
            p[len] = label.len() as u8;
            len += 1;
            p[len..len + label.len()].copy_from_slice(label.as_bytes());
            len += label.len()
        }
        p[len] = 0;
        len += 1;
        w16(&mut p, len, 1);
        w16(&mut p, len + 2, 1);
        len += 4;
        w16(&mut p, 4, len as u16);
        self.state = 2;
        self.status = "Resolving hostname";
        self.ip_send(DNS, 17, &p[..len], true)
    }
    fn tcp(&mut self, flags: u8, data: &[u8], remember: bool) {
        let mut p = [0u8; 1460];
        if data.len() > 1440 {
            return;
        }
        w16(&mut p, 0, self.port);
        w16(&mut p, 2, self.remote_port);
        w32(&mut p, 4, self.seq);
        w32(&mut p, 8, self.ack);
        p[12] = 0x50;
        p[13] = flags;
        w16(&mut p, 14, 16384);
        p[20..20 + data.len()].copy_from_slice(data);
        let len = 20 + data.len();
        let c = fold(sum(&IP) + sum(&self.remote) + 6 + len as u32 + sum(&p[..len]));
        w16(&mut p, 16, c);
        self.ip_send(self.remote, 6, &p[..len], remember)
    }
    fn connect(&mut self) {
        self.seq = (ticks() as u32) & 0x7fffffff;
        self.ack = 0;
        self.state = 3;
        self.status = "Connecting to HTTP server";
        self.tcp(2, &[], true);
        self.seq = self.seq.wrapping_add(1)
    }
    fn fail(&mut self, s: &'static str) {
        self.state = 0;
        self.status = s;
        self.changed = true
    }
    pub fn browse(&mut self, url: &str) {
        self.body.clear();
        self.response = None;
        self.verified = false;
        self.cert_rejected = false;
        self.tls_pending = false;
        self.raw.clear();
        self.raw_eof = false;
        self.raw_waiting = false;
        self.changed = true;
        self.have_gateway = false;
        self.last_len = 0;
        if !self.online() && !self.wifi.joined {
            self.fail("Connect Ethernet or Wi-Fi");
            return;
        }
        let s = if let Some(v) = url.strip_prefix("https://") {
            self.secure = true;
            self.remote_port = 443;
            v
        } else if let Some(v) = url.strip_prefix("http://") {
            self.secure = false;
            self.remote_port = 80;
            v
        } else {
            self.fail("Use an https://, http:// or file: address");
            return;
        };
        let (authority, path) = s.split_once('/').unwrap_or((s, ""));
        let host = if let Some((h, p)) = authority.split_once(':') {
            match p.parse::<u16>() {
                Ok(port) if port > 0 => {
                    self.remote_port = port;
                    h
                }
                _ => {
                    self.fail("Invalid port");
                    return;
                }
            }
        } else {
            authority
        };
        if host.len() > 127
            || path.len() > 250
            || !host
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
            || host.is_empty()
            || !s.bytes().all(|b| b >= 33 && b < 127)
        {
            self.fail("Invalid HTTP address");
            return;
        }
        self.host.set(host);
        self.path.set("/");
        self.path.append(path);
        if self.wifi.joined && self.secure {
            self.status = "Connecting over Wi-Fi";
            if self.wifi.connect(host, self.remote_port).is_ok() {
                self.state = 6;
                self.tls_pending = true;
            } else {
                self.fail("Wi-Fi TCP connection failed");
            }
            return;
        }
        if !self.online() {
            self.fail("Use an HTTPS address over Wi-Fi");
            return;
        }
        self.remote = [0; 4];
        let mut octets = host.split('.');
        let mut numeric = true;
        for i in 0..4 {
            if let Some(v) = octets.next().and_then(|v| v.parse::<u8>().ok()) {
                self.remote[i] = v
            } else {
                numeric = false
            }
        }
        if octets.next().is_some() {
            numeric = false
        }
        if !numeric {
            self.remote = [0; 4]
        }
        self.port = 49152 + (ticks() % 15000) as u16;
        self.state = 1;
        self.status = "Finding Ethernet gateway";
        self.retries = 0;
        self.deadline = millis() + 1000;
        self.arp(false, [0; 6], GATE)
    }
    pub fn poll(&mut self) {
        if !self.online() {
            return;
        }
        for _ in 0..16 {
            let mut f = [0u8; 1536];
            let len = if let Some(adapter) = &mut self.intel {
                let Some(len) = adapter.receive(&mut f) else {
                    break;
                };
                len
            } else {
                unsafe {
                    if in8(self.io + 0x37) & 1 != 0 {
                        break;
                    }
                    let p = (DMA + self.rx) as *const u8;
                    let status = core::ptr::read_volatile(p as *const u16);
                    let len = core::ptr::read_volatile(p.add(2) as *const u16) as usize;
                    if len < 4 || len > 1522 {
                        out8(self.io + 0x37, 4);
                        self.rx = 0;
                        out32(self.io + 0x30, DMA as u32);
                        out16(self.io + 0x38, 0xfff0);
                        out8(self.io + 0x37, 12);
                        break;
                    }
                    if status & 1 != 0 {
                        for i in 0..len - 4 {
                            f[i] = core::ptr::read_volatile((DMA + self.rx + 4 + i) as *const u8)
                        }
                    }
                    self.rx = (self.rx + len + 4 + 3) & !3;
                    self.rx %= 8192;
                    out16(self.io + 0x38, (self.rx as u16).wrapping_sub(16));
                    out16(self.io + 0x3e, 0xffff);
                    if status & 1 == 0 {
                        continue;
                    }
                    len - 4
                }
            };
            self.received += 1;
            self.frame(&f[..len]);
        }
        if self.state != 0 && (self.state != 6 || self.raw_waiting) && millis() > self.deadline {
            if self.retries >= 3 {
                self.fail("Network request timed out")
            } else {
                self.retries += 1;
                self.deadline = millis() + 1500;
                if self.state == 1 {
                    self.arp(false, [0; 6], GATE)
                } else if self.state == 2 || self.state == 3 || self.state == 4 || self.raw_waiting
                {
                    let mut copy = [0u8; 1536];
                    copy[..self.last_len].copy_from_slice(&self.last[..self.last_len]);
                    self.transmit(&copy[..self.last_len])
                } else {
                    self.fail("HTTP response timed out")
                }
            }
        }
    }
    pub fn raw_send(&mut self, data: &[u8]) {
        self.tcp(0x18, data, true);
        self.seq = self.seq.wrapping_add(data.len() as u32);
        self.raw_waiting = true;
    }
    pub fn tls_result(&mut self, verified: bool, status: &'static str) {
        if self.wifi.active {
            self.wifi.close();
        } else if self.state == 6 {
            self.tcp(0x11, &[], false);
            self.seq = self.seq.wrapping_add(1);
        }
        self.state = 0;
        self.raw_waiting = false;
        self.verified = verified;
        self.status = status;
        self.changed = true;
    }
    fn frame(&mut self, f: &[u8]) {
        if f.len() < 14 {
            return;
        }
        match u16be(f, 12) {
            0x0806 => {
                if f.len() < 42
                    || u16be(f, 14) != 1
                    || u16be(f, 16) != 0x800
                    || f[18] != 6
                    || f[19] != 4
                {
                    return;
                }
                let mut mac = [0; 6];
                mac.copy_from_slice(&f[22..28]);
                let mut addr = [0; 4];
                addr.copy_from_slice(&f[28..32]);
                if u16be(f, 20) == 1 && f[38..42] == IP {
                    self.arp(true, mac, addr)
                }
                if addr == GATE && f[38..42] == IP {
                    self.gateway = mac;
                    self.have_gateway = true;
                    if self.state == 1 {
                        if self.remote == [0; 4] {
                            self.dns()
                        } else {
                            self.connect()
                        }
                    }
                }
            }
            0x0800 => self.ip(&f[14..]),
            _ => (),
        }
    }
    fn ip(&mut self, p: &[u8]) {
        if p.len() < 20 || p[0] >> 4 != 4 {
            return;
        }
        let h = (p[0] & 15) as usize * 4;
        let total = u16be(p, 2) as usize;
        if h < 20
            || h > p.len()
            || total < h
            || total > p.len()
            || p[16..20] != IP
            || u16be(p, 6) & 0x3fff != 0
            || fold(sum(&p[..h])) != 0
        {
            return;
        }
        let d = &p[h..total];
        if p[9] == 17
            && self.state == 2
            && p[12..16] == DNS
            && d.len() >= 20
            && u16be(d, 0) == 53
            && u16be(d, 2) == 53000
            && u16be(d, 8) == self.port
        {
            let plen = u16be(d, 4) as usize;
            if plen < 20 || plen > d.len() {
                return;
            }
            let q = &d[8..plen];
            if q[3] & 15 != 0 {
                self.fail("DNS name was not found");
                return;
            }
            let mut pos = 12;
            for _ in 0..u16be(q, 4) {
                pos = match skip_name(q, pos) {
                    Some(v) => v,
                    None => return,
                };
                pos += 4;
                if pos > q.len() {
                    return;
                }
            }
            for _ in 0..u16be(q, 6) {
                pos = match skip_name(q, pos) {
                    Some(v) => v,
                    None => return,
                };
                if pos + 10 > q.len() {
                    return;
                }
                let kind = u16be(q, pos);
                let class = u16be(q, pos + 2);
                let len = u16be(q, pos + 8) as usize;
                pos += 10;
                if pos + len > q.len() {
                    return;
                }
                if kind == 1 && class == 1 && len == 4 {
                    self.remote.copy_from_slice(&q[pos..pos + 4]);
                    self.connect();
                    return;
                }
                pos += len
            }
            self.fail("DNS response has no IPv4 address");
        } else if p[9] == 6
            && self.state >= 3
            && p[12..16] == self.remote
            && d.len() >= 20
            && u16be(d, 0) == self.remote_port
            && u16be(d, 2) == self.port
        {
            if fold(sum(&p[12..20]) + 6 + d.len() as u32 + sum(d)) != 0 {
                return;
            }
            let off = (d[12] >> 4) as usize * 4;
            if off < 20 || off > d.len() {
                return;
            }
            let flags = d[13];
            let seq = u32be(d, 4);
            let ack = u32be(d, 8);
            if flags & 4 != 0 {
                self.fail("Connection refused by server");
                return;
            }
            if self.state == 3 && flags & 0x12 == 0x12 && ack == self.seq {
                self.ack = seq.wrapping_add(1);
                self.tcp(0x10, &[], false);
                if self.secure {
                    self.state = 6;
                    self.tls_pending = true;
                    self.status = "Securing connection...";
                    return;
                }
                let mut req: Text<512> = Text::new();
                req.append("GET ");
                req.append(self.path.str());
                req.append(" HTTP/1.0\r\nHost: ");
                req.append(self.host.str());
                req.append("\r\nUser-Agent: KreativeOS/0.1\r\nConnection: close\r\n\r\n");
                self.tcp(0x18, &req.bytes[..req.len], true);
                self.seq = self.seq.wrapping_add(req.len as u32);
                self.state = 4;
                self.status = "Receiving page";
            } else if self.state == 6 {
                if flags & 16 != 0 && ack == self.seq {
                    self.raw_waiting = false;
                }
                if seq != self.ack {
                    if !d[off..].is_empty() {
                        self.tcp(0x10, &[], false);
                    }
                    return;
                }
                let data = &d[off..];
                if self.raw.len() + data.len() > 32768 {
                    return;
                }
                self.raw.extend(data.iter().copied());
                self.ack = self.ack.wrapping_add(data.len() as u32);
                if flags & 1 != 0 {
                    self.ack = self.ack.wrapping_add(1);
                    self.raw_eof = true;
                }
                if !data.is_empty() || flags & 1 != 0 {
                    self.tcp(0x10, &[], false);
                }
            } else if self.state >= 4 {
                if ack != self.seq {
                    return;
                }
                if flags & 16 != 0 {
                    self.state = 5;
                    self.deadline = millis() + 10000
                }
                if seq != self.ack {
                    self.tcp(0x10, &[], false);
                    return;
                }
                let data = &d[off..];
                for &b in data {
                    self.body.push(if b < 128 { b } else { b'?' })
                }
                self.ack = self.ack.wrapping_add(data.len() as u32);
                if !data.is_empty() {
                    self.changed = true
                }
                if flags & 1 != 0 {
                    self.ack = self.ack.wrapping_add(1);
                    self.tcp(0x11, &[], false);
                    self.seq = self.seq.wrapping_add(1);
                    self.state = 0;
                    self.status = if self.body.len == 12288 {
                        "Page loaded (12 KiB limit)"
                    } else {
                        "Page loaded"
                    };
                    self.changed = true
                } else if !data.is_empty() {
                    self.tcp(0x10, &[], false)
                }
            }
        }
    }
}
fn skip_name(p: &[u8], mut n: usize) -> Option<usize> {
    for _ in 0..128 {
        let b = *p.get(n)?;
        if b & 0xc0 == 0xc0 {
            return if n + 2 <= p.len() { Some(n + 2) } else { None };
        }
        if b == 0 {
            return Some(n + 1);
        }
        if b > 63 {
            return None;
        }
        n += 1 + b as usize;
        if n > p.len() {
            return None;
        }
    }
    None
}
/// Text layout for HTML bodies. Suppresses script/style and decodes common entities.
pub fn readable(src: &str, out: &mut Text<12288>) {
    out.clear();
    let src = src.split_once("\r\n\r\n").map(|(_, b)| b).unwrap_or(src);
    let b = src.as_bytes();
    let mut i = 0;
    let mut hidden = false;
    while i < b.len() {
        if b[i] == b'<' {
            let start = i + 1;
            while i < b.len() && b[i] != b'>' {
                i += 1
            }
            let tag = core::str::from_utf8(&b[start..i]).unwrap_or("");
            if tag.starts_with("script") || tag.starts_with("style") {
                hidden = true
            }
            if tag.starts_with("/script") || tag.starts_with("/style") {
                hidden = false
            }
            if !hidden
                && (tag.starts_with('p')
                    || tag.starts_with("/p")
                    || tag.starts_with("br")
                    || tag.starts_with('h')
                    || tag.starts_with("/h")
                    || tag.starts_with("li"))
            {
                out.push(b'\n')
            }
            i += 1;
            continue;
        }
        if hidden {
            i += 1;
            continue;
        }
        if b[i] == b'&' {
            let rest = &src[i..];
            let mut done = false;
            for (s, c) in [
                ("&amp;", b'&'),
                ("&lt;", b'<'),
                ("&gt;", b'>'),
                ("&quot;", b'"'),
                ("&nbsp;", b' '),
            ] {
                if rest.starts_with(s) {
                    out.push(c);
                    i += s.len();
                    done = true;
                    break;
                }
            }
            if done {
                continue;
            }
        }
        let c = b[i];
        if c == b'\r' || c == b'\t' {
            out.push(b' ')
        } else if c >= 32 || c == b'\n' {
            out.push(c)
        }
        i += 1
    }
}
