use crate::{
    desktop::{button, clip_label, INK, MUTED, SURFACE},
    graphics::Screen,
    net::Network,
    storage,
    text::Text,
};
use alloc::vec::Vec;
use core::fmt::Write;

pub struct Center {
    tab: usize,
    query: Text<48>,
    offset: usize,
    devices: Vec<Text<80>>,
    notice: &'static str,
    cpu: Text<48>,
}

const TABS: [&str; 3] = ["Overview", "PCI inventory", "Storage"];

fn processor() -> Text<48> {
    let cpuid = core::arch::x86_64::__cpuid;
    let mut name = Text::new();
    if cpuid(0x8000_0000).eax >= 0x8000_0004 {
        let mut brand = [0u8; 48];
        for (i, leaf) in (0x8000_0002..=0x8000_0004).enumerate() {
            let leaf = cpuid(leaf);
            for (j, word) in [leaf.eax, leaf.ebx, leaf.ecx, leaf.edx].iter().enumerate() {
                let index = i * 16 + j * 4;
                brand[index..index + 4].copy_from_slice(&word.to_le_bytes());
            }
        }
        if let Ok(label) = core::str::from_utf8(&brand) {
            name.set(label.trim_matches(['\0', ' ']));
        }
    }
    if name.len == 0 {
        name.set("x86-64 processor");
    }
    name
}

