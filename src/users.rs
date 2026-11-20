//! Local accounts and the restricted console for native user programs.
use crate::{graphics::Screen, hardware, ring3, storage, text::Text};
use alloc::vec::Vec;
use core::fmt::Write;
use sha2::{Digest, Sha256};

const DATABASE: &str = "Accounts.cfg";
const MAGIC: &[u8; 4] = b"KUA1";
const LIMIT: usize = 12;
const RECORD: usize = 64;
const ROUNDS: usize = 100_000;

#[derive(Clone, Copy)]
struct User {
    name: Text<16>,
    salt: [u8; 16],
    key: [u8; 32],
}

fn valid_name(name: &str) -> bool {
    (1..=16).contains(&name.len())
        && name.as_bytes()[0].is_ascii_lowercase()
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

fn decode(bytes: &[u8]) -> Option<Vec<User>> {
    let rows = bytes.strip_prefix(MAGIC)?;
    if rows.is_empty() || rows.len() % RECORD != 0 || rows.len() / RECORD > LIMIT {
        return None;
    }
    let mut accounts: Vec<User> = Vec::new();
    for row in rows.chunks_exact(RECORD) {
        let end = row[..16].iter().position(|&b| b == 0).unwrap_or(16);
        let name = core::str::from_utf8(&row[..end]).ok()?;
        if !valid_name(name) || accounts.iter().any(|entry| entry.name.str() == name) {
            return None;
        }
        let mut label = Text::new();
        label.set(name);
        accounts.push(User {
            name: label,
            salt: row[16..32].try_into().ok()?,
            key: row[32..64].try_into().ok()?,
        });
    }
    (accounts[0].name.str() == "admin").then_some(accounts)
}

fn save(accounts: &[User]) -> bool {
    let previous = storage::find(DATABASE).map(|i| unsafe { storage::FILES[i].data });
    let mut bytes = Vec::with_capacity(4 + accounts.len() * RECORD);
    bytes.extend_from_slice(MAGIC);
    for account in accounts {
        let mut label = [0; 16];
        label[..account.name.len].copy_from_slice(account.name.str().as_bytes());
        bytes.extend_from_slice(&label);
        bytes.extend_from_slice(&account.salt);
        bytes.extend_from_slice(&account.key);
    }
    if storage::put(DATABASE, &bytes).is_err() {
        return false;
    }
    if storage::sync() {
        return true;
    }
    if let Some(previous) = previous {
        let _ = storage::put(DATABASE, &previous.bytes[..previous.len]);
    } else {
        storage::remove(DATABASE);
    }
    false
}

fn salt() -> Option<[u8; 16]> {
    if core::arch::x86_64::__cpuid(1).ecx & (1 << 30) == 0 {
        return None;
    }
    let mut salt = [0; 16];
    for part in salt.chunks_exact_mut(8) {
        let mut value = 0;
        if !(0..16).any(|_| unsafe { core::arch::x86_64::_rdrand64_step(&mut value) == 1 }) {
            return None;
        }
        part.copy_from_slice(&value.to_le_bytes());
    }
    Some(salt)
}

fn password_key(password: &[u8], salt: &[u8; 16]) -> [u8; 32] {
    let mut inner = [0x36; 64];
    let mut outer = [0x5c; 64];
    for (i, &byte) in password.iter().enumerate() {
        inner[i] ^= byte;
        outer[i] ^= byte;
    }
    let inner = Sha256::new_with_prefix(inner);
    let outer = Sha256::new_with_prefix(outer);
    let hmac = |message: &[u8]| {
        let mut h = inner.clone();
        h.update(message);
        let mut result = outer.clone();
        result.update(h.finalize());
        <[u8; 32]>::from(result.finalize())
    };
    let mut block = [0; 20];
    block[..16].copy_from_slice(salt);
    block[19] = 1;
    let mut part = hmac(&block);
    let mut key = part;
    for _ in 1..ROUNDS {
        part = hmac(&part);
        for (byte, next) in key.iter_mut().zip(part) {
            *byte ^= next;
        }
    }
    key
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum View {
    Setup,
    Login,
    Add,
    Console,
    Password,
}

pub struct Session {
    users: Vec<User>,
    view: Option<View>,
    invalid_database: bool,
    entered_name: Text<16>,
    password: Text<64>,
    confirm: Text<64>,
    field: usize,
    message: &'static str,
    blocked_until: u64,
    current: usize,
    command: Text<80>,
    output: Text<3072>,
}

impl Session {
    pub fn new() -> Self {
        let snapshot = storage::find(DATABASE).map(|i| unsafe { storage::FILES[i].data });
        let users = snapshot.and_then(|data| decode(&data.bytes[..data.len]));
        Self {
            view: snapshot.is_some().then_some(View::Login),
            invalid_database: snapshot.is_some() && users.is_none(),
            users: users.unwrap_or_default(),
            entered_name: Text::new(),
            password: Text::new(),
            confirm: Text::new(),
            field: 0,
            message: "",
            blocked_until: 0,
            current: 0,
            command: Text::new(),
            output: Text::new(),
        }
    }

    pub fn visible(&self) -> bool {
        self.view.is_some()
    }

    fn clear_passwords(&mut self) {
        for bytes in [&mut self.password, &mut self.confirm] {
            for byte in &mut bytes.bytes {
                unsafe { core::ptr::write_volatile(byte, 0) };
            }
            bytes.clear();
        }
    }

    fn show(&mut self, view: View) {
        self.clear_passwords();
        self.entered_name.clear();
        self.command.clear();
        self.field = if matches!(view, View::Setup | View::Password) {
            1
        } else {
            0
        };
        self.message = "";
        self.view = Some(view);
    }

    pub fn manage(&mut self, arg: &str, out: &mut impl Write) {
        if self.invalid_database {
            let _ = writeln!(out, "Account database could not be read");
            return;
        }
        if self.users.is_empty() {
            self.show(View::Setup);
        } else if arg == "add" {
            self.show(View::Add);
        } else if arg == "password" {
            self.current = 0;
            self.show(View::Password);
        } else if let Some(name) = arg.strip_prefix("remove ") {
            if let Some(index) = self
                .users
                .iter()
                .position(|user| user.name.str() == name)
                .filter(|&i| i != 0)
            {
                let user = self.users.remove(index);
                if save(&self.users) {
                    let _ = writeln!(out, "Removed {name}");
                } else {
                    self.users.insert(index, user);
                    let _ = writeln!(out, "Could not save account changes");
                }
            } else {
                let _ = writeln!(out, "No removable account named {name}");
            }
        } else {
            let _ = writeln!(
                out,
                "accounts add | accounts remove NAME | accounts password | lock"
            );
            for user in &self.users {
                let _ = writeln!(out, "{}", user.name.str());
            }
        }
    }

    pub fn lock(&mut self) -> bool {
        if self.users.is_empty() {
            return false;
        }
        self.show(View::Login);
        self.output.clear();
        true
    }

    fn submit(&mut self, view: View) {
        if view == View::Login {
            if self.invalid_database {
                self.message = "Account database is damaged";
                return;
            }
            if hardware::millis() < self.blocked_until {
                self.message = "Wait before trying again";
                return;
            }
            let account = self
                .users
                .iter()
                .position(|user| user.name.str() == self.entered_name.str());
            let user = self.users[account.unwrap_or(0)];
            let key = password_key(self.password.str().as_bytes(), &user.salt);
            let difference = key
                .iter()
                .zip(user.key)
                .fold(0, |diff, (&a, b)| diff | (a ^ b));
            self.clear_passwords();
            if let Some(index) = account.filter(|_| difference == 0) {
                self.current = index;
                if index == 0 {
                    self.view = None;
                } else {
                    self.show(View::Console);
                    self.output
                        .set("Use ls, exec NAME.kr3, passwd or logout.\n");
                }
            } else {
                self.blocked_until = hardware::millis() + 1500;
                self.message = "Account name or password is incorrect";
                self.field = 1;
            }
            return;
        }
        if self.password.len < 12 || self.password.str() != self.confirm.str() {
            self.message = "Use a password of at least 12 characters and confirm it";
            self.clear_passwords();
            self.field = 1;
            return;
        }
        let name = if view == View::Setup {
            "admin"
        } else if view == View::Password {
            self.users[self.current].name.str()
        } else {
            self.entered_name.str()
        };
        if !valid_name(name)
            || (view == View::Add
                && (self.users.len() == LIMIT
                    || self.users.iter().any(|user| user.name.str() == name)))
        {
            self.message = "Use a new lowercase username. Up to 12 accounts are supported";
            self.field = 0;
            return;
        }
        let Some(salt) = salt() else {
            self.message = "Hardware entropy is unavailable";
            return;
        };
        let mut label = Text::new();
        label.set(name);
        let key = password_key(self.password.str().as_bytes(), &salt);
        self.clear_passwords();
        let user = User {
            name: label,
            salt,
            key,
        };
        let previous = if view == View::Password {
            Some(core::mem::replace(&mut self.users[self.current], user))
        } else {
            self.users.push(user);
            None
        };
        if !save(&self.users) {
            if let Some(old) = previous {
                self.users[self.current] = old;
            } else {
                self.users.pop();
            }
            self.message = "Account changes require writable KFS storage and a free file slot";
            self.field = 1;
        } else if view == View::Password && self.current != 0 {
            self.show(View::Console);
            let _ = writeln!(self.output, "Password updated");
        } else {
            self.view = None;
        }
    }

    fn console(&mut self) {
        let input = self.command;
        self.command.clear();
        let command = input.str().trim();
        if self.output.len > 1850 {
            self.output.clear();
        }
        let _ = writeln!(self.output, "> {command}");
        if command == "logout" {
            self.lock();
        } else if command == "passwd" {
            self.show(View::Password);
        } else {
            let username = self.users[self.current].name;
            let owned = |name: &str| {
                name.ends_with(".kr3")
                    && name
                        .strip_prefix(username.str())
                        .is_some_and(|tail| tail.starts_with('.'))
            };
            if command == "ls" {
                for file in unsafe { &storage::FILES } {
                    if file.used && owned(file.name.str()) {
                        let _ = writeln!(self.output, "{}", &file.name.str()[username.len + 1..]);
                    }
                }
            } else if let Some(name) = command.strip_prefix("exec ") {
                let mut path = Text::<80>::new();
                let _ = write!(path, "{}.{name}", username.str());
                if let Some(index) = storage::find(path.str()).filter(|_| owned(path.str())) {
                    let data = unsafe { storage::FILES[index].data };
                    if let Err(error) = ring3::run(&data.bytes[..data.len], &mut self.output) {
                        let _ = writeln!(self.output, "{error}");
                    }
                } else {
                    let _ = writeln!(self.output, "Program not found in this account");
                }
            } else {
                let _ = writeln!(self.output, "Use ls, exec NAME.kr3, passwd or logout");
            }
        }
    }

    pub fn key(&mut self, key: u8) {
        let Some(view) = self.view else { return };
        if view == View::Console {
            match key {
                b'\n' => self.console(),
                8 => self.command.pop(),
                32..=126 => self.command.push(key),
                _ => {}
            }
            return;
        }
        let first = if matches!(view, View::Setup | View::Password) {
            1
        } else {
            0
        };
        let last = if view == View::Login { 1 } else { 2 };
        match key {
            27 if view != View::Login => {
                self.clear_passwords();
                if view == View::Password && self.current != 0 {
                    self.show(View::Console);
                } else {
                    self.view = None;
                }
            }
            0x80 => self.field = self.field.saturating_sub(1).max(first),
            0x81 | 9 => {
                self.field = if self.field == last {
                    first
                } else {
                    self.field + 1
                }
            }
            b'\n' if self.field < last => self.field += 1,
            b'\n' => self.submit(view),
            8 => match self.field {
                0 => self.entered_name.pop(),
                1 => self.password.pop(),
                _ => self.confirm.pop(),
            },
            32..=126 => match self.field {
                0 => self.entered_name.push(key),
                1 => self.password.push(key),
                _ => self.confirm.push(key),
            },
            _ => {}
        }
    }

    pub fn draw(&self, screen: &mut Screen) {
        let Some(view) = self.view else { return };
        let (w, h) = (screen.w as i32, screen.h as i32);
        screen.wallpaper();
        let title = match view {
            View::Setup => "Create administrator",
            View::Login => "Sign in to KreativeOS",
            View::Add => "Create user account",
            View::Console => self.users[self.current].name.str(),
            View::Password => "Change password",
        };
        let width = if view == View::Console { w - 96 } else { 508 };
        let height = if view == View::Console { h - 88 } else { 320 };
        let (x, y) = ((w - width) / 2, (h - height) / 2);
        screen.shadow(x, y, width, height);
        screen.rounded(x, y, width, height, 14, 0x172439, 255);
        screen.text(x + 24, y + 20, title, 0xeaf4fa, 2);
        if view == View::Console {
            let mut lines: Vec<_> = self.output.str().lines().collect();
            let rows = ((height - 100) / 20).max(1) as usize;
            lines = lines.split_off(lines.len().saturating_sub(rows));
            for (row, line) in lines.iter().enumerate() {
                screen.text(
                    x + 24,
                    y + 58 + row as i32 * 20,
                    crate::desktop::clip_label(line, width - 48, 1),
                    0xb7d2e2,
                    1,
                );
            }
            screen.text(x + 24, y + height - 32, ">", 0x85ddc6, 1);
            screen.text(
                x + 47,
                y + height - 32,
                crate::desktop::clip_label(self.command.str(), width - 72, 1),
                0xeaf4fa,
                1,
            );
            return;
        }
        for (i, name) in ["Username", "Password", "Confirm password"]
            .iter()
            .enumerate()
        {
            if (i == 2 && view == View::Login)
                || (i == 0 && matches!(view, View::Setup | View::Password))
            {
                continue;
            }
            let yy = y + 68 + i as i32 * 58;
            screen.text(x + 24, yy, name, 0xb7d2e2, 1);
            screen.rounded(
                x + 24,
                yy + 19,
                width - 48,
                31,
                5,
                if i == self.field { 0x395772 } else { 0x263950 },
                255,
            );
            let mut mask = Text::<64>::new();
            let value = match i {
                0 => self.entered_name.str(),
                _ => {
                    let len = if i == 1 {
                        self.password.len
                    } else {
                        self.confirm.len
                    };
                    for _ in 0..len.min(40) {
                        mask.push(b'*');
                    }
                    mask.str()
                }
            };
            screen.text(x + 34, yy + 24, value, 0xeaf4fa, 1);
        }
        screen.text(
            x + 24,
            y + 258,
            "Tab selects a field  /  Enter submits",
            0x85ddc6,
            1,
        );
        screen.wrapped(x + 24, y + 281, width - 48, 36, self.message, 0xf4c2af);
    }
}
