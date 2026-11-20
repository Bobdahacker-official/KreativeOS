use crate::{
    dhcp::{address, Config},
    graphics::Screen,
    net::Network,
    storage,
    text::Text,
};
use core::fmt::Write;

const PROFILE: &str = "Ethernet.cfg";
const LABELS: [&str; 5] = [
    "IPv4 address",
    "Subnet mask",
    "Gateway",
    "DNS server",
    "VLAN ID",
];

#[derive(Clone, Copy)]
enum Profile {
    Dhcp(u16),
    Static(Config, u16),
}

impl Profile {
    fn read(text: &str) -> Option<Self> {
        let mut tokens = text.split_ascii_whitespace();
        let profile = match tokens.next()? {
            "dhcp" => Self::Dhcp(vlan(tokens.next()?)?),
            "static" => {
                let config = Config {
                    ip: address(tokens.next()?)?,
                    mask: address(tokens.next()?)?,
                    router: address(tokens.next()?)?,
                    dns: address(tokens.next()?)?,
                };
                if !config.valid() {
                    return None;
                }
                Self::Static(config, vlan(tokens.next()?)?)
            }
            _ => return None,
        };
        tokens.next().is_none().then_some(profile)
    }

    fn apply(self, network: &mut Network) -> bool {
        match self {
            Self::Dhcp(tag) => network.start_dhcp(tag),
            Self::Static(config, tag) => network.static_config(config, tag),
        }
    }

    fn saved() -> Option<Self> {
        let file = storage::find(PROFILE)?;
        Self::read(unsafe { storage::FILES[file].data.clone() }.str())
    }
}

fn vlan(text: &str) -> Option<u16> {
    text.parse().ok().filter(|value| *value <= 4094)
}

pub fn configure_at_boot(network: &mut Network) {
    Profile::saved().unwrap_or(Profile::Dhcp(0)).apply(network);
}

pub fn dotted(a: [u8; 4]) -> Text<16> {
    let mut text = Text::new();
    let _ = write!(text, "{}.{}.{}.{}", a[0], a[1], a[2], a[3]);
    text
}

pub struct Editor {
    manual: bool,
    selected: usize,
    fields: [Text<16>; 5],
    status: &'static str,
}

impl Editor {
    pub fn new() -> Self {
        let (manual, config, tag) = match Profile::saved() {
            Some(Profile::Static(config, tag)) => (true, config, tag),
            Some(Profile::Dhcp(tag)) => (false, Config::QEMU, tag),
            None => (false, Config::QEMU, 0),
        };
        let mut tag_field = Text::new();
        let _ = write!(tag_field, "{tag}");
        Self {
            manual,
            selected: if manual { 0 } else { 4 },
            fields: [
                dotted(config.ip),
                dotted(config.mask),
                dotted(config.router),
                dotted(config.dns),
                tag_field,
            ],
            status: "",
        }
    }

    fn mode(&mut self, manual: bool) {
        self.manual = manual;
        self.selected = if manual { 0 } else { 4 };
        self.status = "";
    }

    fn apply(&mut self, network: &mut Network) {
        let mut line = Text::<96>::new();
        if self.manual {
            line.set("static");
            for field in &self.fields {
                let _ = write!(line, " {}", field.str());
                if field.len == 0 {
                    self.status = "Fill all fields. Use 0.0.0.0 for no gateway or DNS.";
                    return;
                }
            }
        } else {
            let _ = write!(line, "dhcp {}", self.fields[4].str());
        }
        let Some(profile) = Profile::read(line.str()) else {
            self.status = "Check IPv4, subnet, gateway and VLAN (0 to 4094).";
            return;
        };
        if !profile.apply(network) {
            self.status = "Ethernet is missing or a network request is still active.";
            return;
        }
        self.status = match storage::put(PROFILE, line.str().as_bytes()) {
            Ok(()) if storage::sync() => "Applied and saved for the next boot.",
            Ok(()) => "Applied. The data disk could not save this profile.",
            Err(_) => "Applied. No KFS file slot is available for this profile.",
        };
    }