impl Center {
    pub fn new() -> Self {
        Self {
            tab: 0,
            query: Text::new(),
            offset: 0,
            devices: Vec::new(),
            notice: "",
            cpu: processor(),
        }
    }
    pub fn refresh(&mut self) {
        self.devices = crate::devices::inventory();
        self.offset = 0;
        self.notice = "PCI inventory refreshed";
    }
    fn matches(&self, name: &str) -> bool {
        self.query.len == 0
            || name
                .as_bytes()
                .windows(self.query.len)
                .any(|part| part.eq_ignore_ascii_case(self.query.str().as_bytes()))
    }
    pub fn scroll(&mut self, direction: i32, rows: usize) {
        if self.tab != 1 {
            return;
        }
        let limit = self
            .devices
            .iter()
            .filter(|device| self.matches(device.str()))
            .count()
            .saturating_sub(rows);
        self.offset = self
            .offset
            .saturating_add_signed(direction as isize)
            .min(limit);
    }
    pub fn key(&mut self, key: u8, height: i32) {
        match key {
            0x80 => self.scroll(-1, listing_rows(height)),
            0x81 => self.scroll(1, listing_rows(height)),
            0x82 => self.tab = (self.tab + 2) % 3,
            0x83 => self.tab = (self.tab + 1) % 3,
            0x84 if self.tab == 1 => {
                self.query.clear();
                self.offset = 0;
            }
            8 if self.tab == 1 => {
                self.query.pop();
                self.offset = 0;
            }
            32..=126 if self.tab == 1 => {
                self.query.push(key);
                self.offset = 0;
            }
            _ => {}
        }
    }
    fn save_report(&mut self, network: &Network) {
        let mut report = Text::<2048>::new();
        let _ = writeln!(
            report,
            "KreativeOS Hardware Center\nCPU: {}\nStorage: {}",
            self.cpu.str(),
            storage::backend()
        );
        if let Some(((bus, slot, function), port, sectors, model)) = storage::sata_details() {
            let _ = writeln!(
                report,
                "SATA: {} @ {bus:02x}:{slot:02x}.{function} / port {port} / {sectors} sectors",
                model.str()
            );
        }
        let _ = writeln!(
            report,
            "Ethernet: {}\nPCI functions: {}\n",
            network.adapter_name(),
            self.devices.len()
        );
        for device in &self.devices {
            if 2048 - report.len < device.len + 1 + 24 {
                report.append("More devices in PCI view\n");
                break;
            }
            let _ = writeln!(report, "{}", device.str());
        }
        self.notice = match storage::put("Hardware.txt", report.str().as_bytes()) {
            Ok(()) if storage::sync() => "Saved Hardware.txt to disk",
            Ok(()) => "Hardware.txt is available in memory",
            Err(error) => error,
        };
    }
    pub fn click(&mut self, x: i32, y: i32, w: i32, h: i32, network: &Network) {
        let step = tab_width(w);
        if (20..20 + 3 * step).contains(&x) && (112..146).contains(&y) {
            self.tab = ((x - 20) / step) as usize;
        } else if (h - 47..h - 13).contains(&y) {
            if (20..118).contains(&x) {
                self.refresh();
            } else if (130..250).contains(&x) {
                self.save_report(network);
            } else if w >= 365 && (w - 105..w - 63).contains(&x) {
                self.scroll(-(listing_rows(h) as i32), listing_rows(h));
            } else if w >= 365 && (w - 60..w - 18).contains(&x) {
                self.scroll(listing_rows(h) as i32, listing_rows(h));
            }
        }
    }
    pub fn draw(
        &self,
        s: &mut Screen,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        network: &Network,
        accent: u32,
    ) {
        s.rect(x + 1, y + 38, w - 2, 65, 0x203349);
        s.symbol(x + 20, y + 57, 14, 0x8de4cc);
        s.text(x + 51, y + 50, "Hardware Center", 0xeaf6fa, 2);
        s.text(
            x + 52,
            y + 79,
            "Device inventory and driver status",
            0xacceda,
            1,
        );
        let step = tab_width(w);
        for (i, title) in TABS.iter().enumerate() {
            button(
                s,
                x + 20 + i as i32 * step,
                y + 112,
                step - 6,
                clip_label(title, step - 14, 1),
                if i == self.tab { accent } else { SURFACE },
            );
        }
        let mut value = Text::<100>::new();
        match self.tab {
            0 => {
                card(s, x + 20, y + 156, w - 40, "Processor", self.cpu.str());
                card(
                    s,
                    x + 20,
                    y + 203,
                    w - 40,
                    "Data volume",
                    storage::backend(),
                );
                let _ = write!(value, "{}", network.adapter_name());
                if let Some(speed) = network.link_speed() {
                    let _ = write!(value, "   /   {speed} Mbps");
                } else {
                    value.append("   /   No link");
                }
                card(s, x + 20, y + 250, w - 40, "Ethernet", value.str());
            }
            1 => {
                let count = self
                    .devices
                    .iter()
                    .filter(|device| self.matches(device.str()))
                    .count();
                let label = if self.query.len == 0 {
                    "Type to filter by PCI ID, address or class"
                } else {
                    self.query.str()
                };
                s.rounded(x + 20, y + 158, w - 40, 31, 6, SURFACE, 255);
                s.text(x + 30, y + 165, clip_label(label, w - 60, 1), INK, 1);
                let _ = write!(value, "{count} matching PCI functions");
                s.text(x + 20, y + 197, value.str(), MUTED, 1);
                for (i, device) in self
                    .devices
                    .iter()
                    .filter(|device| self.matches(device.str()))
                    .skip(self.offset)
                    .take(listing_rows(h))
                    .enumerate()
                {
                    let yy = y + 222 + i as i32 * 25;
                    s.rounded(x + 20, yy, w - 40, 23, 4, SURFACE, 255);
                    s.text(x + 28, yy + 3, clip_label(device.str(), w - 56, 1), INK, 1);
                }
            }
            _ => {
                card(s, x + 20, y + 156, w - 40, "Transport", storage::backend());
                let _ = write!(
                    value,
                    "{} of {} file slots in use",
                    storage::used_slots(),
                    storage::COUNT
                );
                card(s, x + 20, y + 203, w - 40, "KFS volume", value.str());
                if let Some(((bus, slot, function), port, sectors, model)) = storage::sata_details()
                {
                    value.clear();
                    let _ = write!(
                        value,
                        "{bus:02x}:{slot:02x}.{function}   /   Port {port}   /   {} MiB",
                        sectors / 2048
                    );
                    card(s, x + 20, y + 250, w - 40, model.str(), value.str());
                }
            }
        }
        s.rect(x + 1, y + h - 68, w - 2, 67, 0xeaf0ee);
        s.text(
            x + 20,
            y + h - 66,
            clip_label(self.notice, w - 40, 1),
            MUTED,
            1,
        );
        button(s, x + 20, y + h - 47, 98, "Rescan", SURFACE);
        button(s, x + 130, y + h - 47, 120, "Save report", accent);
        if self.tab == 1 && w >= 365 {
            button(s, x + w - 105, y + h - 47, 42, "<", SURFACE);
            button(s, x + w - 60, y + h - 47, 42, ">", SURFACE);
        }
    }
}

fn tab_width(w: i32) -> i32 {
    ((w - 32) / 3).clamp(55, 130)
}
pub fn listing_rows(h: i32) -> usize {
    ((h - 290) / 25).max(1) as usize
}

fn card(s: &mut Screen, x: i32, y: i32, w: i32, label: &str, value: &str) {
    s.rounded(x, y, w, 43, 7, SURFACE, 255);
    s.text(x + 12, y + 3, clip_label(label, w - 24, 1), MUTED, 1);
    s.text(x + 12, y + 21, clip_label(value, w - 24, 1), INK, 1);
}
