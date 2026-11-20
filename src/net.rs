//! RTL8139 + Ethernet/ARP/IPv4/UDP/DNS/TCP/HTTP. All wire reads are length checked.
use crate::{
    dhcp::{self, Address, Config, Offer},
    hardware::*,
    text::Text,
};
use alloc::collections::VecDeque;
pub static mut DMA: usize = 0;
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
    pub config: Config,
    pub vlan: u16,
    pub ethernet_status: &'static str,
    dhcp: Option<Discovery>,
    lease: Option<Lease>,
    neighbors: [Option<(Address, [u8; 6], u64)>; 8],
    neighbor_slot: usize,
    dns_id: u16,
    dns_port: u16,
    dns_name: Text<253>,
    dns_aliases: u8,
    link_up: bool,
    next_hop: Address,
    io: u16,
    intel: Option<crate::e1000::Adapter>,
    pub wifi: crate::wifi::Module,
    pub response: Option<crate::web::Response>,
    pub mac: [u8; 6],
    pub received: u64,
    pub sent: u64,
    pub received_bytes: u64,
    pub sent_bytes: u64,
    pub status: &'static str,
    pub body: Text<12288>,
    pub changed: bool,
    rx: usize,
    tx: usize,
    gateway: [u8; 6],
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
struct Discovery {
    xid: u32,
    offer: Option<Offer>,
    attempt: u8,
    deadline: u64,
}
struct Lease {
    server: Address,
    renewal: u64,
    rebinding: u64,
    expires: u64,
}