    pub fn key(&mut self, key: u8, network: &mut Network) {
        match key {
            b'\n' => self.apply(network),
            0x80 if self.manual => self.selected = (self.selected + 4) % 5,
            0x81 if self.manual => self.selected = (self.selected + 1) % 5,
            0x84 => self.fields[self.selected].clear(),
            8 => {
                self.fields[self.selected].pop();
            }
            b'0'..=b'9' => {
                self.fields[self.selected].push(key);
            }
            b'.' if self.selected < 4 => {
                self.fields[self.selected].push(key);
            }
            _ => {}
        }
    }

    pub fn click(&mut self, x: i32, y: i32, w: i32, network: &mut Network) {
        match (x, y) {
            (24..128, 150..184) => self.mode(false),
            (136..240, 150..184) => self.mode(true),
            (24..172, 325..359) => self.apply(network),
            (x, 198..316) if (24..w - 24).contains(&x) => {
                let field = ((y - 198) / 42 * 2 + i32::from(x >= w / 2)) as usize;
                if field < 5 && (self.manual || field == 4) {
                    self.selected = field;
                }
            }
            _ => {}
        }
    }

    pub fn draw(&self, s: &mut Screen, x: i32, y: i32, w: i32, network: &Network, accent: u32) {
        use crate::desktop::{button, clip_label, INK, MUTED, SURFACE};
        s.rounded(x + 24, y + 88, w - 48, 50, 8, SURFACE, 255);
        s.symbol(x + 36, y + 102, 8, accent);
        let mut link = Text::<96>::new();
        let _ = write!(link, "{}  /  ", network.adapter_name());
        if let Some(speed) = network.link_speed() {
            let _ = write!(link, "{speed} Mbps");
        } else {
            link.append("Link down");
        }
        s.text(x + 67, y + 95, link.str(), INK, 1);
        let mut status = Text::<96>::new();
        status.set(network.ethernet_status);
        if let Some(seconds) = network.lease_seconds() {
            let _ = write!(status, "  /  {seconds}s remaining");
        }
        s.text(
            x + 67,
            y + 114,
            clip_label(status.str(), w - 100, 1),
            MUTED,
            1,
        );
        button(
            s,
            x + 24,
            y + 150,
            104,
            "Automatic",
            if self.manual { SURFACE } else { accent },
        );
        button(
            s,
            x + 136,
            y + 150,
            104,
            "Manual",
            if self.manual { accent } else { SURFACE },
        );
        let column = (w - 48) / 2;
        for (i, label) in LABELS.iter().enumerate() {
            if !self.manual && i != 4 {
                continue;
            }
            let xx = x + 24 + (i as i32 % 2) * column;
            let yy = y + 198 + (i as i32 / 2) * 42;
            s.text(xx, yy, label, MUTED, 1);
            s.rounded(
                xx,
                yy + 16,
                column - 12,
                23,
                4,
                if i == self.selected {
                    0xcde7dc
                } else {
                    SURFACE
                },
                255,
            );
            s.text(xx + 8, yy + 19, self.fields[i].str(), INK, 1);
        }
        if !self.manual {
            let mut values = Text::<112>::new();
            let _ = write!(
                values,
                "IPv4 {}   DNS {}",
                dotted(network.config.ip).str(),
                dotted(network.config.dns).str()
            );
            s.text(x + 24, y + 203, clip_label(values.str(), w - 48, 1), INK, 1);
            values.clear();
            let _ = write!(
                values,
                "Mask {}   Gateway {}",
                dotted(network.config.mask).str(),
                dotted(network.config.router).str()
            );
            s.text(
                x + 24,
                y + 228,
                clip_label(values.str(), w - 48, 1),
                MUTED,
                1,
            );
        }
        button(s, x + 24, y + 325, 148, "Apply and save", accent);
        s.text(
            x + 24,
            y + 369,
            clip_label(self.status, w - 48, 1),
            MUTED,
            1,
        );
    }
}
