//! Local accounts and the graphical session roles.
use crate::{
    desktop::{clip_label, Box2, INK, MUTED},
    graphics::Screen,
    hardware, storage,
    text::Text,
};
use alloc::vec::Vec;
use core::fmt::Write;
use sha2::{Digest, Sha256};

const DATABASE: &str = "Accounts.cfg";
const MAGIC: &[u8; 4] = b"KUA2";
const LEGACY_MAGIC: &[u8; 4] = b"KUA1";
const LIMIT: usize = 12;
const LEGACY_ROUNDS: u32 = 100_000;
const ROUNDS: u32 = 600_000;
pub const SYSTEM_NAME: &str = "system1";

pub fn is_database(name: &str) -> bool {
    name.eq_ignore_ascii_case(DATABASE)
}

#[derive(Clone, Copy)]
struct User {
    name: Text<16>,
    salt: [u8; 16],
    key: [u8; 32],
    rounds: u32,
}

fn valid_name(name: &str) -> bool {
    (1..=16).contains(&name.len())
        && name.as_bytes()[0].is_ascii_lowercase()
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

fn decode(bytes: &[u8]) -> Option<Vec<User>> {
    let (rows, size) = if let Some(rows) = bytes.strip_prefix(MAGIC) {
        (rows, 68)
    } else {
        (bytes.strip_prefix(LEGACY_MAGIC)?, 64)
    };
    if rows.is_empty() || rows.len() % size != 0 || rows.len() / size > LIMIT {
        return None;
    }
    let mut accounts: Vec<User> = Vec::new();
    for row in rows.chunks_exact(size) {
        let end = row[..16].iter().position(|&b| b == 0).unwrap_or(16);
        let name = core::str::from_utf8(&row[..end]).ok()?;
        if !valid_name(name) || accounts.iter().any(|entry| entry.name.str() == name) {
            return None;
        }
        let mut label = Text::new();
        label.set(name);
        let rounds = if size == 64 {
            LEGACY_ROUNDS
        } else {
            u32::from_le_bytes(row[64..68].try_into().ok()?)
        };
        if !matches!(rounds, LEGACY_ROUNDS | ROUNDS) {
            return None;
        }
        accounts.push(User {
            name: label,
            salt: row[16..32].try_into().ok()?,
            key: row[32..64].try_into().ok()?,
            rounds,
        });
    }
    Some(accounts)
}

fn save(accounts: &[User]) -> bool {
    let previous =
        storage::find_internal(DATABASE).map(|i| unsafe { storage::FILES[i].data.clone() });
    let mut bytes = Vec::with_capacity(4 + accounts.len() * 68);
    bytes.extend_from_slice(MAGIC);
    for account in accounts {
        let mut label = [0; 16];
        label[..account.name.len].copy_from_slice(account.name.str().as_bytes());
        bytes.extend_from_slice(&label);
        bytes.extend_from_slice(&account.salt);
        bytes.extend_from_slice(&account.key);
        bytes.extend_from_slice(&account.rounds.to_le_bytes());
    }
    if storage::store_account_file(DATABASE, &bytes) {
        return true;
    }
    if let Some(previous) = previous {
        let _ = storage::put_internal(DATABASE, &previous.bytes[..previous.len()]);
    } else {
        storage::remove_internal(DATABASE);
    }
    false
}

fn salt() -> Option<[u8; 16]> {
    let mut salt = [0; 16];
    getrandom::getrandom(&mut salt).ok()?;
    Some(salt)
}

fn password_key(password: &[u8], salt: &[u8; 16], rounds: u32) -> [u8; 32] {
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
    for _ in 1..rounds {
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
    Signup,
    Password,
}

impl View {
    fn fields(self) -> core::ops::RangeInclusive<usize> {
        usize::from(self == Self::Password)..=if self == Self::Login { 1 } else { 2 }
    }

    fn secondary(self) -> &'static str {
        match self {
            Self::Setup => "",
            Self::Login => "Create an account",
            Self::Signup => "Back to sign in",
            Self::Password | Self::Add => "Cancel",
        }
    }
}

struct AccountLayout {
    panel: Box2,
    form: Box2,
}

impl AccountLayout {
    fn new(w: i32, h: i32, view: View) -> Self {
        let (width, height) = (
            (w - 32).min(440),
            406 + (view.fields().count() as i32 - 2) * 63,
        );
        let panel = Box2::at((w - width) / 2, (h - height) / 2, width, height);
        let form = Box2::at(panel.x + 32, panel.y + 114, width - 64, height - 146);
        Self { panel, form }
    }

    fn field(&self, row: usize) -> Box2 {
        Box2::at(
            self.form.x,
            self.form.y + row as i32 * 63 + 19,
            self.form.w,
            37,
        )
    }

    fn submit(&self) -> Box2 {
        Box2::at(
            self.form.x,
            self.panel.y + self.panel.h - 126,
            self.form.w,
            38,
        )
    }

    fn secondary(&self) -> Box2 {
        Box2::at(
            self.form.x,
            self.panel.y + self.panel.h - 77,
            self.form.w,
            30,
        )
    }
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
    current: Option<usize>,
    system: bool,
    confirm_disk_write: bool,
}

impl Session {
    pub fn new() -> Self {
        let snapshot =
            storage::find_internal(DATABASE).map(|i| unsafe { storage::FILES[i].data.clone() });
        let users = snapshot.as_ref().and_then(|data| decode(&data.bytes));
        Self {
            view: Some(if snapshot.is_some() {
                View::Login
            } else {
                View::Setup
            }),
            invalid_database: snapshot.is_some() && users.is_none(),
            users: users.unwrap_or_default(),
            entered_name: Text::new(),
            password: Text::new(),
            confirm: Text::new(),
            field: 0,
            message: "",
            blocked_until: 0,
            current: None,
            system: false,
            confirm_disk_write: false,
        }
    }

    pub fn visible(&self) -> bool {
        self.view.is_some()
    }

    pub fn region(&self, w: i32, h: i32) -> Option<Box2> {
        let panel = AccountLayout::new(w, h, self.view?).panel;
        Some(Box2::at(
            panel.x - 10,
            panel.y - 10,
            panel.w + 20,
            panel.h + 26,
        ))
    }

    pub fn log_initial_view(&self) {
        let event: &[u8] = if self.invalid_database {
            b"KreativeOS: account database invalid\r\n"
        } else if self.users.is_empty() {
            b"KreativeOS: account setup ready\r\n"
        } else {
            b"KreativeOS: sign-in ready\r\n"
        };
        unsafe { hardware::log(event) };
    }

    pub fn current(&self) -> Option<usize> {
        self.current
    }

    pub fn system(&self) -> bool {
        self.system
    }

    pub fn enter_system(&mut self) -> bool {
        if !self.administrator() || self.system || self.view.is_some() {
            return false;
        }
        self.system = true;
        true
    }

    pub fn administrator(&self) -> bool {
        self.current == Some(0)
    }

    pub fn name(&self) -> &str {
        if self.system {
            return SYSTEM_NAME;
        }
        self.current.map_or("", |index| {
            let name = self.users[index].name.str();
            if name == SYSTEM_NAME {
                "system1 (local)"
            } else {
                name
            }
        })
    }

    pub fn standard_name(&self) -> Option<Text<16>> {
        self.current
            .filter(|&index| index != 0)
            .map(|index| self.users[index].name)
    }

    pub fn add(&mut self) {
        if self.administrator() {
            self.show(View::Add);
        }
    }

    pub fn change_password(&mut self) {
        if self.current.is_some() && !self.system {
            self.show(View::Password);
        }
    }

    fn clear_passwords(&mut self) {
        for field in [&mut self.password, &mut self.confirm] {
            for byte in &mut field.bytes {
                unsafe { core::ptr::write_volatile(byte, 0) };
            }
            field.clear();
        }
    }

    fn show(&mut self, view: View) {
        self.clear_passwords();
        self.entered_name.clear();
        self.field = usize::from(view == View::Password);
        self.message = "";
        self.view = Some(view);
        self.confirm_disk_write = false;
    }

    pub fn manage(&mut self, arg: &str, out: &mut impl Write) {
        if !self.administrator() {
            let _ = writeln!(out, "Administrator access required");
            return;
        }
        if arg == "add" {
            self.add();
        } else if arg == "password" {
            self.change_password();
        } else if let Some(name) = arg.strip_prefix("remove ") {
            let index = self.users.iter().position(|user| user.name.str() == name);
            if let Some(index) = index.filter(|&index| index != 0) {
                if storage::account_has_files(name) {
                    let _ = writeln!(out, "Resolve this account's files, folders, or draft first");
                    return;
                }
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
        if self.current.take().is_none() {
            return false;
        }
        self.system = false;
        self.show(View::Login);
        unsafe { hardware::log(b"KreativeOS: signed out\r\n") };
        true
    }

    fn back(&mut self, view: View) {
        match view {
            View::Signup => self.show(View::Login),
            View::Add | View::Password => {
                self.clear_passwords();
                self.view = None;
            }
            View::Setup | View::Login => {}
        }
    }

    fn show_secondary(&self, view: View) -> bool {
        !view.secondary().is_empty()
            && (view != View::Login || (!self.invalid_database && self.users.len() < LIMIT))
    }

    fn secondary(&mut self, view: View) {
        if view == View::Login && self.show_secondary(view) {
            self.show(View::Signup);
            unsafe { hardware::log(b"KreativeOS: signup ready\r\n") };
        } else {
            self.back(view);
        }
    }

    pub fn click(&mut self, x: i32, y: i32, w: i32, h: i32) {
        let Some(view) = self.view else { return };
        let layout = AccountLayout::new(w, h, view);
        if self.show_secondary(view) && layout.secondary().has(x, y) {
            self.secondary(view);
        } else if layout.submit().has(x, y) {
            self.submit(view);
        } else if let Some((_, field)) = view
            .fields()
            .enumerate()
            .find(|(row, _)| layout.field(*row).has(x, y))
        {
            self.field = field;
        }
    }

    fn sign_in(&mut self) {
        if self.invalid_database || self.users.is_empty() {
            return;
        }
        if hardware::millis() < self.blocked_until {
            self.message = "Wait before trying again";
            return;
        }
        let account = self.users.iter().position(|user| {
            let name = user.name.str();
            let login = if name == SYSTEM_NAME {
                "local:system1"
            } else {
                name
            };
            login == self.entered_name.str()
        });
        let user = self.users[account.unwrap_or(0)];
        let key = password_key(self.password.str().as_bytes(), &user.salt, user.rounds);
        let difference = key
            .iter()
            .zip(user.key)
            .fold(0, |diff, (&a, b)| diff | (a ^ b));
        self.clear_passwords();
        if let Some(index) = account.filter(|_| difference == 0) {
            storage::enable_data_writes();
            self.current = Some(index);
            self.view = None;
            let event: &[u8] = if self.administrator() {
                b"KreativeOS: administrator signed in\r\n"
            } else {
                b"KreativeOS: standard user signed in\r\n"
            };
            unsafe { hardware::log(event) };
        } else {
            self.blocked_until = hardware::millis() + 1500;
            self.message = "Account name or password is incorrect";
            self.field = 1;
            unsafe { hardware::log(b"KreativeOS: credentials rejected\r\n") };
        }
    }

    fn submit(&mut self, view: View) {
        if view == View::Login {
            self.sign_in();
            return;
        }
        if !storage::can_write() {
            if storage::needs_recovery() {
                self.message = "Repair the exFAT data disk before making account changes.";
            } else if view == View::Setup && storage::has_exfat() {
                if self.confirm_disk_write && storage::enable_data_writes() {
                    self.confirm_disk_write = false;
                    self.message = "Data disk writes enabled. Create your account.";
                } else {
                    self.confirm_disk_write = true;
                    self.message =
                        "Only confirm if this exFAT volume is your KreativeOS data disk.";
                }
            } else {
                self.message = "Account changes need a writable data disk.";
            }
            return;
        }
        if self.password.len < 12 || self.password.str() != self.confirm.str() {
            self.message = "Use a password of at least 12 characters and confirm it";
            self.clear_passwords();
            self.field = 1;
            return;
        }
        let changing = if view == View::Password {
            let Some(index) = self.current else { return };
            Some(index)
        } else {
            None
        };
        let label = changing.map_or(self.entered_name, |index| self.users[index].name);
        if changing.is_none() && label.str() == SYSTEM_NAME {
            self.message = "This name is reserved for the system service.";
            self.field = 0;
            self.clear_passwords();
            return;
        }
        if !valid_name(label.str())
            || (changing.is_none()
                && (self.users.len() >= LIMIT
                    || self.users.iter().any(|user| user.name.str() == label.str())))
        {
            self.message = "Use a new lowercase username. Up to 12 accounts are supported";
            self.field = 0;
            return;
        }
        let Some(salt) = salt() else {
            self.message = "Hardware entropy is unavailable";
            return;
        };
        let key = password_key(self.password.str().as_bytes(), &salt, ROUNDS);
        self.clear_passwords();
        let user = User {
            name: label,
            salt,
            key,
            rounds: ROUNDS,
        };
        let previous = changing.map(|index| core::mem::replace(&mut self.users[index], user));
        if changing.is_none() {
            self.users.push(user);
        }
        if !save(&self.users) {
            if let Some((index, old)) = changing.zip(previous) {
                self.users[index] = old;
            } else {
                self.users.pop();
            }
            self.message = "Could not save the account.";
            self.field = 1;
        } else if matches!(view, View::Setup | View::Signup) {
            self.show(View::Login);
            self.entered_name = label;
            self.field = 1;
            self.message = "Account created. Enter your password to sign in.";
            unsafe { hardware::log(b"KreativeOS: local account saved\r\n") };
        } else {
            self.view = None;
        }
    }

    pub fn key(&mut self, key: u8) {
        let Some(view) = self.view else { return };

        let fields = view.fields();
        let (first, last) = (*fields.start(), *fields.end());
        let secondary = !view.secondary().is_empty()
            && (view != View::Login || (!self.invalid_database && self.users.len() < LIMIT));
        match key {
            27 => self.back(view),
            0x80 => {
                self.field = match self.field {
                    4 => 3,
                    3 => last,
                    _ => self.field.saturating_sub(1).max(first),
                }
            }
            0x81 | 9 => {
                self.field = match self.field {
                    4 => first,
                    3 if secondary => 4,
                    3 => first,
                    field if field == last => 3,
                    field => field + 1,
                }
            }
            b'\n' if self.field < last => self.field += 1,
            b'\n' if self.field == 4 => self.secondary(view),
            b'\n' => self.submit(view),
            8 => match self.field {
                0 => self.entered_name.pop(),
                1 => self.password.pop(),
                2 => self.confirm.pop(),
                _ => {}
            },
            32..=126 => match self.field {
                0 => self.entered_name.push(key),
                1 => self.password.push(key),
                2 => self.confirm.push(key),
                _ => {}
            },
            _ => {}
        }
    }

    pub fn draw(&self, screen: &mut Screen) {
        let Some(view) = self.view else { return };
        let (w, h) = (screen.w as i32, screen.h as i32);
        screen.wallpaper();
        let layout = AccountLayout::new(w, h, view);
        let Box2 {
            x,
            y,
            w: width,
            h: height,
        } = layout.panel;
        screen.shadow(x, y, width, height);
        layout.panel.glass(screen, 16, 0xf9fbfd, 255);
        let (title, detail, mut action) = match view {
            View::Setup => (
                "Create your account",
                "The first account administers this computer.",
                "Create administrator",
            ),
            View::Login => ("Sign in", "Use your local KreativeOS account.", "Sign in"),
            View::Add | View::Signup => (
                "Create an account",
                "A standard account opens the desktop.",
                "Create account",
            ),
            View::Password => ("Change password", self.name(), "Save password"),
        };
        if view == View::Setup
            && !storage::can_write()
            && storage::has_exfat()
            && !storage::needs_recovery()
        {
            action = if self.confirm_disk_write {
                "Confirm data disk writes"
            } else {
                "Enable data disk writes"
            };
        }
        let left = layout.form.x;
        let mut volume_description = Text::<96>::new();
        let detail = if view == View::Setup && !storage::can_write() && storage::has_exfat() {
            let label = storage::data_volume_label();
            let _ = write!(
                volume_description,
                "{}  /  {}",
                if label.len == 0 {
                    "Unlabeled exFAT"
                } else {
                    label.str()
                },
                storage::backend()
            );
            volume_description.str()
        } else {
            detail
        };
        screen.symbol(left, y + 24, 15, 0x175b98);
        screen.text(left + 27, y + 24, "KreativeOS", MUTED, 1);
        screen.text(left, y + 48, title, INK, 3);
        screen.text(left, y + 85, clip_label(detail, layout.form.w, 1), MUTED, 1);
        for (row, i) in view.fields().enumerate() {
            let field = layout.field(row);
            let name = ["Username", "Password", "Confirm password"][i];
            screen.text(field.x, field.y - 19, name, INK, 1);
            screen.input_box(
                field.x,
                field.y,
                field.w,
                field.h,
                self.field == i,
                0x175b98,
            );
            if i == 0 {
                screen.text(
                    field.x + 12,
                    field.y + 9,
                    clip_label(self.entered_name.str(), field.w - 24, 1),
                    INK,
                    1,
                );
            } else {
                let length = if i == 1 {
                    self.password.len
                } else {
                    self.confirm.len
                };
                for index in 0..length.min(((field.w - 24) / 8).max(0) as usize) {
                    screen.rounded(
                        field.x + 12 + index as i32 * 8,
                        field.y + 16,
                        5,
                        5,
                        2,
                        INK,
                        255,
                    );
                }
            }
        }
        let submit = layout.submit();
        submit.glass(
            screen,
            7,
            if self.field == 3 { 0x103d67 } else { 0x175b98 },
            255,
        );
        screen.centered(submit.x, submit.y, submit.w, submit.h, action, 0xffffff, 1);
        if !view.secondary().is_empty()
            && (view != View::Login || (!self.invalid_database && self.users.len() < LIMIT))
        {
            let link = layout.secondary();
            if self.field == 4 {
                link.glass(screen, 6, 0xe4eef8, 255);
            }
            screen.centered(
                link.x,
                link.y,
                link.w,
                link.h,
                view.secondary(),
                0x175b98,
                1,
            );
        }
        let (message, color) = if self.invalid_database {
            ("The account database could not be read.", 0x973c48)
        } else if self.message.starts_with("Account created") {
            (self.message, 0x216548)
        } else if !self.message.is_empty() {
            (self.message, 0x973c48)
        } else if view != View::Login {
            ("Use at least 12 characters for your password.", MUTED)
        } else {
            ("", MUTED)
        };
        screen.wrapped_small(left, y + height - 45, layout.form.w, 38, message, color);
    }
}