fn entropy() -> u64 {
    if core::arch::x86_64::__cpuid(1).ecx & (1 << 30) != 0 {
        for _ in 0..8 {
            let mut value = 0;
            if unsafe { core::arch::x86_64::_rdrand64_step(&mut value) } == 1 {
                return value;
            }
        }
    }
    ticks()
}
impl Network {
    pub unsafe fn new() -> Self {
        let mut n = Self {
            config: if cfg!(feature = "tls-smoke") {
                Config::QEMU
            } else {
                Config::UNCONFIGURED
            },
            vlan: 0,
            ethernet_status: if cfg!(feature = "tls-smoke") {
                "Static IPv4"
            } else {
                "IPv4 not configured"
            },
            dhcp: None,
            lease: None,
            neighbors: [None; 8],
            neighbor_slot: 0,
            dns_id: 0,
            dns_port: 0,
            dns_name: Text::new(),
            dns_aliases: 0,
            link_up: false,
            next_hop: [0; 4],
            io: 0,
            wifi: crate::wifi::Module::new(),
            response: None,
            intel: None,
            mac: [0; 6],
            received: 0,
            sent: 0,
            received_bytes: 0,
            sent_bytes: 0,
            status: "No supported Ethernet adapter",
            body: Text::new(),
            changed: true,
            rx: 0,
            tx: 0,
            gateway: [0; 6],
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
                    if let Some(model) = crate::e1000::Model::from_pci(id) {
                        if intel_slot.is_none() {
                            intel_slot = Some((bus, slot, func, model));
                        }
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
            if let Some((bus, slot, func, model)) = intel_slot {
                if let Some(adapter) = crate::e1000::Adapter::init(bus, slot, func, model, DMA) {
                    n.mac = adapter.mac;
                    n.intel = Some(adapter);
                    n.status = "Ethernet adapter ready";
                    log(b"KreativeOS: ");
                    log(model.name().as_bytes());
                    log(b" ready\r\n");
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
        n.status = "Ethernet adapter ready";
        log(b"KreativeOS: RTL8139 ready\r\n");
        n
    }
    pub fn adapter_name(&self) -> &'static str {
        if let Some(adapter) = &self.intel {
            adapter.model.name()
        } else if self.io != 0 {
            "RTL8139"
        } else {
            "None"
        }
    }
    pub fn online(&self) -> bool {
        self.io != 0 || self.intel.is_some()
    }
    pub fn link_speed(&self) -> Option<u16> {
        if let Some(adapter) = &self.intel {
            adapter.link_speed()
        } else if self.io != 0 {
            let status = unsafe { in8(self.io + 0x58) };
            match status & 12 {
                0 => Some(100),
                8 => Some(10),
                _ => None,
            }
        } else {
            None
        }
    }
    pub fn static_config(&mut self, config: Config, vlan: u16) -> bool {
        if !self.online() || self.state != 0 || vlan > 4094 || !config.valid() {
            return false;
        }
        self.config = config;
        self.vlan = vlan;
        self.dhcp = None;
        self.lease = None;
        self.neighbors.fill(None);
        self.ethernet_status = "Static IPv4";
        true
    }
    pub fn start_dhcp(&mut self, vlan: u16) -> bool {
        if !self.online() || self.state != 0 || vlan > 4094 {
            return false;
        }
        self.vlan = vlan;
        self.config = Config::UNCONFIGURED;
        self.lease = None;
        self.neighbors.fill(None);
        self.ethernet_status = "Looking for a DHCP server";
        self.dhcp = Some(Discovery {
            xid: entropy() as u32,
            offer: None,
            attempt: 0,
            deadline: millis(),
        });
        true
    }
    fn dhcp_tick(&mut self) {
        let now = millis();
        if self
            .lease
            .as_ref()
            .is_some_and(|lease| now >= lease.expires)
        {
            if self.state != 0 {
                self.fail("Ethernet DHCP lease expired");
            }
            self.start_dhcp(self.vlan);
        }
        if self.dhcp.is_none()
            && self
                .lease
                .as_ref()
                .is_some_and(|lease| now >= lease.renewal)
        {
            self.dhcp = Some(Discovery {
                xid: entropy() as u32,
                offer: None,
                attempt: 0,
                deadline: now,
            });
        }
        let Some(state) = &self.dhcp else { return };
        if now < state.deadline {
            return;
        }
        let (destination, phase_end) = match &self.lease {
            Some(lease) if now < lease.rebinding => (lease.server, lease.rebinding),
            Some(lease) => ([255; 4], lease.expires),
            None => ([255; 4], u64::MAX),
        };
        let broadcast = destination == [255; 4];
        if self.lease.is_none() && state.attempt == 4 {
            self.ethernet_status = "DHCP: retrying discovery";
            self.dhcp = Some(Discovery {
                xid: entropy() as u32,
                offer: None,
                attempt: 0,
                deadline: now + 10_000,
            });
            return;
        }
        let mac = if broadcast {
            [255; 6]
        } else if let Some(hop) = self.config.route(destination) {
            if let Some(mac) = self.neighbor(hop) {
                mac
            } else {
                self.dhcp.as_mut().unwrap().deadline = (now + 500).min(phase_end);
                self.arp(false, [0; 6], hop);
                return;
            }
        } else {
            self.dhcp.as_mut().unwrap().deadline = phase_end;
            return;
        };
        let state = self.dhcp.as_mut().unwrap();
        let client = self.config.ip;
        let packet = dhcp::request(state.xid, self.mac, state.offer, client, broadcast);
        state.deadline = (now + (1000 << state.attempt.min(5))).min(phase_end);
        state.attempt = state.attempt.saturating_add(1);
        if self.lease.is_some() {
            self.ethernet_status = if broadcast {
                "DHCP: rebinding lease"
            } else {
                "DHCP: renewing lease"
            };
        }
        let mut frame = [0; 342];
        frame[..6].copy_from_slice(&mac);
        frame[6..12].copy_from_slice(&self.mac);
        w16(&mut frame, 12, 0x0800);
        frame[14] = 0x45;
        w16(&mut frame, 16, 328);
        frame[22] = 64;
        frame[23] = 17;
        frame[26..30].copy_from_slice(&client);
        frame[30..34].copy_from_slice(&destination);
        let checksum = fold(sum(&frame[14..34]));
        w16(&mut frame, 24, checksum);
        w16(&mut frame, 34, 68);
        w16(&mut frame, 36, 67);
        w16(&mut frame, 38, 308);
        frame[42..].copy_from_slice(&packet);
        self.transmit(&frame);
    }
    fn dhcp_reply(&mut self, packet: &[u8]) {
        let Some(state) = &mut self.dhcp else { return };
        let Some((kind, mut offer)) = dhcp::reply(packet, state.xid, self.mac) else {
            return;
        };
        let now = millis();
        if !dhcp::unicast(offer.server) {
            return;
        }
        if kind == 2 && state.offer.is_none() && self.lease.is_none() && offer.config.valid() {
            state.offer = Some(offer);
            state.attempt = 0;
            state.deadline = now;
            return;
        }
        let previous = if let Some(lease) = &self.lease {
            if now < lease.rebinding && lease.server != offer.server {
                return;
            }
            self.config
        } else if let Some(selected) = state
            .offer
            .filter(|selected| selected.server == offer.server)
        {
            selected.config
        } else {
            return;
        };
        if kind == 6 {
            if self.state != 0 {
                self.fail("DHCP server revoked the IPv4 lease");
            }
            self.start_dhcp(self.vlan);
            if let Some(next) = &mut self.dhcp {
                next.deadline = now + 1000;
            }
        } else if kind == 5 && offer.seconds != 0 && offer.config.ip == previous.ip {
            if offer.config.mask == [0; 4] {
                offer.config.mask = previous.mask;
            }
            if !offer.has_router {
                offer.config.router = previous.router;
            }
            if !offer.has_dns {
                offer.config.dns = previous.dns;
            }
            if !offer.config.valid() {
                return;
            }
            if self.config.mask != offer.config.mask
                || self.config.router != offer.config.router
                || self.config.dns != offer.config.dns
            {
                self.neighbors.fill(None);
                if self.config.ip != [0; 4] && self.state != 0 {
                    self.fail("Ethernet configuration changed during DHCP renewal");
                }
            }
            self.config = offer.config;
            let (renewal, rebinding, expires) = offer.deadlines(now);
            self.lease = Some(Lease {
                server: offer.server,
                renewal,
                rebinding,
                expires,
            });
            self.ethernet_status = "DHCP lease active";
            self.dhcp = None;
            unsafe { log(b"KreativeOS: Ethernet DHCP lease acquired\r\n") };
        }
    }

    pub fn lease_seconds(&self) -> Option<u64> {
        self.lease
            .as_ref()
            .map(|lease| lease.expires.saturating_sub(millis()).div_ceil(1000))
    }

    fn neighbor(&self, address: Address) -> Option<[u8; 6]> {
        let now = millis();
        self.neighbors
            .iter()
            .flatten()
            .find_map(|&(ip, mac, expires)| (ip == address && now < expires).then_some(mac))
    }

    fn remember_neighbor(&mut self, address: Address, mac: [u8; 6]) {
        let index = self
            .neighbors
            .iter()
            .position(|entry| entry.is_some_and(|(ip, _, _)| ip == address))
            .unwrap_or_else(|| {
                let index = self.neighbor_slot;
                self.neighbor_slot = (index + 1) % self.neighbors.len();
                index
            });
        self.neighbors[index] = Some((address, mac, millis() + 60_000));
    }
    fn transmit(&mut self, frame: &[u8]) {
        let mut tagged = [0; 1536];
        let frame = if self.vlan != 0 {
            if !(14..=1532).contains(&frame.len()) {
                return;
            }
            tagged[..12].copy_from_slice(&frame[..12]);
            w16(&mut tagged, 12, 0x8100);
            w16(&mut tagged, 14, self.vlan);
            tagged[16..frame.len() + 4].copy_from_slice(&frame[12..]);
            &tagged[..frame.len() + 4]
        } else {
            frame
        };
        if !self.online() || frame.len() > 1536 {
            return;
        }
        if let Some(adapter) = &mut self.intel {
            if adapter.transmit(frame) {
                self.sent += 1;
                self.sent_bytes += frame.len() as u64;
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
            self.sent_bytes += frame.len() as u64;
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
        p[28..32].copy_from_slice(&self.config.ip);
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
        f[26..30].copy_from_slice(&self.config.ip);
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
        let random = entropy();
        self.dns_id = random as u16;
        self.dns_port = 49152 + (((random >> 16) as u16) % 16384);
        w16(&mut p, 0, self.dns_port);
        w16(&mut p, 2, 53);
        w16(&mut p, 8, self.dns_id);
        w16(&mut p, 10, 0x0100);
        w16(&mut p, 12, 1);
        let mut len = 20;
        for label in self.dns_name.str().split('.') {
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
        let checksum =
            fold(sum(&self.config.ip) + sum(&self.config.dns) + 17 + len as u32 + sum(&p[..len]));
        w16(&mut p, 6, if checksum == 0 { u16::MAX } else { checksum });
        self.state = 2;
        self.status = "Resolving hostname";
        self.ip_send(self.config.dns, 17, &p[..len], true)
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
        let c = fold(sum(&self.config.ip) + sum(&self.remote) + 6 + len as u32 + sum(&p[..len]));
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
        self.dns_name.set(host.trim_end_matches('.'));
        self.dns_aliases = 0;
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
        self.port = 49152 + (entropy() % 16384) as u16;
        if self.config.ip == [0; 4] {
            self.fail("Ethernet has no active IPv4 lease");
            return;
        }
        self.resolve_hop(if self.remote == [0; 4] {
            self.config.dns
        } else {
            self.remote
        });
    }
    fn resolve_hop(&mut self, destination: Address) {
        let Some(hop) = self.config.route(destination) else {
            self.fail("No IPv4 route or DNS server configured");
            return;
        };
        self.next_hop = hop;
        if let Some(mac) = self.neighbor(hop) {
            self.gateway = mac;
            self.resolved_hop();
            return;
        }
        self.state = 1;
        self.status = "Resolving Ethernet address";
        self.retries = 0;
        self.deadline = millis() + 1000;
        self.arp(false, [0; 6], self.next_hop)
    }
    fn resolved_hop(&mut self) {
        if self.remote == [0; 4] {
            self.dns()
        } else {
            self.connect()
        }
    }
    pub fn poll(&mut self) {
        if !self.online() {
            return;
        }
        let link_up = self.link_speed().is_some();
        if self.link_up && !link_up {
            self.neighbors.fill(None);
            if self.state != 0 && !(self.secure && self.wifi.joined) {
                self.fail("Ethernet link disconnected");
            }
        } else if !self.link_up && link_up && self.lease.is_none() {
            if let Some(discovery) = &mut self.dhcp {
                discovery.deadline = millis();
            }
        }
        self.link_up = link_up;
        if link_up {
            self.dhcp_tick();
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
                    if !(4..=1522).contains(&len) {
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
            self.received_bytes += len as u64;
            self.frame(&f[..len]);
        }
        if self.state != 0 && (self.state != 6 || self.raw_waiting) && millis() > self.deadline {
            if self.retries >= 3 {
                self.fail("Network request timed out")
            } else {
                self.retries += 1;
                self.deadline = millis() + 1500;
                if self.state == 1 {
                    self.arp(false, [0; 6], self.next_hop)
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
        if f.len() < 14 || (f[..6] != self.mac && f[..6] != [255; 6]) {
            return;
        }
        let mut untagged = [0; 1536];
        let f = if self.vlan != 0 {
            if f.len() < 18 || u16be(f, 12) != 0x8100 || u16be(f, 14) & 0x0fff != self.vlan {
                return;
            }
            untagged[..12].copy_from_slice(&f[..12]);
            untagged[12..f.len() - 4].copy_from_slice(&f[16..]);
            &untagged[..f.len() - 4]
        } else {
            f
        };
        match u16be(f, 12) {
            0x0806 => {
                if self.config.ip == [0; 4]
                    || f.len() < 42
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
                let operation = u16be(f, 20);
                if f[38..42] != self.config.ip
                    || f[6..12] != mac
                    || mac == [0; 6]
                    || mac[0] & 1 != 0
                {
                    return;
                }
                if operation == 1 {
                    self.arp(true, mac, addr);
                } else if operation == 2 && f[32..38] == self.mac && dhcp::unicast(addr) {
                    let lease_hop = self
                        .lease
                        .as_ref()
                        .and_then(|lease| self.config.route(lease.server));
                    if (self.state == 1 && addr == self.next_hop)
                        || (self.dhcp.is_some() && lease_hop == Some(addr))
                    {
                        self.remember_neighbor(addr, mac);
                        if self.state == 1 && addr == self.next_hop {
                            self.gateway = mac;
                            self.resolved_hop();
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
            || u16be(p, 6) & 0x3fff != 0
            || fold(sum(&p[..h])) != 0
        {
            return;
        }
        let d = &p[h..total];
        if p[9] == 17 && d.len() >= 8 && u16be(d, 0) == 67 && u16be(d, 2) == 68 {
            if let Some(bootp) = udp_payload(&p[12..20], d) {
                self.dhcp_reply(bootp);
            }
            return;
        }
        if self.config.ip == [0; 4] || p[16..20] != self.config.ip {
            return;
        }
        if p[9] == 17
            && self.state == 2
            && p[12..16] == self.config.dns
            && d.len() >= 8
            && u16be(d, 0) == 53
            && u16be(d, 2) == self.dns_port
        {
            use crate::dns::Answer;
            let Some(answer) = udp_payload(&p[12..20], d)
                .and_then(|q| crate::dns::response(q, self.dns_id, self.dns_name.str().as_bytes()))
            else {
                return;
            };
            match answer {
                Answer::Address(ip) => {
                    self.remote = ip;
                    self.resolve_hop(ip);
                }
                Answer::Alias(target, len) if self.dns_aliases < 8 => {
                    self.dns_aliases += 1;
                    self.dns_name
                        .set(core::str::from_utf8(&target[..len]).unwrap_or_default());
                    self.dns();
                }
                Answer::Alias(..) => self.fail("DNS alias limit exceeded"),
                Answer::Missing => self.fail("DNS name has no IPv4 address"),
                Answer::ServerError => self.fail("DNS server returned an error"),
                Answer::Truncated => self.fail("DNS reply was truncated"),
            }
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
fn udp_payload<'a>(addresses: &[u8], packet: &'a [u8]) -> Option<&'a [u8]> {
    let declared = u16be(packet, 4) as usize;
    let udp = packet.get(..declared)?;
    let payload = udp.get(8..)?;
    if u16be(udp, 6) != 0 && fold(sum(addresses) + 17 + declared as u32 + sum(udp)) != 0 {
        return None;
    }
    Some(payload)
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
