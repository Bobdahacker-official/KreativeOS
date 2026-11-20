use crate::{graphics::Screen, hardware, storage, text::Text};
use alloc::{string::String, vec::Vec};
use core::fmt::Write;

pub struct Organizer {
    month: i32,
    year: i32,
    day: u8,
    contacts: bool,
    selected: usize,
    field: Text<100>,
    status: &'static str,
}

fn month_days(year: i32, month: i32) -> i32 {
    [
        31,
        if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) {
            29
        } else {
            28
        },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ][month as usize - 1]
}

impl Organizer {
    pub fn new() -> Self {
        let (mut year, mut month, mut day) = (1970, 1, 1);
        if let Some(time) = hardware::unix_time() {
            let mut days = (time / 86400) as i32;
            while days >= 337 + month_days(year, 2) {
                days -= 337 + month_days(year, 2);
                year += 1;
            }
            while days >= month_days(year, month) {
                days -= month_days(year, month);
                month += 1;
            }
            day = days + 1;
        }
        let mut organizer = Self {
            month,
            year,
            day: day as u8,
            contacts: false,
            selected: 0,
            field: Text::new(),
            status: "",
        };
        organizer.load();
        organizer
    }

    fn name(&self) -> &'static str {
        if self.contacts {
            "Contacts.txt"
        } else {
            "Calendar.txt"
        }
    }
    fn records(&self) -> Vec<String> {
        storage::find(self.name())
            .map(|index| {
                let data = unsafe { storage::FILES[index].data.clone() };
                data.str().lines().map(String::from).collect()
            })
            .unwrap_or_default()
    }
    fn date(&self) -> Text<10> {
        let mut date = Text::new();
        let _ = write!(date, "{:04}-{:02}-{:02}", self.year, self.month, self.day);
        date
    }
    fn load(&mut self) {
        let records = self.records();
        self.field.clear();
        if self.contacts {
            if let Some(contact) = records.get(self.selected) {
                self.field.set(contact);
            }
        } else {
            let date = self.date();
            if let Some(event) = records
                .iter()
                .filter_map(|record| record.split_once(' '))
                .find(|(key, _)| *key == date.str())
            {
                self.field.set(event.1);
            }
        }
        self.status = "";
    }
    fn save(&mut self) {
        let mut rows = self.records();
        let value = self.field.str().trim();
        if self.contacts {
            if !value.is_empty()
                && !value.split_once('|').is_some_and(|(name, detail)| {
                    !name.trim().is_empty() && !detail.trim().is_empty()
                })
            {
                self.status = "Use Name | phone or email";
                return;
            }
            if self.selected < rows.len() {
                if value.is_empty() {
                    rows.remove(self.selected);
                } else {
                    rows[self.selected] = String::from(value);
                }
            } else if !value.is_empty() {
                rows.push(String::from(value));
            }
        } else {
            let date = self.date();
            rows.retain(|row| row.split_once(' ').is_none_or(|(key, _)| key != date.str()));
            if !value.is_empty() {
                rows.push(alloc::format!("{} {}", date.str(), value));
            }
            rows.sort_unstable();
        }
        self.status = match storage::put(self.name(), rows.join("\n").as_bytes()) {
            Ok(()) if storage::sync() => "Saved",
            Ok(()) => "Saved in memory",
            Err(error) => error,
        };
    }
    fn step_month(&mut self, offset: i32) {
        let next = (self.year * 12 + self.month - 1 + offset).clamp(1970 * 12, 2099 * 12 + 11);
        self.year = next / 12;
        self.month = next % 12 + 1;
        self.day = self.day.min(month_days(self.year, self.month) as u8);
        self.load();
    }
    fn first_weekday(&self) -> i32 {
        let mut days = 4;
        for year in 1970..self.year {
            days += 337 + month_days(year, 2);
        }
        for month in 1..self.month {
            days += month_days(self.year, month);
        }
        days % 7
    }
    pub fn key(&mut self, key: u8) {
        match key {
            b'\n' => self.save(),
            8 => self.field.pop(),
            0x82 if !self.contacts => self.step_month(-1),
            0x83 if !self.contacts => self.step_month(1),
            0x80 if self.contacts => {
                self.selected = self.selected.saturating_sub(1);
                self.load();
            }
            0x81 if self.contacts => {
                self.selected = (self.selected + 1).min(self.records().len());
                self.load();
            }
            32..=126 => self.field.push(key),
            _ => {}
        }
    }
    pub fn click(&mut self, x: i32, y: i32) {
        match (x, y) {
            (18..=138, 48..=80) => {
                self.contacts = false;
                self.load();
            }
            (148..=268, 48..=80) => {
                self.contacts = true;
                self.load();
            }
            (18..=95, 94..=124) if !self.contacts => self.step_month(-1),
            (269..=346, 94..=124) if !self.contacts => self.step_month(1),
            (18..=374, 149..=328) if !self.contacts => {
                let day = (y - 149) / 30 * 7 + (x - 18) / 51 - self.first_weekday() + 1;
                if (1..=month_days(self.year, self.month)).contains(&day) {
                    self.day = day as u8;
                    self.load();
                }
            }
            (18..=128, 94..=124) if self.contacts => {
                self.selected = self.records().len();
                self.load();
            }
            (18..=374, 139..=329) if self.contacts => {
                let index = self.selected.saturating_sub(6) + ((y - 139) / 27) as usize;
                if index < self.records().len() {
                    self.selected = index;
                    self.load();
                }
            }
            (18..=128, 404..=438) => self.save(),
            _ => {}
        }
    }
    pub fn draw(&self, s: &mut Screen, x: i32, y: i32, w: i32, accent: u32) {
        let button = |s: &mut Screen, left, top, width, label: &str, active| {
            s.rounded(
                x + left,
                y + top,
                width,
                30,
                5,
                if active { 0xc4dfcf } else { 0xe5ebe0 },
                255,
            );
            s.centered(x + left, y + top, width, 30, label, 0x20363a, 1);
        };
        button(s, 18, 48, 120, "Calendar", !self.contacts);
        button(s, 148, 48, 120, "Contacts", self.contacts);
        if self.contacts {
            button(s, 18, 94, 110, "New contact", false);
            for (row, record) in self
                .records()
                .iter()
                .skip(self.selected.saturating_sub(6))
                .take(7)
                .enumerate()
            {
                let name = record
                    .split_once('|')
                    .map_or(record.as_str(), |(name, _)| name);
                let clipped = crate::desktop::clip_label(name, (w - 52).min(356), 1);
                button(
                    s,
                    18,
                    139 + row as i32 * 27,
                    (w - 36).min(356),
                    clipped,
                    self.selected == row + self.selected.saturating_sub(6),
                );
            }
            s.text(x + 18, y + 342, "Name | phone or email", accent, 1);
        } else {
            let months = [
                "January",
                "February",
                "March",
                "April",
                "May",
                "June",
                "July",
                "August",
                "September",
                "October",
                "November",
                "December",
            ];
            let title = alloc::format!("{} {}", months[self.month as usize - 1], self.year);
            button(s, 18, 94, 77, "<", false);
            button(s, 269, 94, 77, ">", false);
            s.centered(x + 98, y + 94, 168, 30, &title, accent, 1);
            for (index, weekday) in ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"]
                .iter()
                .enumerate()
            {
                s.centered(
                    x + 18 + index as i32 * 51,
                    y + 124,
                    46,
                    25,
                    weekday,
                    0x20363a,
                    1,
                );
            }
            for day in 1..=month_days(self.year, self.month) {
                let cell = self.first_weekday() + day - 1;
                button(
                    s,
                    18 + cell % 7 * 51,
                    149 + cell / 7 * 30,
                    46,
                    &alloc::format!("{day}"),
                    day == i32::from(self.day),
                );
            }
            let label = alloc::format!("Event on {}", self.date().str());
            s.text(x + 18, y + 342, &label, accent, 1);
        }
        s.rounded(x + 18, y + 365, w - 36, 32, 5, 0xffffff, 255);
        s.text(
            x + 26,
            y + 372,
            crate::desktop::clip_label(self.field.str(), w - 53, 1),
            0x20363a,
            1,
        );
        button(s, 18, 404, 110, "Save", true);
        s.text(x + 141, y + 410, self.status, accent, 1);
    }
}
