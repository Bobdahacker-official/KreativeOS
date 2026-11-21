use crate::{
    graphics::Screen,
    hardware::{self, Event, Input},
    net::{self, Network},
    pictures::Catalog,
    storage,
    text::Text,
};
use core::fmt::Write;
pub(crate) const INK: u32 = 0x202e3d;
pub(crate) const MUTED: u32 = 0x526273;
const PANEL: u32 = 0xf7f9fc;
pub(crate) const SURFACE: u32 = 0xe7edf4;
const DIVIDER: u32 = 0xdbe3ec;
const SELECTED: u32 = 0xe0edfa;
const NAMES: [&str; 15] = [
    "Files",
    "Notes",
    "Terminal",
    "Browser",
    "Calculator",
    "Paint",
    "Snake",
    "Packages",
    "Network",
    "Settings",
    "Tic Tac Toe",
    "Kreative Guide",
    "Images",
    "Organizer",
    "Hardware Center",
];
const GUIDE: usize = 11;
const IMAGES: usize = 12;
const ORGANIZER: usize = 13;
const HARDWARE_CENTER: usize = 14;
const APP_COUNT: usize = NAMES.len();
const BAR: i32 = 54;
const APP_COLORS: [u32; APP_COUNT] = [
    0xeab353, 0x7ac7b0, 0x6687b9, 0x69b8f1, 0x9e91ee, 0xe8829e, 0x89c376, 0xd19d77, 0x6bd1ca,
    0x9aaed4, 0xe0a373, 0xa2c58b, 0x75b9f2, 0xb89de9, 0x69bdc2,
];
const PRESETS: [(&str, u8, u32, u8); 4] = [
    ("Glacier", 0, 0x185c9a, 0),
    ("Dusk", 1, 0x5353a0, 1),
    ("Lagoon", 0, 0x176e62, 2),
    ("Contrast", 2, 0x254256, 3),
];

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Box2 {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) w: i32,
    pub(crate) h: i32,
}
impl Box2 {
    pub(crate) fn at(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }
    pub(crate) fn has(self, x: i32, y: i32) -> bool {
        (self.x..self.x + self.w).contains(&x) && (self.y..self.y + self.h).contains(&y)
    }
    pub(crate) fn glass(self, s: &mut Screen, radius: i32, fill: u32, alpha: u32) {
        s.rounded(self.x, self.y, self.w, self.h, radius, fill, alpha);
    }
}
struct Dock {
    body: Box2,
    start: Box2,
    status: Box2,
    step: i32,
}
impl Dock {
    fn new(w: i32, h: i32) -> Self {
        let width = (w - 24).min(848);
        let x = (w - width) / 2;
        let y = h - BAR;
        Self {
            body: Box2::at(x, y, width, 46),
            start: Box2::at(x + 6, y + 5, 73, 36),
            status: Box2::at(x + width - 106, y + 5, 100, 36),
            step: (width - 192) / APP_COUNT as i32,
        }
    }
    fn slot(&self, index: usize) -> Box2 {
        Box2::at(
            self.body.x + 84 + index as i32 * self.step,
            self.body.y + 5,
            self.step,
            36,
        )
    }
}
struct AppGrid {
    body: Box2,
    search: Box2,
    step: i32,
}
impl AppGrid {
    fn new(w: i32, h: i32) -> Self {
        let width = (w - 40).min(688);
        let x = (w - width) / 2;
        let y = (h - BAR - 342) / 2;
        Self {
            body: Box2::at(x, y, width, 342),
            search: Box2::at(x + width - 211, y + 22, 187, 35),
            step: (width - 36) / 5,
        }
    }
    fn item(&self, index: usize) -> Box2 {
        Box2::at(
            self.body.x + 18 + (index % 5) as i32 * self.step,
            self.body.y + 76 + (index / 5) as i32 * 82,
            self.step - 7,
            73,
        )
    }
}
struct SettingsLayout {
    wide: bool,
    x: i32,
    y: i32,
    width: i32,
}
impl SettingsLayout {
    fn new(w: i32) -> Self {
        let wide = w >= 565;
        let x = if wide { 174 } else { 18 };
        Self {
            wide,
            x,
            y: if wide { 60 } else { 100 },
            width: w - x - 18,
        }
    }
    fn nav(&self, index: usize) -> Box2 {
        if self.wide {
            Box2::at(15, 94 + index as i32 * 42, 139, 36)
        } else {
            let step = self.width / 4;
            Box2::at(18 + index as i32 * step, 49, step - 4, 34)
        }
    }
    fn preset(&self, index: usize) -> Box2 {
        let step = self.width / 4;
        Box2::at(self.x + index as i32 * step, self.y + 65, step - 8, 74)
    }
    fn mode(&self, index: usize) -> Box2 {
        let step = (self.width / 3).min(118);
        Box2::at(self.x + index as i32 * step, self.y + 182, step - 6, 34)
    }
    fn swatch(&self, index: usize) -> Box2 {
        Box2::at(self.x + index as i32 * 36, self.y + 251, 27, 27)
    }
    fn action(&self, index: usize) -> Box2 {
        Box2::at(
            self.x,
            self.y + 155 + index as i32 * 44,
            self.width.min(240),
            34,
        )
    }
}

fn calculator_key(width: i32, height: i32, index: usize) -> Box2 {
    let column_width = (width - 48) / 4;
    let row_height = ((height - 161) / 4).clamp(16, 60);
    Box2::at(
        24 + (index % 4) as i32 * column_width,
        141 + (index / 4) as i32 * row_height,
        column_width - 6,
        row_height - 6,
    )
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum WebJob {
    Page,
    Image,
    Audio,
}
struct Tab {
    url: Text<384>,
    page: Text<12288>,
    links: alloc::vec::Vec<crate::web::Link>,
    picture: Option<crate::pictures::Picture>,
    audio_url: Option<Text<384>>,
    audio: alloc::vec::Vec<u8>,
    download: Option<alloc::vec::Vec<u8>>,
    scroll: usize,
    status: &'static str,
}
enum SearchTarget {
    App(usize),
    File(Text<{ storage::PATH_CAP }>),
    Web(alloc::boxed::Box<Text<384>>),
}
enum FileChoice {
    Parent,
    Folder(alloc::boxed::Box<Text<{ storage::PATH_CAP }>>),
    Image(usize),
    Document(usize),
}

#[derive(Clone, Copy)]
struct Window {
    open: bool,
    min: bool,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}
impl Window {
    const fn new() -> Self {
        Self {
            open: false,
            min: false,
            x: 150,
            y: 90,
            w: 710,
            h: 510,
        }
    }
    fn inside(&self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h
    }
}
struct Snake {
    body: [(i32, i32); 384],
    len: usize,
    dir: (i32, i32),
    next: (i32, i32),
    food: (i32, i32),
    alive: bool,
    at: u64,
    seed: u32,
}
impl Snake {
    fn new() -> Self {
        let mut s = Self {
            body: [(0, 0); 384],
            len: 4,
            dir: (1, 0),
            next: (1, 0),
            food: (15, 8),
            alive: true,
            at: 0,
            seed: 12345,
        };
        for i in 0..4 {
            s.body[i] = (8 - i as i32, 8)
        }
        s
    }
    fn step(&mut self) {
        if !self.alive || hardware::millis() - self.at < 140 {
            return;
        }
        self.at = hardware::millis();
        self.dir = self.next;
        let p = (self.body[0].0 + self.dir.0, self.body[0].1 + self.dir.1);
        let eat = p == self.food;
        let check = if eat { self.len } else { self.len - 1 };
        if p.0 < 0 || p.0 >= 24 || p.1 < 0 || p.1 >= 16 || self.body[..check].contains(&p) {
            self.alive = false;
            return;
        }
        if eat && self.len < 384 {
            self.len += 1
        }
        for i in (1..self.len).rev() {
            self.body[i] = self.body[i - 1]
        }
        self.body[0] = p;
        if eat {
            if self.len == 384 {
                self.alive = false;
                return;
            }
            loop {
                self.seed = self.seed.wrapping_mul(1664525).wrapping_add(1013904223);
                self.food = ((self.seed % 24) as i32, ((self.seed / 24) % 16) as i32);
                if !self.body[..self.len].contains(&self.food) {
                    break;
                }
            }
        }
    }
}
static mut PAINT: [u32; 320 * 180] = [0xf2f0e9; 320 * 180];
struct Desktop {
    windows: [Window; APP_COUNT],
    restore: [Option<Window>; APP_COUNT],
    order: [usize; APP_COUNT],
    focus: usize,
    menu: bool,
    launch_index: usize,
    search: Option<Text<64>>,
    search_row: usize,
    overview: bool,
    theme: u8,
    wallpaper: u8,
    settings_page: usize,
    restart_armed: bool,
    settings_notice: &'static str,
    magnifier: bool,
    workspace: usize,
    window_workspace: [usize; APP_COUNT],
    clipboard: alloc::vec::Vec<storage::Data>,
    mx: i32,
    my: i32,
    buttons: u8,
    drag: Option<(usize, i32, i32)>,
    accent: u32,
    note: storage::Data,
    filename: Text<{ storage::PATH_CAP }>,
    draft_at: Option<u64>,
    file_search: Text<48>,
    file_cursor: usize,
    file_images: bool,
    file_folder: Text<{ storage::PATH_CAP }>,
    file_preview: Option<(usize, crate::pictures::Picture)>,
    terminal: Text<4096>,
    command: Text<128>,
    url: Text<384>,
    page: Text<12288>,
    web_links: alloc::vec::Vec<crate::web::Link>,
    web_job: WebJob,
    resource: Text<384>,
    redirects: u8,
    web_picture: Option<crate::pictures::Picture>,
    audio_url: Option<Text<384>>,
    audio_clip: alloc::vec::Vec<u8>,
    web_status: &'static str,
    tab: usize,
    tabs: [Option<alloc::boxed::Box<Tab>>; 4],
    download: Option<alloc::vec::Vec<u8>>,
    history: alloc::vec::Vec<Text<384>>,
    bookmarks: alloc::vec::Vec<Text<384>>,
    web_list: u8,
    web_list_offset: usize,
    native_wifi: crate::ax210::Status,
    wifi_ssid: Text<32>,
    wifi_pass: Text<63>,
    wifi_password_field: bool,
    wifi_status: &'static str,
    ethernet: crate::ethernet::Editor,
    ethernet_tab: bool,
    access_points: alloc::vec::Vec<crate::wifi::AccessPoint>,
    scanning_results: bool,
    access_point_offset: usize,
    saved_wifi: alloc::vec::Vec<Text<384>>,
    saved_wifi_view: bool,
    tray: bool,
    tray_tab: usize,
    devices: alloc::vec::Vec<Text<80>>,
    device_offset: usize,
    byte_rates: (u64, u64),
    alerts: alloc::vec::Vec<&'static str>,
    last_link: Option<u16>,
    scroll: usize,
    calc: Text<128>,
    answer: Text<128>,
    notice: Text<128>,
    snake: Snake,
    paint_color: u32,
    last_paint: Option<(i32, i32)>,
    painting: bool,
    installed: bool,
    package: Text<2048>,
    ttt: [u8; 9],
    turn: u8,
    winner: u8,
    images: Catalog,
    organizer: crate::organizer::Organizer,
    users: crate::users::Session,
    scheduler: crate::ring3::Scheduler,
    hardware_center: crate::hardware_center::Center,
}
impl Desktop {
    #[inline(never)]
    fn new(w: usize, h: usize, users: crate::users::Session) -> Self {
        let mut appearance = (0, 0, false, 0);
        if let Some(index) = storage::find("Appearance.cfg") {
            let data = unsafe { storage::FILES[index].data.clone() };
            let mut fields = data
                .str()
                .split_ascii_whitespace()
                .filter_map(|word| word.parse::<u8>().ok());
            appearance.0 = fields.next().unwrap_or(0).min(2);
            appearance.1 = fields.next().unwrap_or(0).min(3);
            appearance.2 = fields.next().unwrap_or(0) == 1;
            appearance.3 = fields.next().unwrap_or(0).min(3);
        }
        let mut d = Self {
            windows: [Window::new(); APP_COUNT],
            restore: [None; APP_COUNT],
            order: core::array::from_fn(|i| i),
            focus: 0,
            menu: false,
            launch_index: 0,
            search: None,
            search_row: 0,
            overview: false,
            magnifier: appearance.2,
            theme: appearance.0,
            wallpaper: appearance.1,
            settings_page: 0,
            restart_armed: false,
            settings_notice: "",
            workspace: 0,
            window_workspace: [0; APP_COUNT],
            clipboard: alloc::vec::Vec::new(),
            mx: 500,
            my: 380,
            buttons: 0,
            drag: None,
            accent: PRESETS[appearance.3 as usize].2,
            note: storage::Data::default(),
            filename: Text::new(),
            draft_at: None,
            file_search: Text::new(),
            file_cursor: 0,
            file_images: false,
            file_folder: Text::new(),
            file_preview: None,
            terminal: Text::new(),
            command: Text::new(),
            url: Text::new(),
            page: Text::new(),
            web_links: alloc::vec::Vec::new(),
            web_job: WebJob::Page,
            resource: Text::new(),
            redirects: 0,
            web_picture: None,
            audio_url: None,
            audio_clip: alloc::vec::Vec::new(),
            web_status: "",
            tab: 0,
            tabs: [None, None, None, None],
            download: None,
            history: read_urls("History.urls"),
            bookmarks: read_urls("Bookmarks.urls"),
            web_list: 0,
            web_list_offset: 0,
            native_wifi: crate::ax210::probe(),
            wifi_ssid: Text::new(),
            wifi_pass: Text::new(),
            wifi_password_field: false,
            wifi_status: "ESP-AT on COM2",
            access_points: alloc::vec::Vec::new(),
            scanning_results: false,
            access_point_offset: 0,
            saved_wifi: read_urls("WiFi.networks"),
            saved_wifi_view: false,
            tray: false,
            tray_tab: 0,
            devices: alloc::vec::Vec::new(),
            device_offset: 0,
            byte_rates: (0, 0),
            alerts: alloc::vec::Vec::new(),
            last_link: None,
            scroll: 0,
            calc: Text::new(),
            answer: Text::new(),
            notice: Text::new(),
            snake: Snake::new(),
            paint_color: 0x176f80,
            last_paint: None,
            painting: false,
            installed: false,
            package: Text::new(),
            ttt: [0; 9],
            turn: 1,
            winner: 0,
            images: Catalog::new(),
            organizer: crate::organizer::Organizer::new(),
            users,
            scheduler: crate::ring3::Scheduler::new(),
            hardware_center: crate::hardware_center::Center::new(),
            ethernet: crate::ethernet::Editor::new(),
            ethernet_tab: true,
        };
        for i in 0..APP_COUNT {
            d.windows[i].w = (w as i32 - 90).min(760);
            d.windows[i].h = (h as i32 - 115).min(530);
            d.windows[i].x = 45 + (i as i32 % 4) * 18;
            d.windows[i].y = 40 + (i as i32 % 3) * 16
        }
        d.filename.set(if storage::find("Welcome.txt").is_some() {
            "Welcome.txt"
        } else {
            "Untitled.txt"
        });
        if let Some(i) = storage::find("Welcome.txt") {
            unsafe { d.note = storage::FILES[i].data.clone() }
        }
        d.terminal
            .set("KreativeOS shell\nType help to see commands.\n");
        d.url.set("file:Readme.html");
        d.load_local();
        d.windows[0] = Window {
            open: false,
            min: false,
            x: 64,
            y: 102,
            w: 510,
            h: 430,
        };
        d.windows[1] = Window {
            open: false,
            min: false,
            x: (w as i32 - 650).max(170),
            y: 160,
            w: 600,
            h: 440,
        };
        d.windows[IMAGES] = Window {
            open: false,
            min: false,
            x: (w as i32 / 5).max(105),
            y: (h as i32 / 6).max(45),
            w: (w as i32 * 3 / 4).min(800),
            h: (h as i32 * 2 / 3).min(545),
        };
        if let Some((name, note)) = d
            .users
            .current()
            .filter(|_| !d.users.system())
            .and_then(|_| storage::load_draft())
        {
            let already_saved = storage::find(name.str()).is_some_and(|index| {
                let saved = unsafe { &storage::FILES[index].data };
                saved.bytes[..saved.len()] == note.bytes[..note.len()]
            });
            if !already_saved {
                d.filename.set(name.str());
                d.note = note;
                d.open(1);
                d.notice.set("Recovered an unsaved draft");
                d.alert("Notes recovered an unsaved draft");
                unsafe { hardware::log(b"KreativeOS: note draft recovered\r\n") };
            }
        }
        if let Some(package) = crate::packages::CATALOG
            .iter()
            .find_map(|package| package.open())
        {
            d.package = package;
            d.installed = true
        }
        d
    }
    fn open(&mut self, id: usize) {
        if id >= APP_COUNT {
            return;
        }
        if self.focus != id || !self.visible(id) {
            self.restart_armed = false;
        }
        if id == HARDWARE_CENTER && !self.windows[id].open {
            self.hardware_center.refresh();
        }
        self.search = None;
        if self.windows[id].open {
            self.workspace = self.window_workspace[id];
        } else {
            self.window_workspace[id] = self.workspace;
        }
        self.tray = false;
        self.windows[id].open = true;
        self.windows[id].min = false;
        self.focus = id;
        let pos = self.order.iter().position(|&x| x == id).unwrap_or(0);
        for i in pos..APP_COUNT - 1 {
            self.order[i] = self.order[i + 1]
        }
        self.order[APP_COUNT - 1] = id;
        self.menu = false
    }
    fn visible(&self, id: usize) -> bool {
        self.windows[id].open
            && !self.windows[id].min
            && self.window_workspace[id] == self.workspace
    }
    fn search_results(&self) -> alloc::vec::Vec<(Text<96>, SearchTarget)> {
        let Some(query) = self.search else {
            return alloc::vec::Vec::new();
        };
        let matches = |label: &str| {
            query.len == 0
                || label
                    .as_bytes()
                    .windows(query.len)
                    .any(|part| part.eq_ignore_ascii_case(&query.bytes[..query.len]))
        };
        let mut rows = alloc::vec::Vec::new();
        for (index, label) in NAMES.iter().enumerate() {
            if self.overview && !self.windows[index].open {
                continue;
            }
            if (index != GUIDE || self.installed)
                && (matches(label)
                    || (index == ORGANIZER && matches("calendar contacts"))
                    || (index == HARDWARE_CENTER
                        && matches("pci sata disk devices drivers diagnostics"))
                    || (index == 9 && matches("display input appearance settings power accounts")))
            {
                let mut text = Text::new();
                if self.overview {
                    let _ = write!(
                        text,
                        "Workspace {}    {label}",
                        self.window_workspace[index] + 1
                    );
                } else {
                    let _ = write!(text, "App   {label}");
                }
                rows.push((text, SearchTarget::App(index)));
            }
        }
        if self.overview {
            rows.truncate(8);
            return rows;
        }
        for file in unsafe { &storage::FILES } {
            let Some(name) = storage::visible_name(file.name.str()).filter(|_| file.used) else {
                continue;
            };
            if matches(name) || (query.len > 0 && matches(file.data.str())) {
                let mut text = Text::new();
                let mut local_name = Text::new();
                local_name.set(name);
                let _ = write!(text, "File   {name}");
                rows.push((text, SearchTarget::File(local_name)));
            }
        }
        for url in &self.history {
            if matches(url.str()) {
                let mut text = Text::new();
                let _ = write!(text, "Web   {}", clip_label(url.str(), 280, 1));
                rows.push((text, SearchTarget::Web(alloc::boxed::Box::new(*url))));
            }
        }
        rows.truncate(8);
        rows
    }
    fn search_open(&mut self, row: usize, n: &mut Network) {
        if let Some((_, target)) = self.search_results().into_iter().nth(row) {
            match target {
                SearchTarget::App(id) => self.open(id),
                SearchTarget::File(name) => self.open_file(name),
                SearchTarget::Web(url) if n.state == 0 && !n.tls_pending => {
                    self.url = *url;
                    self.navigate(n);
                    self.open(3);
                }
                SearchTarget::Web(_) => {}
            }
        }
    }
    fn open_file(&mut self, name: Text<{ storage::PATH_CAP }>) {
        let Some(index) = storage::find(name.str()) else {
            return;
        };
        let file = unsafe { storage::FILES[index].data.clone() };
        if file.bytes[..file.len()].starts_with(b"PK\x03\x04") || name.str().ends_with(".zip") {
            self.archive(name.str(), false);
            self.open(2);
        } else if name.str().ends_with(".html") {
            self.url.set("file:");
            self.url.append(name.str());
            self.load_local();
            self.open(3);
        } else {
            if !self.checkpoint_draft(true) {
                self.open(1);
                return;
            }
            self.filename = name;
            self.note = file;
            self.open(1);
        }
    }
    fn file_matches(&self, name: &str) -> bool {
        self.file_search.len == 0
            || name.as_bytes().windows(self.file_search.len).any(|part| {
                part.eq_ignore_ascii_case(&self.file_search.bytes[..self.file_search.len])
            })
    }
    fn file_items(&self) -> alloc::vec::Vec<FileChoice> {
        if self.file_images {
            return self
                .images
                .files
                .iter()
                .enumerate()
                .filter_map(|(i, (name, _))| {
                    self.file_matches(name).then_some(FileChoice::Image(i))
                })
                .collect();
        }
        let mut items = alloc::vec::Vec::new();
        if self.file_folder.len != 0 {
            items.push(FileChoice::Parent);
        }
        for folder in storage::folders() {
            let (parent, label) = folder.str().rsplit_once('/').unwrap_or(("", folder.str()));
            if parent.eq_ignore_ascii_case(self.file_folder.str()) && self.file_matches(label) {
                items.push(FileChoice::Folder(alloc::boxed::Box::new(folder)));
            }
        }
        for (index, file) in unsafe { &storage::FILES }.iter().enumerate() {
            let Some(path) = storage::visible_name(file.name.str()).filter(|_| file.used) else {
                continue;
            };
            let (parent, label) = path.rsplit_once('/').unwrap_or(("", path));
            if parent.eq_ignore_ascii_case(self.file_folder.str()) && self.file_matches(label) {
                items.push(FileChoice::Document(index));
            }
        }
        items
    }

    fn file_viewport(&self) -> (usize, usize) {
        let rows = ((self.windows[0].h - 135) / 36).max(1) as usize;
        (self.file_cursor.saturating_sub(rows - 1), rows)
    }

    fn file_select(&mut self, row: usize) {
        let items = self.file_items();
        let Some(item) = items.get(row) else { return };
        self.file_cursor = row;
        self.file_preview = match item {
            FileChoice::Image(index) => {
                crate::pictures::decode(self.images.files[*index].1).map(|image| (*index, image))
            }
            _ => None,
        };
    }

    fn file_open_selected(&mut self) {
        let items = self.file_items();
        let Some(item) = items.get(self.file_cursor) else {
            return;
        };
        match item {
            FileChoice::Parent => {
                self.file_folder.len = self.file_folder.str().rfind('/').unwrap_or(0);
                self.file_cursor = 0;
                self.file_search.clear();
            }
            FileChoice::Folder(folder) => {
                self.file_folder = **folder;
                self.file_cursor = 0;
                self.file_search.clear();
            }
            FileChoice::Image(index) => {
                self.images.select(*index);
                self.open(IMAGES);
            }
            FileChoice::Document(index) => {
                if let Some(path) =
                    storage::visible_name(unsafe { storage::FILES[*index].name.str() })
                {
                    let mut name = Text::new();
                    name.set(path);
                    self.open_file(name);
                }
            }
        }
    }

    fn archive(&mut self, name: &str, extract: bool) {
        let Some(index) = storage::find(name) else {
            self.terminal.append("Archive not found");
            return;
        };
        let data = unsafe { storage::FILES[index].data.clone() };
        let Some(entries) = crate::archive::read(&data.bytes[..data.len()]) else {
            self.terminal
                .append("Unsupported ZIP layout, file size or checksum");
            return;
        };
        if extract {
            self.terminal
                .append(match crate::archive::extract(&entries) {
                    Ok(true) => "Extracted to Files",
                    Ok(false) => "Extracted in memory",
                    Err(error) => error,
                });
        } else {
            let _ = writeln!(self.terminal, "\n{} ({} entries)", name, entries.len());
            for entry in entries {
                let _ = writeln!(self.terminal, "{}", entry.name);
            }
            let _ = writeln!(self.terminal, "Type unzip {name} to extract.");
        }
    }
    fn alert(&mut self, event: &'static str) {
        if self.alerts.first() != Some(&event) {
            self.alerts.insert(0, event);
            self.alerts.truncate(5);
        }
    }
    fn clear_web_media(&mut self) {
        crate::audio::stop();
        self.web_links.clear();
        self.web_picture = None;
        self.audio_url = None;
        self.audio_clip.clear();
        self.download = None;
        self.web_status = "";
    }
    fn navigate(&mut self, n: &mut Network) {
        if n.state != 0 || n.tls_pending {
            return;
        }
        self.web_list = 0;
        self.web_list_offset = 0;
        self.clear_web_media();
        self.scroll = 0;
        if self.url.str().starts_with("file:") {
            self.load_local();
            self.record_visit();
            return;
        }
        self.page.clear();
        self.request(self.url, WebJob::Page, n);
    }
    fn record_visit(&mut self) {
        if self
            .history
            .first()
            .is_some_and(|url| url.str() == self.url.str())
        {
            return;
        }
        self.history.retain(|url| url.str() != self.url.str());
        self.history.insert(0, self.url);
        save_urls("History.urls", &mut self.history);
    }
    fn bookmark(&mut self) {
        if self.url.len == 0 {
            return;
        }
        if let Some(index) = self
            .bookmarks
            .iter()
            .position(|url| url.str() == self.url.str())
        {
            self.bookmarks.remove(index);
            self.web_status = "Bookmark removed";
        } else {
            self.bookmarks.insert(0, self.url);
            self.web_status = "Bookmark added";
        }
        save_urls("Bookmarks.urls", &mut self.bookmarks);
    }
    fn switch_tab(&mut self, tab: usize, n: &Network) {
        if tab == self.tab || n.state != 0 || n.tls_pending || n.changed {
            return;
        }
        crate::audio::stop();
        self.tabs[self.tab] = Some(alloc::boxed::Box::new(Tab {
            url: self.url,
            page: self.page,
            links: core::mem::take(&mut self.web_links),
            picture: self.web_picture.take(),
            audio_url: self.audio_url.take(),
            audio: core::mem::take(&mut self.audio_clip),
            download: self.download.take(),
            scroll: self.scroll,
            status: self.web_status,
        }));
        self.tab = tab;
        self.web_list = 0;
        self.web_list_offset = 0;
        if let Some(saved) = self.tabs[tab].take() {
            self.url = saved.url;
            self.page = saved.page;
            self.web_links = saved.links;
            self.web_picture = saved.picture;
            self.audio_url = saved.audio_url;
            self.audio_clip = saved.audio;
            self.download = saved.download;
            self.scroll = saved.scroll;
            self.web_status = saved.status;
        } else {
            self.url.set("file:Readme.html");
            self.load_local();
        }
    }
    fn web_row(&self, index: usize) -> Option<(&str, Text<384>)> {
        match self.web_list {
            1 => self
                .web_links
                .get(index)
                .map(|link| (link.label.str(), link.url)),
            2 => self.bookmarks.get(index).map(|url| ("", *url)),
            3 => self.history.get(index).map(|url| ("", *url)),
            _ => None,
        }
    }
    fn scroll_web(&mut self, delta: i32) {
        if self.web_list == 0 {
            self.scroll = self
                .scroll
                .saturating_add_signed(delta as isize * 80)
                .min(self.page.len);
        } else {
            let count = match self.web_list {
                1 => self.web_links.len(),
                2 => self.bookmarks.len(),
                _ => self.history.len(),
            };
            let rows = Self::web_rows(self.windows[3].h);
            self.web_list_offset = self
                .web_list_offset
                .saturating_add_signed(delta as isize)
                .min(count.saturating_sub(rows));
        }
    }
    fn web_rows(height: i32) -> usize {
        ((height - 196) / 23).max(0) as usize
    }
    fn save_download(&mut self) {
        let Some(data) = &self.download else {
            self.web_status = "No downloadable response";
            return;
        };
        let mut name = Text::<32>::new();
        for index in 1..=storage::COUNT {
            name.clear();
            let _ = write!(name, "Download-{index}");
            if storage::find(name.str()).is_none() {
                break;
            }
        }
        self.web_status =
            if storage::find(name.str()).is_none() && storage::put(name.str(), data).is_ok() {
                if storage::sync() {
                    "Download saved in Files"
                } else {
                    "Download available in memory"
                }
            } else {
                "Could not save download: file directory full"
            };
    }
    fn request(&mut self, url: Text<384>, job: WebJob, n: &mut Network) {
        self.resource = url;
        self.web_job = job;
        self.redirects = 0;
        n.browse(url.str());
    }
    fn play_audio(&mut self, n: &mut Network) {
        if !self.audio_clip.is_empty() {
            self.web_status = match crate::audio::play(&self.audio_clip) {
                Ok(()) => "Playing WAV audio",
                Err(error) => error,
            };
        } else if let Some(url) = self.audio_url {
            if n.state == 0 {
                self.request(url, WebJob::Audio, n);
                self.web_status = "Loading audio";
            }
        }
    }
    fn receive_web(&mut self, n: &mut Network) {
        use crate::web::Media;
        let Some(response) = n.response.take() else {
            if n.state == 0 && !self.url.str().starts_with("file:") {
                if self.web_job == WebJob::Page {
                    self.web_status = n.status;
                } else if !n.verified {
                    self.web_status = n.status;
                }
            }
            return;
        };
        if response.location.len > 0 {
            if self.redirects < 4 {
                if let Some(url) = crate::web::resolve(self.resource.str(), response.location.str())
                {
                    self.redirects += 1;
                    self.resource = url;
                    if self.web_job == WebJob::Page {
                        self.url = url;
                    }
                    n.browse(url.str());
                    return;
                }
            }
            self.web_status = "Redirect could not be followed";
            return;
        }
        if self.web_job == WebJob::Page {
            self.record_visit();
            self.web_status = if n.verified {
                "HTTPS certificate verified"
            } else {
                n.status
            };
            self.download = (response.bytes.len() <= storage::CAP).then(|| response.bytes.clone());
        }
        if response.media == Media::Image && self.web_job != WebJob::Audio {
            self.web_picture = crate::pictures::decode(&response.bytes);
            self.web_status = if self.web_picture.is_some() {
                if n.verified {
                    "Image loaded over HTTPS"
                } else {
                    "Image loaded over HTTP"
                }
            } else {
                "Image format or size unsupported"
            };
            return;
        }
        if response.media == Media::Audio && self.web_job != WebJob::Image {
            if crate::audio::wave(&response.bytes).is_none() {
                self.web_status = "Expected 16-bit PCM WAV";
                return;
            }
            self.audio_clip = response.bytes;
            self.web_status = "WAV audio ready";
            if self.web_job == WebJob::Audio {
                self.play_audio(n);
            }
            return;
        }
        if self.web_job == WebJob::Page {
            net::readable(n.body.str(), &mut self.page);
            let html = core::str::from_utf8(&response.bytes).unwrap_or(n.body.str());
            self.web_links = crate::web::links(self.resource.str(), html);
            let (image, audio) = crate::web::media_urls(self.resource.str(), html);
            self.audio_url = audio;
            if let Some(url) = image {
                self.request(url, WebJob::Image, n);
            }
        } else {
            self.web_status = "The server returned an unexpected media type";
        }
    }
    fn join_wifi(&mut self, n: &mut Network) {
        self.wifi_status = match n.wifi.join(self.wifi_ssid.str(), self.wifi_pass.str()) {
            Ok(()) => {
                self.saved_wifi
                    .retain(|name| name.str() != self.wifi_ssid.str());
                let mut name = Text::new();
                name.set(self.wifi_ssid.str());
                self.saved_wifi.insert(0, name);
                save_urls("WiFi.networks", &mut self.saved_wifi);
                "Connected. HTTPS uses Wi-Fi."
            }
            Err(error) => error,
        };
        self.wifi_pass.bytes.fill(0);
        self.wifi_pass.clear();
        self.alert(self.wifi_status);
    }
    fn scan_wifi(&mut self, n: &mut Network) {
        self.access_point_offset = 0;
        self.saved_wifi_view = false;
        self.scanning_results = true;
        self.access_points.clear();
        match n.wifi.scan() {
            Ok(points) => {
                self.wifi_status = if points.is_empty() {
                    "No networks found"
                } else {
                    "Choose a network to connect"
                };
                self.access_points = points;
            }
            Err(error) => self.wifi_status = error,
        }
    }
    fn view_saved_wifi(&mut self) {
        self.access_point_offset = 0;
        self.scanning_results = true;
        self.saved_wifi_view = true;
        self.access_points = self
            .saved_wifi
            .iter()
            .filter(|name| name.len <= 32)
            .map(|name| {
                let mut ssid = Text::new();
                ssid.set(name.str());
                crate::wifi::AccessPoint {
                    ssid,
                    signal: 0,
                    security: 0,
                }
            })
            .collect();
        self.wifi_status = if self.access_points.is_empty() {
            "No saved networks"
        } else {
            "Choose a saved network and enter its password"
        };
    }
    fn hotspot(&mut self, n: &mut Network, wpa3: bool) {
        self.wifi_status =
            match n
                .wifi
                .start_hotspot(self.wifi_ssid.str(), self.wifi_pass.str(), wpa3)
            {
                Ok(()) => "Local hotspot enabled",
                Err(error) => error,
            };
        self.wifi_pass.bytes.fill(0);
        self.wifi_pass.clear();
        self.alert(self.wifi_status);
    }
    fn load_local(&mut self) {
        self.clear_web_media();
        self.page.clear();
        let name = self
            .url
            .str()
            .strip_prefix("file:")
            .unwrap_or("Readme.html");
        if let Some(i) = storage::find(name) {
            unsafe { net::readable(storage::FILES[i].data.str(), &mut self.page) }
        } else {
            self.page.set("File not found")
        };
        self.scroll = 0
    }
    fn save(&mut self) {
        match storage::save_file(self.filename.str(), &self.note.bytes) {
            Ok(()) => {
                self.draft_at = None;
                storage::clear_saved_draft(self.filename.str());
                self.notice.set("Saved to disk");
                unsafe { hardware::log(b"KreativeOS: note saved to disk\r\n") };
            }
            Err(error) => self.notice.set(error),
        }
    }
    fn checkpoint_draft(&mut self, now: bool) -> bool {
        let Some(deadline) = self.draft_at else {
            return true;
        };
        if !now && hardware::millis() < deadline {
            return false;
        }
        if storage::store_draft(self.filename.str(), &self.note) {
            self.draft_at = None;
            unsafe { hardware::log(b"KreativeOS: note draft saved\r\n") };
            true
        } else {
            self.draft_at = Some(hardware::millis() + 5_000);
            self.notice
                .set("Draft could not be saved. Keep this note open or save it to disk.");
            false
        }
    }
    fn package_install(&mut self, index: usize, open: bool) {
        let Some(package) = crate::packages::CATALOG.get(index) else {
            return;
        };
        let result = if open && package.open().is_some() {
            Ok(true)
        } else {
            package.install()
        };
        match result {
            Ok(persisted) => {
                if let Some(document) = package.open() {
                    self.package = document;
                    self.installed = true;
                    self.notice.set(if persisted {
                        "Signed package verified and available"
                    } else {
                        "Signed package available in memory"
                    });
                    self.alert("Signed package available");
                    if open {
                        self.open(GUIDE);
                    }
                }
            }
            Err(error) => self.notice.set(error),
        }
    }
    fn key(&mut self, key: u8, n: &mut Network) {
        if self.users.visible() {
            self.users.key(key);
            return;
        }
        if key == 0xab {
            self.magnifier = !self.magnifier;
            self.save_appearance();
            return;
        }
        if key == 0xa2 || key == 0xaa {
            self.search = if self.search.is_some() && self.overview == (key == 0xaa) {
                None
            } else {
                Some(Text::new())
            };
            self.overview = key == 0xaa;
            self.menu = false;
            self.tray = false;
            self.search_row = 0;
            return;
        }
        if (0xa3..=0xa6).contains(&key) {
            self.switch_workspace((key - 0xa3) as usize);
            return;
        }
        if self.search.is_some() {
            match key {
                27 => self.search = None,
                b'\n' => self.search_open(self.search_row, n),
                0x80 => self.search_row = self.search_row.saturating_sub(1),
                0x81 if self.search_row + 1 < self.search_results().len() => self.search_row += 1,
                8 | 32..=126 => {
                    if let Some(search) = &mut self.search {
                        if key == 8 {
                            search.pop();
                        } else {
                            search.push(key);
                        }
                        self.search_row = 0;
                    }
                }
                _ => {}
            }
            return;
        }
        if self.tray {
            match key {
                27 => self.tray = false,
                0x82 => self.select_tray((self.tray_tab + 2) % 3),
                0x83 => self.select_tray((self.tray_tab + 1) % 3),
                0x80 => self.device_offset = self.device_offset.saturating_sub(1),
                0x81 if self.device_offset + 1 < self.devices.len() => self.device_offset += 1,
                b'\n' if self.tray_tab == 0 => self.open(8),
                _ => {}
            }
            return;
        }
        if (0x90..=0x99).contains(&key) {
            self.open((key - 0x90) as usize);
            return;
        }
        if key == 27 {
            self.menu = !self.menu;
            self.launch_index = 0;
            self.restart_armed = false;
            return;
        }
        if self.menu {
            let count = self.launch_ids().count();
            match key {
                b'\n' => {
                    let id = self.launch_ids().nth(self.launch_index);
                    if let Some(id) = id {
                        self.open(id);
                    }
                }
                0x80 => self.launch_index = self.launch_index.saturating_sub(5),
                0x81 => self.launch_index = (self.launch_index + 5).min(count.saturating_sub(1)),
                0x82 => self.launch_index = self.launch_index.saturating_sub(1),
                0x83 | b'\t' => {
                    self.launch_index = (self.launch_index + 1).min(count.saturating_sub(1))
                }
                32..=126 => {
                    let mut query = Text::new();
                    if key != b' ' {
                        query.push(key);
                    }
                    self.open_search(query);
                }
                _ => {}
            }
            return;
        }
        if key == b'\t' {
            for k in 1..=APP_COUNT {
                let id = (self.focus + k) % APP_COUNT;
                if self.visible(id) {
                    self.open(id);
                    break;
                }
            }
            return;
        }
        if self.focus >= APP_COUNT || !self.visible(self.focus) {
            return;
        }
        match self.focus {
            0 => match key {
                b'\n' => self.file_open_selected(),
                0x80 => self.file_select(self.file_cursor.saturating_sub(1)),
                0x81 => self.file_select(self.file_cursor + 1),
                0x84 | 8 | 32..=126 => {
                    match key {
                        0x84 => self.file_search.clear(),
                        8 => self.file_search.pop(),
                        _ => self.file_search.push(key),
                    }
                    self.file_cursor = 0;
                    self.file_preview = None;
                    self.file_select(0);
                }
                _ => {}
            },
            1 => match key {
                0xac => self.save(),
                0xa7 if self.note.len() > 0 => {
                    self.clipboard.retain(|text| text.str() != self.note.str());
                    self.clipboard.insert(0, self.note.clone());
                    self.clipboard.truncate(8);
                    self.notice
                        .set("Copied document. Ctrl+V pastes, Ctrl+Shift+V cycles history.");
                }
                0xa8 | 0xa9 => {
                    if key == 0xa9 && !self.clipboard.is_empty() {
                        self.clipboard.rotate_left(1);
                    }
                    if let Some(text) = self.clipboard.first() {
                        if self.note.len() + text.len() <= storage::CAP {
                            self.note.append(text.str());
                            self.draft_at = Some(hardware::millis() + 750);
                            self.notice.set("Pasted from clipboard");
                        } else {
                            self.notice.set("Paste exceeds the document size limit");
                        }
                    }
                }
                8 | b'\n' | 32..=126 => {
                    let size = self.note.len();
                    if key == 8 {
                        self.note.pop();
                    } else {
                        self.note.push(key);
                    }
                    if size != self.note.len() {
                        self.draft_at = Some(hardware::millis() + 750);
                    }
                }
                _ => (),
            },
            2 => match key {
                8 => self.command.pop(),
                b'\n' => self.execute(n),
                32..=126 => self.command.push(key),
                _ => (),
            },
            3 => match key {
                0x84 => self.url.clear(),
                0xa0 => self.switch_tab((self.tab + 1) % 4, n),
                0xa1 => self.bookmark(),
                8 => self.url.pop(),
                b'\n' => self.navigate(n),
                0x80 => self.scroll_web(-1),
                0x81 => self.scroll_web(1),
                32..=126 => self.url.push(key),
                _ => (),
            },
            4 => match key {
                8 => self.calc.pop(),
                b'\n' => self.calculate(),
                b'0'..=b'9' | b'+' | b'-' | b'*' | b'/' | b'(' | b')' | b' ' => self.calc.push(key),
                _ => (),
            },
            6 => {
                let v = match key {
                    0x80 => (0, -1),
                    0x81 => (0, 1),
                    0x82 => (-1, 0),
                    0x83 => (1, 0),
                    b' ' => {
                        self.snake = Snake::new();
                        return;
                    }
                    _ => (0, 0),
                };
                if v != (0, 0) && v != (-self.snake.dir.0, -self.snake.dir.1) {
                    self.snake.next = v
                }
            }
            8 if !self.users.administrator() => (),
            8 if self.ethernet_tab => self.ethernet.key(key, n),
            8 if self.scanning_results => match key {
                8 => self.scanning_results = false,
                0x80 => self.access_point_offset = self.access_point_offset.saturating_sub(1),
                0x81 if self.access_point_offset + 1 < self.access_points.len() => {
                    self.access_point_offset += 1
                }
                b'\n' => self.select_access_point(self.access_point_offset),
                _ => {}
            },
            8 => match key {
                0x80 => self.wifi_password_field = false,
                0x81 => self.wifi_password_field = true,
                b'\n' if !self.wifi_password_field => self.wifi_password_field = true,
                b'\n' => self.join_wifi(n),
                8 => {
                    if self.wifi_password_field {
                        self.wifi_pass.pop();
                    } else {
                        self.wifi_ssid.pop();
                    }
                }
                32..=126 => {
                    if self.wifi_password_field {
                        self.wifi_pass.push(key);
                    } else {
                        self.wifi_ssid.push(key);
                    }
                }
                _ => {}
            },
            IMAGES => match key {
                0x80 | 0x82 => self.step_image(-1),
                0x81 | 0x83 => self.step_image(1),
                b' ' => self.images.zoom = !self.images.zoom,
                _ => {}
            },
            ORGANIZER => self.organizer.key(key),
            HARDWARE_CENTER => self
                .hardware_center
                .key(key, self.windows[HARDWARE_CENTER].h),
            _ => (),
        }
    }
    fn select_access_point(&mut self, index: usize) {
        if let Some(point) = self.access_points.get(index) {
            self.wifi_ssid = point.ssid;
            self.wifi_pass.bytes.fill(0);
            self.wifi_pass.clear();
            self.wifi_password_field = true;
            self.scanning_results = false;
        }
    }
    fn switch_workspace(&mut self, workspace: usize) {
        self.workspace = workspace;
        self.focus = self
            .order
            .iter()
            .rev()
            .copied()
            .find(|&id| self.visible(id))
            .unwrap_or(APP_COUNT);
        self.menu = false;
        self.tray = false;
    }
    fn select_tray(&mut self, tab: usize) {
        self.tray_tab = tab;
        if tab == 1 && self.devices.is_empty() {
            self.devices = crate::devices::inventory();
        }
    }
    fn step_image(&mut self, direction: isize) {
        let count = self.images.files.len() as isize;
        if count == 0 {
            return;
        }
        self.images
            .select((self.images.selected as isize + direction).rem_euclid(count) as usize);
    }
    fn image_rows(&self) -> (usize, usize) {
        let count = ((self.windows[IMAGES].h - 115) / 34).max(1) as usize;
        (self.images.selected.saturating_sub(count - 1), count)
    }
    fn task_ids(&self) -> impl Iterator<Item = usize> + '_ {
        let show_guide = self.installed && self.windows[GUIDE].open;
        (0..APP_COUNT).filter(move |&id| id != GUIDE || show_guide)
    }
    fn launch_ids(&self) -> impl Iterator<Item = usize> + '_ {
        (0..APP_COUNT).filter(|&id| id != GUIDE || self.installed)
    }
    fn open_search(&mut self, query: Text<64>) {
        self.search = Some(query);
        self.overview = false;
        self.search_row = 0;
        self.menu = false;
        self.tray = false;
    }
    fn save_appearance(&mut self) {
        let accent = PRESETS
            .iter()
            .position(|preset| preset.2 == self.accent)
            .unwrap_or(0);
        let mut value = Text::<16>::new();
        let _ = write!(
            value,
            "{} {} {} {}",
            self.theme,
            self.wallpaper,
            u8::from(self.magnifier),
            accent
        );
        self.settings_notice = if storage::store_file("Appearance.cfg", value.str().as_bytes()) {
            "Appearance saved"
        } else {
            "Applied for this session"
        };
    }
    fn settings_click(&mut self, x: i32, y: i32, w: i32, h: i32) {
        let layout = SettingsLayout::new(w);
        for page in 0..4 {
            if layout.nav(page).has(x, y) {
                self.settings_page = page;
                self.restart_armed = false;
                self.settings_notice = "";
                return;
            }
        }
        self.restart_armed =
            self.restart_armed && self.settings_page == 1 && layout.action(1).has(x, y);
        match self.settings_page {
            0 => {
                for (index, &(_, theme, accent, wallpaper)) in PRESETS.iter().enumerate() {
                    if layout.preset(index).has(x, y) {
                        self.theme = theme;
                        self.accent = accent;
                        self.wallpaper = wallpaper;
                        self.save_appearance();
                        return;
                    }
                }
                for mode in 0..3 {
                    if layout.mode(mode).has(x, y) {
                        self.theme = mode as u8;
                        self.save_appearance();
                        return;
                    }
                }
                for (index, &(_, _, accent, _)) in PRESETS.iter().enumerate() {
                    if layout.swatch(index).has(x, y) {
                        self.accent = accent;
                        self.save_appearance();
                        return;
                    }
                }
            }
            1 if layout.action(0).has(x, y) => {
                self.settings_notice = if storage::sync() {
                    "Files saved to disk"
                } else {
                    "No writable data disk"
                };
            }
            1 if self.users.administrator()
                && layout.action(1).has(x, y)
                && layout.action(1).y + 34 < h =>
            {
                if self.restart_armed {
                    if !self.checkpoint_draft(true) {
                        self.restart_armed = false;
                        self.settings_notice = "The current note could not be saved";
                        return;
                    }
                    hardware::reboot();
                }
                self.restart_armed = true;
                self.settings_notice = "Select Restart again to confirm";
            }
            2 if layout.action(0).has(x, y) => {
                self.magnifier = !self.magnifier;
                self.save_appearance();
            }
            3 if layout.action(0).has(x, y) => self.users.add(),
            3 if layout.action(1).has(x, y) => self.users.change_password(),
            3 if layout.action(2).has(x, y) => {
                if !self.checkpoint_draft(true) {
                    self.settings_notice = "The current note could not be saved";
                    return;
                }
                crate::audio::stop();
                self.users.lock();
            }
            _ => {}
        }
    }
    fn draw_settings(&self, s: &mut Screen, n: &Network, x: i32, y: i32, w: i32, h: i32) {
        let layout = SettingsLayout::new(w);
        if layout.wide {
            s.rect(x + 1, y + 38, 158, h - 39, 0xe4edf7);
            s.text(x + 24, y + 62, "Preferences", MUTED, 1);
        }
        for (index, name) in ["Appearance", "System", "Accessibility", "Accounts"]
            .iter()
            .enumerate()
        {
            let nav = layout.nav(index);
            let active = index == self.settings_page;
            s.rounded(
                x + nav.x,
                y + nav.y,
                nav.w,
                nav.h,
                8,
                if active { 0xcfdef4 } else { 0xe9f0f8 },
                255,
            );
            s.centered(
                x + nav.x,
                y + nav.y,
                nav.w,
                nav.h,
                clip_label(name, nav.w - 8, 1),
                INK,
                1,
            );
        }
        let left = x + layout.x;
        let top = y + layout.y;
        match self.settings_page {
            0 => {
                s.text(left, top, "Appearance", INK, 2);
                s.text(left, top + 40, "Themes", MUTED, 1);
                for (index, &(name, _, accent, wallpaper)) in PRESETS.iter().enumerate() {
                    let r = layout.preset(index);
                    let [base, sky, core, rim] = crate::graphics::DESK_PALETTES[wallpaper as usize];
                    let selected = self.wallpaper == wallpaper;
                    s.rounded(
                        x + r.x,
                        y + r.y,
                        r.w,
                        r.h,
                        9,
                        if selected { self.accent } else { 0xe0ebf5 },
                        255,
                    );
                    s.rounded(x + r.x + 2, y + r.y + 2, r.w - 4, r.h - 4, 7, 0xf3f7fa, 255);
                    s.rounded(x + r.x + 6, y + r.y + 6, r.w - 12, 37, 5, sky, 255);
                    let size = (r.w - 18).min(35);
                    s.rounded(x + r.x + 9, y + r.y + 13, size, 24, 12, core, 255);
                    s.rounded(x + r.x + r.w - 22, y + r.y + 10, 14, 15, 7, rim, 255);
                    s.rect(
                        x + r.x + 8,
                        y + r.y + 44,
                        r.w - 16,
                        3,
                        if selected { accent } else { base },
                    );
                    s.text(
                        x + r.x + 7,
                        y + r.y + 52,
                        clip_label(name, r.w - 12, 1),
                        INK,
                        1,
                    );
                }
                s.text(left, top + 154, "Color mode", MUTED, 1);
                for (index, name) in ["Light", "Dark", "Contrast"].iter().enumerate() {
                    let r = layout.mode(index);
                    button(
                        s,
                        x + r.x,
                        y + r.y,
                        r.w,
                        clip_label(name, r.w - 6, 1),
                        if self.theme == index as u8 {
                            self.accent
                        } else {
                            SURFACE
                        },
                    );
                }
                s.text(left, top + 224, "Accent color", MUTED, 1);
                for (index, &(_, _, color, _)) in PRESETS.iter().enumerate() {
                    let r = layout.swatch(index);
                    s.rounded(
                        x + r.x,
                        y + r.y,
                        r.w,
                        r.h,
                        11,
                        if self.accent == color { INK } else { SURFACE },
                        255,
                    );
                    s.rounded(x + r.x + 3, y + r.y + 3, r.w - 6, r.h - 6, 9, color, 255);
                }
            }
            1 => {
                s.text(left, top, "System", INK, 2);
                let (used, capacity) = crate::memory::usage();
                let mut metrics = Text::<250>::new();
                let _ = write!(
                    metrics,
                    "Display   {} x {}\nStorage   {} / {}\nNetwork   {}\nKernel heap   {} / {} MiB",
                    s.w,
                    s.h,
                    storage::filesystem(),
                    storage::backend(),
                    n.adapter_name(),
                    used / 1048576,
                    capacity / 1048576
                );
                s.wrapped(left, top + 43, layout.width, 104, metrics.str(), INK);
                for (index, name) in [
                    "Save files",
                    if self.restart_armed {
                        "Confirm restart"
                    } else {
                        "Restart KreativeOS"
                    },
                ]
                .iter()
                .enumerate()
                {
                    if index == 1 && !self.users.administrator() {
                        continue;
                    }
                    let r = layout.action(index);
                    if r.y + r.h < h {
                        button(
                            s,
                            x + r.x,
                            y + r.y,
                            r.w,
                            name,
                            if self.restart_armed && index == 1 {
                                0xa6414b
                            } else {
                                SURFACE
                            },
                        );
                    }
                }
                if self.users.system() {
                    let mut summary = Text::<240>::new();
                    crate::system1::status(&mut summary);
                    s.wrapped_small(
                        left,
                        top + 245,
                        layout.width,
                        (h - layout.y - 275).max(0),
                        summary.str(),
                        INK,
                    );
                }
            }
            2 => {
                s.text(left, top, "Accessibility", INK, 2);
                s.wrapped(left, top + 45, layout.width, 85, "Magnify the area around the pointer. Press Ctrl+M to toggle while using other apps.", MUTED);
                let r = layout.action(0);
                button(
                    s,
                    x + r.x,
                    y + r.y,
                    r.w,
                    if self.magnifier {
                        "Turn magnifier off"
                    } else {
                        "Turn magnifier on"
                    },
                    if self.magnifier { self.accent } else { SURFACE },
                );
            }
            _ => {
                s.text(left, top, "Accounts", INK, 2);
                s.text(left, top + 44, self.users.name(), INK, 2);
                let admin = self.users.administrator();
                s.text(
                    left,
                    top + 73,
                    if self.users.system() {
                        "System maintenance"
                    } else if admin {
                        "Administrator"
                    } else {
                        "Standard account"
                    },
                    MUTED,
                    1,
                );
                for (index, title) in ["Add account", "Change password", "Sign out"]
                    .into_iter()
                    .enumerate()
                {
                    if index == 0 && !admin {
                        continue;
                    }
                    let r = layout.action(index);
                    button(s, x + r.x, y + r.y, r.w, title, SURFACE);
                }
            }
        }
        s.text(
            left,
            y + h - 30,
            clip_label(self.settings_notice, layout.width, 1),
            self.accent,
            1,
        );
    }
    fn calculate(&mut self) {
        self.answer.clear();
        match calculate(self.calc.str()) {
            Ok(v) => {
                let _ = write!(self.answer, "{}", v);
            }
            Err(e) => self.answer.set(e),
        }
    }
    fn execute(&mut self, n: &mut Network) {
        let command = self.command;
        self.command.clear();
        self.execute_line(command.str(), n, true);
    }
    fn execute_line(&mut self, line: &str, n: &mut Network, allow_script: bool) {
        if self.terminal.len > 3000 {
            self.terminal.clear()
        }
        self.terminal.append("\n> ");
        self.terminal.append(line);
        self.terminal.push(b'\n');
        let (verb, arg) = line.trim().split_once(' ').unwrap_or((line.trim(), ""));
        if matches!(verb, "service" | "scrub" | "memory") {
            if self.users.system() && allow_script {
                crate::system1::command(verb, arg, &mut self.terminal);
            } else {
                self.terminal
                    .append("An interactive system1 session is required");
            }
            return;
        }
        if !self.users.administrator()
            && matches!(
                verb,
                "accounts" | "dev" | "snapshot" | "reboot" | "dhcp" | "static"
            )
        {
            self.terminal.append("Administrator access required");
            return;
        }
        match verb{
 "help"=>self.terminal.append("help | ls | cat NAME | touch NAME | rm NAME\nedit NAME | save | clear | open APP\nbrowse URL | net | uptime | calc EXPR\ndhcp [VLAN] | static IP MASK ROUTER DNS [VLAN]\nziplist NAME | unzip NAME | run NAME\ndev list | dev import | exec NAME.kr3\naccounts | lock | sdk | install NAME.kapp | snapshot | reboot\n"),
 "ls"=>unsafe{for f in &storage::FILES{if let Some(name)=storage::visible_name(f.name.str()).filter(|_| f.used){let _=writeln!(self.terminal,"{name}  {} bytes",f.data.len());}}},
 "lsdir" => {
    for folder in storage::folders() { let _ = writeln!(self.terminal, "{}/", folder.str()); }
 },
 "mkdir" => {
    self.terminal.append(match storage::mkdir(arg) { Ok(()) => "Folder created", Err(error) => error });
 },
 "rmdir" => {
    self.terminal.append(match storage::rmdir(arg) { Ok(()) => "Folder removed", Err(error) => error });
 },
 "cp" => {
    if let Some((source, destination)) = arg.split_once(' ') {
        if let Some(index) = storage::find(source) {
            let bytes = unsafe { storage::FILES[index].data.bytes.clone() };
            self.terminal.append(if storage::store_file(destination.trim(), &bytes) { "File copied" } else { "Could not save the copy" });
        } else { self.terminal.append("Source file not found"); }
    } else { self.terminal.append("Use cp SOURCE DESTINATION"); }
 },
 "cat"=>{if let Some(i)=storage::find(arg){unsafe{self.terminal.append(storage::FILES[i].data.str())}}else{self.terminal.append("File not found")}},
 "touch" if storage::find(arg).is_some() => self.terminal.append("File already exists"),
 "touch"=>{match storage::put(arg,b""){Ok(())=>{self.terminal.append(if storage::sync(){"Created on disk"}else{"Created in memory"})},Err(e)=>self.terminal.append(e)}},
 "rm"=>{if storage::remove(arg){self.terminal.append(if storage::sync(){"Removed from disk"}else{"Removed from memory"})}else{self.terminal.append("File not found")}},
 "edit"=>{if arg.is_empty() || arg.len() > storage::PATH_CAP || !arg.is_ascii() || arg.contains('\\') {
   self.terminal.append("Enter a valid file name");
 } else {
   if !self.checkpoint_draft(true) {
       self.terminal.append("The current note could not be saved");
       return;
   }
   self.filename.set(arg);self.note.clear();if let Some(i)=storage::find(arg){unsafe{self.note=storage::FILES[i].data.clone()}}self.open(1)
 }},
 "save"=>{self.save();self.terminal.append(self.notice.str())},"clear"=>self.terminal.clear(),
 "open"=>{if let Some(i)=NAMES.iter().position(|s|s.eq_ignore_ascii_case(arg)){self.open(i)}else{self.terminal.append("Unknown app")}},
 "browse"=>{self.url.set(arg);self.navigate(n);self.open(3)},
 "net"=>{ ethernet_details(&mut self.terminal, n); },
 "dhcp"=>{let vlan = if arg.is_empty() { Some(0) } else { arg.parse().ok() };
   self.terminal.append(if vlan.is_some_and(|tag| n.start_dhcp(tag)) { "DHCP discovery started" } else { "Use dhcp [VLAN] with idle Ethernet" });},
 "static"=>{let mut p = arg.split_ascii_whitespace(); let parsed = (|| {
   let config = crate::dhcp::Config { ip: crate::dhcp::address(p.next()?)?, mask: crate::dhcp::address(p.next()?)?,
       router: crate::dhcp::address(p.next()?)?, dns: crate::dhcp::address(p.next()?)? };
   let vlan = p.next().unwrap_or("0").parse().ok()?;
   p.next().is_none().then_some((config,vlan)) })();
   self.terminal.append(if parsed.is_some_and(|(config,vlan)| n.static_config(config,vlan)) { "Static IPv4 applied" } else { "Use static IP MASK ROUTER DNS [VLAN] with idle Ethernet" });},
 "uptime"=>{let _=write!(self.terminal,"{} seconds",hardware::millis()/1000);},
 "calc"=>{self.calc.set(arg);self.calculate();self.terminal.append(self.answer.str())},
 "install"=>{if let Some(index) = crate::packages::CATALOG.iter().position(|p| p.source == arg) { self.package_install(index, false); self.terminal.append(self.notice.str()); } else { self.terminal.append("Package is not in the signed catalog"); }},
 "sdk"=>self.package_install(1, true),
 "dev"=>crate::developer::command(arg, &mut self.terminal),
 "accounts" if allow_script=>self.users.manage(arg, &mut self.terminal),
 "system1" if allow_script && arg.is_empty() => {
     if !self.checkpoint_draft(true) {
         self.terminal.append("The current note could not be saved");
         return;
     }
     if !self.users.enter_system() {
         self.terminal.append("A signed-in administrator must open system1");
     }
 },
 "passwd" if allow_script => self.users.change_password(),
 "lock" | "logout" if allow_script=>{
     if !self.checkpoint_draft(true) {
         self.terminal.append("The current note could not be saved");
         return;
     }
     if !self.users.lock() { self.terminal.append("Run accounts to enable sign-in"); }
 },
 "exec" if allow_script => {
     if let Some(index) = storage::find(arg) {
         let binary = unsafe { &storage::FILES[index].data };
         match self.scheduler.spawn(arg, &binary.bytes[..binary.len()]) {
             Ok(pid) => { let _ = writeln!(self.terminal, "Started process {pid}"); }
             Err(error) => self.terminal.append(error),
         }
     } else {
         self.terminal.append("User executable was not found");
     }
 },
 "ps" => self.scheduler.list(&mut self.terminal),
 "kill" if allow_script => {
     self.terminal.append(if arg.parse().ok().is_some_and(|pid| self.scheduler.kill(pid)) {
         "Process terminated"
     } else { "Process not found" });
 },
 "ziplist"=>self.archive(arg, false),
 "unzip"=>self.archive(arg, true),
 "run" if allow_script => {
   if let Some(index) = storage::find(arg) {
       let script = unsafe { storage::FILES[index].data.clone() };
       let commands = script.str().lines().map(str::trim).filter(|line| !line.is_empty() && !line.starts_with('#'));
       for command in commands.take(32) { self.execute_line(command, n, false); }
   } else { self.terminal.append("Script was not found"); }
 },
 "run"=>self.terminal.append("Nested scripts are unavailable"),
 "snapshot" if arg.is_empty()=> {
   if let Some(generation) = storage::previous_generation() {
       let _ = write!(self.terminal, "Previous file snapshot: {generation}\nTo restore it and restart, type snapshot restore {generation}");
   } else { self.terminal.append("No previous file snapshot available"); }
 },
 "snapshot" if allow_script => {
   let generation = arg.strip_prefix("restore ").and_then(|text| text.parse::<u32>().ok());
   if generation.is_some_and(storage::restore_previous) {
       unsafe { hardware::log(b"KreativeOS: previous file snapshot restored\r\n") };
       hardware::reboot();
   }
   self.terminal.append("Snapshot restore failed. Type snapshot to inspect the previous version.");
 },
 "reboot"=>{if self.checkpoint_draft(true) { hardware::reboot() } self.terminal.append("The current note could not be saved");},
 "discard" if allow_script && arg == "note" => {
     self.draft_at = None;
     storage::clear_saved_draft(self.filename.str());
     self.note = storage::find(self.filename.str())
         .map(|index| unsafe { storage::FILES[index].data.clone() }).unwrap_or_default();
     self.notice.set("Unsaved changes discarded");
     self.terminal.append("Unsaved note changes discarded");
 },
 ""=>(),_=>self.terminal.append("Unknown command. Type help.")}
    }
    fn pointer_hover(&self, w: i32, h: i32) -> Option<(usize, i32)> {
        if self.menu {
            let grid = AppGrid::new(w, h);
            return (0..self.launch_ids().count())
                .find(|&index| grid.item(index).has(self.mx, self.my))
                .map(|index| (APP_COUNT, index as i32));
        }
        let dock = Dock::new(w, h);
        if dock.body.has(self.mx, self.my) {
            let index = if dock.start.has(self.mx, self.my) {
                -1
            } else if dock.status.has(self.mx, self.my) {
                -2
            } else {
                (0..self.task_ids().count())
                    .find(|&n| dock.slot(n).has(self.mx, self.my))
                    .map_or(-3, |n| n as i32)
            };
            return Some((APP_COUNT + 1, index));
        }
        for &id in self.order.iter().rev() {
            let win = self.windows[id];
            if self.visible(id) && win.inside(self.mx, self.my) {
                if self.my < win.y + 38 && self.mx >= win.x + win.w - 114 {
                    return Some((id, (self.mx - (win.x + win.w - 114)) / 38));
                }
                break;
            }
        }
        None
    }
    fn mouse(&mut self, dx: i32, dy: i32, buttons: u8, wheel: i32, s: &Screen, n: &mut Network) {
        self.mx = (self.mx + dx).clamp(0, s.w as i32 - 1);
        self.my = (self.my + dy).clamp(0, s.h as i32 - 1);
        if self.users.visible() {
            let pressed = buttons & 1 != 0 && self.buttons & 1 == 0;
            self.buttons = buttons;
            self.drag = None;
            self.painting = false;
            if pressed {
                self.users.click(self.mx, self.my, s.w as i32, s.h as i32);
            }
            return;
        }
        if wheel != 0 && self.focus < APP_COUNT && self.visible(self.focus) {
            let focused = self.windows[self.focus];
            if focused.open && !focused.min && focused.inside(self.mx, self.my) {
                match self.focus {
                    0 => self.file_select(if wheel > 0 {
                        self.file_cursor.saturating_sub(1)
                    } else {
                        self.file_cursor + 1
                    }),
                    3 => self.scroll_web(-i32::from(wheel)),
                    IMAGES => self.step_image(if wheel > 0 { -1 } else { 1 }),
                    HARDWARE_CENTER => self.hardware_center.scroll(
                        if wheel > 0 { -1 } else { 1 },
                        crate::hardware_center::listing_rows(focused.h),
                    ),
                    _ => {}
                }
            }
        }
        let click = buttons & 1 != 0 && self.buttons & 1 == 0;
        self.buttons = buttons;
        if buttons & 1 == 0 {
            if let Some((id, _, _)) = self.drag {
                let width = s.w as i32;
                let left = self.mx < 12;
                if left || self.mx >= width - 12 {
                    let window = &mut self.windows[id];
                    if self.restore[id].is_none() {
                        self.restore[id] = Some(*window);
                    }
                    window.w = width / 2;
                    window.x = if left { 0 } else { width - window.w };
                    window.y = 0;
                    window.h = s.h as i32 - BAR;
                }
            }
            self.drag = None;
            self.last_paint = None;
            self.painting = false;
        }
        if let Some((id, ox, oy)) = self.drag {
            let window = &mut self.windows[id];
            window.x = (self.mx - ox).clamp(0, (s.w as i32 - window.w).max(0));
            window.y = (self.my - oy).clamp(0, (s.h as i32 - BAR - window.h).max(0));
            return;
        }
        if click {
            let x = self.mx;
            let y = self.my;
            if self.search.is_some() {
                let left = s.w as i32 / 2 - 236;
                if (left..left + 472).contains(&x) && (131..371).contains(&y) {
                    self.search_open(((y - 131) / 30) as usize, n);
                } else if y < 68 || y >= 389 || x < left || x >= left + 472 {
                    self.search = None;
                }
                return;
            }
            let dock = Dock::new(s.w as i32, s.h as i32);
            if dock.body.has(x, y) {
                let app = self
                    .task_ids()
                    .enumerate()
                    .find_map(|(slot, id)| dock.slot(slot).has(x, y).then_some(id));
                self.restart_armed = false;
                if dock.status.has(x, y) {
                    self.tray = !self.tray;
                    self.menu = false;
                } else if dock.start.has(x, y) {
                    self.tray = false;
                    self.menu = !self.menu;
                    self.launch_index = 0;
                } else if let Some(id) = app {
                    if self.focus == id && self.visible(id) {
                        self.windows[id].min = true;
                    } else {
                        self.open(id);
                    }
                }
                return;
            }
            if self.tray {
                let left = s.w as i32 - 406;
                let top = s.h as i32 - BAR - 304;
                if (left..left + 394).contains(&x) && (top..top + 292).contains(&y) {
                    let px = x - left;
                    let py = y - top;
                    if (12..36).contains(&py) && (12..372).contains(&px) {
                        self.select_tray(((px - 12) / 120) as usize);
                    } else if self.tray_tab == 0 && (211..247).contains(&py) {
                        if px < 132 {
                            self.ethernet_tab = false;
                            self.open(8);
                        } else if px < 252 {
                            if self.users.administrator() {
                                self.notice.set(if n.start_dhcp(n.vlan) {
                                    "DHCP discovery started"
                                } else {
                                    "Ethernet is unavailable or busy"
                                });
                            }
                        } else {
                            crate::audio::volume(-4);
                        }
                    } else if self.tray_tab == 0 && (250..282).contains(&py) && px >= 252 {
                        crate::audio::volume(4);
                    } else if self.tray_tab == 1 && py >= 250 {
                        if px < 140 {
                            self.device_offset = self.device_offset.saturating_sub(8);
                        } else if px < 260 && self.device_offset + 8 < self.devices.len() {
                            self.device_offset += 8;
                        } else if px >= 260 {
                            self.devices = crate::devices::inventory();
                            self.device_offset = 0;
                        }
                    }
                    return;
                }
                self.tray = false;
                return;
            }
            if self.menu {
                let grid = AppGrid::new(s.w as i32, s.h as i32);
                let target = self
                    .launch_ids()
                    .enumerate()
                    .find_map(|(position, id)| grid.item(position).has(x, y).then_some(id));
                if let Some(id) = target {
                    self.open(id);
                } else if grid.search.has(x, y) {
                    self.open_search(Text::new());
                } else if !grid.body.has(x, y) {
                    self.menu = false;
                }
                return;
            }
            let width = s.w as i32;
            if (width - 194..width - 18).contains(&x)
                && (10..38).contains(&y)
                && !self
                    .order
                    .iter()
                    .any(|&id| self.visible(id) && self.windows[id].inside(x, y))
            {
                self.switch_workspace(((x - (width - 194)) / 44) as usize);
                return;
            }
            if Box2::at(18, 10, 156, 32).has(x, y)
                && !self
                    .order
                    .iter()
                    .any(|&id| self.visible(id) && self.windows[id].inside(x, y))
            {
                self.open(9);
                self.settings_page = 3;
                self.settings_notice = "";
                self.restart_armed = false;
                return;
            }
            for j in (0..APP_COUNT).rev() {
                let id = self.order[j];
                let w = self.windows[id];
                if !self.visible(id) || !w.inside(x, y) {
                    continue;
                }
                self.open(id);
                let lx = x - w.x;
                let ly = y - w.y;
                if ly < 38 {
                    if lx >= w.w - 38 {
                        self.restart_armed = false;
                        self.windows[id].open = false;
                        if id == 3 {
                            crate::audio::stop();
                        }
                    } else if (w.w - 114..w.w - 76).contains(&lx) {
                        self.restart_armed = false;
                        self.windows[id].min = true
                    } else if (w.w - 76..w.w - 38).contains(&lx) {
                        if let Some(old) = self.restore[id].take() {
                            self.windows[id] = old;
                        } else {
                            self.restore[id] = Some(w);
                            self.windows[id] = Window {
                                open: true,
                                min: false,
                                x: 24,
                                y: 46,
                                w: s.w as i32 - 48,
                                h: s.h as i32 - 112,
                            };
                        }
                    } else {
                        self.drag = Some((id, lx, ly))
                    }
                    return;
                }
                self.painting = id == 5 && lx >= 20 && lx < 660 && ly >= 100 && ly < 460;
                self.app_click(id, lx, ly, n);
                break;
            }
        }
        if buttons & 1 != 0 && self.painting && self.focus == 5 {
            let w = self.windows[5];
            if w.open && !w.min {
                let x = (self.mx - w.x - 20) / 2;
                let y = (self.my - w.y - 100) / 2;
                if self.mx >= w.x + 20 && self.my >= w.y + 100 && x < 320 && y < 180 {
                    if let Some((a, b)) = self.last_paint {
                        paint_line(a, b, x, y, self.paint_color)
                    } else {
                        paint_line(x, y, x, y, self.paint_color)
                    }
                    self.last_paint = Some((x, y));
                } else {
                    self.last_paint = None;
                }
            }
        }
    }
    fn app_click(&mut self, id: usize, x: i32, y: i32, n: &mut Network) {
        match id {
            ORGANIZER => self.organizer.click(x, y),
            HARDWARE_CENTER => {
                self.hardware_center
                    .click(x, y, self.windows[id].w, self.windows[id].h, n)
            }
            0 => {
                if x < 142 && (89..159).contains(&y) {
                    self.file_images = y >= 124;
                    self.file_folder.clear();
                    self.file_cursor = 0;
                    self.file_preview = None;
                    self.file_select(0);
                } else if Box2::at(163, 48, 80, 34).has(x, y) {
                    self.file_open_selected();
                } else if x >= 154 && y >= 126 {
                    let (first, count) = self.file_viewport();
                    let row = ((y - 126) / 36) as usize;
                    if row < count
                        && x < if self.windows[0].w >= 480 {
                            335
                        } else {
                            self.windows[0].w
                        }
                    {
                        self.file_select(first + row);
                    }
                }
            }
            1 => {
                if Box2::at(16, 48, 72, 34).has(x, y) {
                    self.save()
                } else if Box2::at(96, 48, 72, 34).has(x, y) {
                    if !self.checkpoint_draft(true) {
                        return;
                    }
                    self.filename.set("Untitled.txt");
                    self.note.clear();
                    self.notice.clear()
                }
            }
            3 => {
                let w = self.windows[id];
                if (88..113).contains(&y) && x >= 14 && x < w.w - 14 {
                    match ((x - 14) * 7 / (w.w - 28)) as usize {
                        tab @ 0..=3 => self.switch_tab(tab, n),
                        4 => self.bookmark(),
                        5 => {
                            self.web_list = (self.web_list + 1) % 4;
                            self.web_list_offset = 0;
                        }
                        6 => self.save_download(),
                        _ => {}
                    }
                } else if self.web_list != 0 && x >= 14 && y >= 148 {
                    let row = ((y - 148) / 23) as usize;
                    if row < Self::web_rows(w.h) && n.state == 0 && !n.tls_pending {
                        if let Some((_, url)) = self.web_row(self.web_list_offset + row) {
                            self.url = url;
                            self.navigate(n);
                        }
                    }
                } else if (48..84).contains(&y) && x > w.w - 90 {
                    self.navigate(n);
                } else if (48..84).contains(&y) && x >= 14 && x < w.w - 108 {
                    self.url.clear();
                } else if y >= w.h - 44
                    && x >= w.w - 160
                    && (self.audio_url.is_some() || !self.audio_clip.is_empty())
                {
                    self.play_audio(n);
                }
            }
            4 => {
                let window = self.windows[id];
                if let Some(key) =
                    b"789/456*123-C0=+"
                        .iter()
                        .enumerate()
                        .find_map(|(index, &key)| {
                            calculator_key(window.w, window.h, index)
                                .has(x, y)
                                .then_some(key)
                        })
                {
                    if key == b'C' {
                        self.calc.clear();
                        self.answer.clear()
                    } else if key == b'=' {
                        self.calculate()
                    } else {
                        self.calc.push(key)
                    }
                }
            }
            5 => {
                if y < 90 {
                    let colors = [
                        0x176f80, 0x181f30, 0xe15f63, 0xeac75b, 0x51c29b, 0x759fe8, 0xf2f0e9,
                    ];
                    if x >= 20 && x < 300 {
                        self.paint_color = colors[((x - 20) / 40) as usize]
                    } else if x > 350 {
                        unsafe { PAINT.fill(0xf2f0e9) }
                    }
                }
            }
            6 => {
                if y < 90 {
                    self.snake = Snake::new()
                }
            }
            7 => {
                if (138..246).contains(&y) && (24..340).contains(&x) {
                    self.package_install(((y - 138) / 54) as usize, true);
                }
            }
            8 => {
                if (46..80).contains(&y) && (24..246).contains(&x) {
                    self.ethernet_tab = x < 136;
                } else if !self.users.administrator() {
                    self.wifi_status = "Network configuration requires an administrator";
                } else if self.ethernet_tab {
                    self.ethernet.click(x, y, self.windows[8].w, n);
                } else if self.scanning_results {
                    if (46..80).contains(&y) && (254..366).contains(&x) {
                        self.scanning_results = false;
                    } else if (46..80).contains(&y) && (374..486).contains(&x) {
                        self.scan_wifi(n);
                    } else if y >= 127 && x >= 24 {
                        self.select_access_point(
                            self.access_point_offset + ((y - 127) / 29) as usize,
                        );
                    }
                } else if (46..80).contains(&y) && (254..366).contains(&x) {
                    self.scan_wifi(n);
                } else if (177..212).contains(&y) {
                    self.wifi_password_field = false;
                } else if (247..282).contains(&y) {
                    self.wifi_password_field = true;
                } else if (295..336).contains(&y) {
                    if x < 138 {
                        self.join_wifi(n);
                    } else if (148..258).contains(&x) {
                        self.view_saved_wifi();
                    } else if (268..378).contains(&x) {
                        self.wifi_status = n
                            .wifi
                            .disconnect()
                            .map(|()| "Wi-Fi disconnected")
                            .unwrap_or_else(|error| error);
                        self.alert(self.wifi_status);
                    }
                } else if (378..413).contains(&y) {
                    if (24..155).contains(&x) {
                        self.hotspot(n, false);
                    }
                    if (165..296).contains(&x) {
                        self.hotspot(n, true);
                    }
                }
            }
            9 => self.settings_click(x, y, self.windows[id].w, self.windows[id].h),
            10 => {
                if y < 90 {
                    self.ttt = [0; 9];
                    self.turn = 1;
                    self.winner = 0
                } else if x >= 40 && x < 340 && y >= 110 && y < 410 && self.winner == 0 {
                    let cell = ((y - 110) / 100 * 3 + (x - 40) / 100) as usize;
                    if self.ttt[cell] == 0 {
                        self.ttt[cell] = self.turn;
                        self.turn = 3 - self.turn;
                        for [a, b, c] in [
                            [0, 1, 2],
                            [3, 4, 5],
                            [6, 7, 8],
                            [0, 3, 6],
                            [1, 4, 7],
                            [2, 5, 8],
                            [0, 4, 8],
                            [2, 4, 6],
                        ] {
                            if self.ttt[a] != 0
                                && self.ttt[a] == self.ttt[b]
                                && self.ttt[a] == self.ttt[c]
                            {
                                self.winner = self.ttt[a]
                            }
                        }
                        if self.winner == 0 && self.ttt.iter().all(|&v| v != 0) {
                            self.winner = 3
                        }
                    }
                }
            }
            IMAGES => {
                if (49..84).contains(&y) && x >= self.windows[id].w - 106 {
                    self.images.zoom = !self.images.zoom;
                } else if (82..self.windows[id].h - 24).contains(&y) && (14..174).contains(&x) {
                    let (first, count) = self.image_rows();
                    let relative = ((y - 82) / 34) as usize;
                    if relative < count && first + relative < self.images.files.len() {
                        self.images.select(first + relative);
                    }
                }
            }
            _ => (),
        }
    }
    fn draw(&self, s: &mut Screen, n: &Network) {
        if self.users.visible() {
            s.theme = 0;
            self.users.draw(s);
            return;
        }
        let w = s.w as i32;
        let h = s.h as i32;
        s.wallpaper_variant((self.wallpaper as usize + self.workspace) % 4);
        s.theme = self.theme;
        Box2::at(18, 10, 156, 32).glass(s, 13, 0xe8f1fa, 225);
        s.symbol(29, 16, 15, self.accent);
        s.text(55, 17, clip_label(self.users.name(), 108, 1), INK, 1);
        Box2::at(w - 198, 6, 184, 36).glass(s, 13, 0xe8f1fa, 220);
        for (i, label) in ["1", "2", "3", "4"].iter().enumerate() {
            let xx = w - 194 + i as i32 * 44;
            let selected = i == self.workspace;
            s.rounded(
                xx,
                10,
                40,
                28,
                8,
                if selected { self.accent } else { 0xdce8f5 },
                255,
            );
            s.centered(
                xx,
                10,
                40,
                28,
                label,
                if selected { 0xffffff } else { INK },
                1,
            );
        }
        for &id in &self.order {
            let window = self.windows[id];
            if self.visible(id) {
                self.window(s, n, id, window);
            }
        }

        let dock = Dock::new(w, h);
        let bar_y = dock.body.y;
        s.shadow(dock.body.x, bar_y, dock.body.w, dock.body.h);
        dock.body.glass(s, 15, 0xf1f7fe, 240);
        dock.start
            .glass(s, 11, if self.menu { 0xc9e0f4 } else { 0xdceaf7 }, 255);
        s.symbol(dock.start.x + 7, dock.start.y + 8, 15, self.accent);
        s.text(dock.start.x + 30, dock.start.y + 9, "Apps", INK, 1);
        for (slot, id) in self.task_ids().enumerate() {
            let r = dock.slot(slot);
            let selected = self.focus == id && self.visible(id);
            let hover = r.has(self.mx, self.my);
            let xx = r.x + (r.w - 26) / 2;
            let style = if selected || hover {
                0xbfd7ef
            } else {
                0xe2edf8
            };
            Box2::at(xx, r.y + 3, 26, 26).glass(s, 8, style, 255);
            s.symbol(
                xx + 3,
                r.y + 6,
                id,
                if selected { self.accent } else { 0x23435f },
            );
            if self.windows[id].open {
                s.rounded(
                    r.x + r.w / 2 - 4,
                    r.y + 32,
                    8,
                    2,
                    1,
                    if selected { self.accent } else { 0x718da9 },
                    255,
                );
            }
            if hover && !self.menu && !self.tray {
                let width = (crate::graphics::text_width(NAMES[id], 1) + 24).min(w - 8);
                let x = (r.x + (r.w - width) / 2).clamp(4, w - width - 4);
                s.rounded(x, bar_y - 34, width, 27, 7, 0xf3f8ff, 255);
                s.centered(x, bar_y - 34, width, 27, NAMES[id], INK, 1);
            }
        }
        let (hh, mm, _) = hardware::clock();
        let mut clock: Text<12> = Text::new();
        let _ = write!(clock, "{:02}:{:02}", hh, mm);
        dock.status
            .glass(s, 10, if self.tray { 0xc9e0f4 } else { 0xdceaf7 }, 255);
        s.centered(
            dock.status.x,
            dock.status.y - 1,
            dock.status.w,
            22,
            clock.str(),
            INK,
            1,
        );
        s.centered(
            dock.status.x,
            dock.status.y + 17,
            dock.status.w,
            17,
            if n.wifi.joined {
                "Wi-Fi"
            } else if n.link_speed().is_some() && n.config.ip != [0; 4] {
                "Ethernet"
            } else if n.link_speed().is_some() {
                "No IPv4"
            } else {
                "Offline"
            },
            MUTED,
            1,
        );
        if self.tray {
            self.draw_tray(s, n, w - 406, bar_y - 304);
        }
        if let Some(query) = self.search {
            let left = w / 2 - 236;
            s.shadow(left, 68, 472, 321);
            s.rounded(left, 68, 472, 321, 9, PANEL, 255);
            s.rounded(left + 12, 80, 448, 39, 6, 0xffffff, 255);
            let label = if query.len == 0 {
                if self.overview {
                    "Find an open window"
                } else {
                    "Search apps, files and browser history"
                }
            } else {
                query.str()
            };
            s.text(left + 24, 92, clip_label(label, 420, 1), INK, 1);
            for (row, (label, _)) in self.search_results().iter().enumerate() {
                let top = 131 + row as i32 * 30;
                if self.search_row == row {
                    s.rounded(left + 12, top, 448, 27, 5, 0xc4dfcf, 255);
                }
                s.text(left + 24, top + 4, clip_label(label.str(), 420, 1), INK, 1);
            }
        }

        if self.menu {
            let grid = AppGrid::new(w, h);
            let b = grid.body;
            s.shadow(b.x, b.y, b.w, b.h);
            b.glass(s, 20, 0xf2f7fe, 243);
            s.text(b.x + 24, b.y + 21, "Applications", INK, 2);
            grid.search.glass(s, 11, 0xe0edf9, 255);
            s.centered(
                grid.search.x,
                grid.search.y,
                grid.search.w,
                grid.search.h,
                "Search workspace",
                INK,
                1,
            );
            for (index, id) in self.launch_ids().enumerate() {
                let item = grid.item(index);
                let active = index == self.launch_index || item.has(self.mx, self.my);
                item.glass(s, 12, if active { 0xcbdef4 } else { 0xe7eff9 }, 255);
                let icon = Box2::at(item.x + (item.w - 37) / 2, item.y + 8, 37, 34);
                icon.glass(s, 10, APP_COLORS[id], 255);
                s.symbol(icon.x + 8, icon.y + 7, id, 0x192f47);
                s.centered(
                    item.x,
                    item.y + 44,
                    item.w,
                    24,
                    clip_label(NAMES[id], item.w - 5, 1),
                    INK,
                    1,
                );
            }
        }
        s.theme = 0;
        if self.magnifier {
            s.magnify(self.mx, self.my);
        }
    }
    fn draw_tray(&self, s: &mut Screen, n: &Network, x: i32, y: i32) {
        s.shadow(x, y, 394, 292);
        s.rounded(x, y, 394, 292, 9, PANEL, 255);
        for (index, label) in ["Controls", "Devices", "Activity"].iter().enumerate() {
            let left = x + 12 + index as i32 * 120;
            s.rounded(
                left,
                y + 12,
                112,
                24,
                5,
                if self.tray_tab == index {
                    0xc4dfcf
                } else {
                    SURFACE
                },
                255,
            );
            s.centered(left, y + 12, 112, 24, label, INK, 1);
        }
        match self.tray_tab {
            0 => {
                let mut details = Text::<300>::new();
                ethernet_details(&mut details, n);
                s.wrapped(x + 16, y + 50, 360, 132, details.str(), INK);
                let mut sound = Text::<72>::new();
                if let Some(volume) = crate::audio::volume(0) {
                    let _ = write!(sound, "Sound volume: {}%", volume);
                } else {
                    sound.set("No supported sound device");
                }
                s.text(x + 16, y + 181, sound.str(), MUTED, 1);
                for (index, label) in ["Wi-Fi", "DHCP", "Volume -"].iter().enumerate() {
                    button(
                        s,
                        x + 12 + index as i32 * 120,
                        y + 211,
                        112,
                        label,
                        self.accent,
                    );
                }
                button(s, x + 252, y + 250, 112, "Volume +", self.accent);
                s.text(
                    x + 16,
                    y + 261,
                    clip_label(self.notice.str(), 225, 1),
                    MUTED,
                    1,
                );
            }
            1 => {
                for (row, device) in self
                    .devices
                    .iter()
                    .skip(self.device_offset)
                    .take(8)
                    .enumerate()
                {
                    s.text(
                        x + 16,
                        y + 51 + row as i32 * 24,
                        clip_label(device.str(), 360, 1),
                        INK,
                        1,
                    );
                }
                for (index, label) in ["Previous", "Next", "Rescan"].iter().enumerate() {
                    button(
                        s,
                        x + 12 + index as i32 * 120,
                        y + 250,
                        112,
                        label,
                        self.accent,
                    );
                }
            }
            _ => {
                let (used, capacity) = crate::memory::usage();
                let files = storage::used_slots();
                let mut value = Text::<100>::new();
                for (row, (label, taken, total)) in [
                    ("Kernel memory", used, capacity),
                    ("Local file slots", files, storage::COUNT),
                ]
                .into_iter()
                .enumerate()
                {
                    let top = y + 47 + row as i32 * 60;
                    s.rounded(x + 14, top, 366, 54, 9, SURFACE, 255);
                    s.text(x + 25, top + 5, label, INK, 1);
                    value.clear();
                    if row == 0 {
                        let _ = write!(value, "{} / {} MiB", taken / 1048576, total / 1048576);
                    } else {
                        let _ = write!(value, "{taken} / {total}");
                    }
                    s.centered(x + 260, top + 3, 105, 22, value.str(), MUTED, 1);
                    s.rounded(x + 25, top + 33, 342, 8, 4, 0xcddcec, 255);
                    let width = (taken.saturating_mul(342) / total.max(1)).min(342) as i32;
                    if width > 0 {
                        s.rounded(x + 25, top + 33, width, 8, 4, self.accent, 255);
                    }
                }
                value.clear();
                let _ = write!(
                    value,
                    "Download {} B/s     Upload {} B/s",
                    self.byte_rates.0, self.byte_rates.1
                );
                s.text(x + 25, y + 177, clip_label(value.str(), 345, 1), INK, 1);
                value.clear();
                let _ = write!(
                    value,
                    "Uptime {} s     Display {} x {}",
                    hardware::millis() / 1000,
                    s.w,
                    s.h
                );
                s.text(x + 25, y + 202, clip_label(value.str(), 345, 1), MUTED, 1);
                if let Some(event) = self.alerts.last() {
                    s.text(x + 25, y + 233, clip_label(event, 345, 1), MUTED, 1);
                }
            }
        }
    }
    fn window(&self, s: &mut Screen, n: &Network, id: usize, a: Window) {
        let x = a.x;
        let y = a.y;
        let w = a.w;
        let h = a.h;
        let active = self.focus == id;
        s.shadow(x, y, w, h);
        s.rounded(
            x,
            y,
            w,
            h,
            10,
            if active { 0x9cafc1 } else { 0xc1ccd7 },
            255,
        );
        s.rounded(x + 1, y + 1, w - 2, h - 2, 9, PANEL, 255);
        s.rounded(
            x + 1,
            y + 1,
            w - 2,
            37,
            8,
            if active { 0xf4f7fb } else { 0xebeff4 },
            255,
        );
        s.rect(x + 1, y + 37, w - 2, 1, DIVIDER);
        s.symbol(x + 11, y + 8, id, self.accent);
        s.text(x + 42, y + 9, NAMES[id], INK, 1);
        for (i, label) in ["_", "+", "x"].iter().enumerate() {
            let left = x + w - 114 + i as i32 * 38;
            let over = (left..left + 38).contains(&self.mx) && (y..y + 36).contains(&self.my);
            if over {
                s.rounded(
                    left + 3,
                    y + 4,
                    32,
                    27,
                    4,
                    if i == 2 { 0xf3ccd2 } else { 0xcadcf0 },
                    255,
                );
            }
            match *label {
                "_" => s.rect(left + 14, y + 21, 10, 2, INK),
                "+" => {
                    s.rect(left + 14, y + 13, 10, 2, INK);
                    s.rect(left + 14, y + 22, 10, 2, INK);
                    s.rect(left + 14, y + 13, 2, 11, INK);
                    s.rect(left + 22, y + 13, 2, 11, INK);
                }
                _ => {
                    s.stroke(left, y, (15, 14), (23, 22), INK);
                    s.stroke(left, y, (23, 14), (15, 22), INK);
                }
            }
        }
        let clip = s.clip_rect(x + 1, y + 38, w - 2, h - 39);
        match id {
            ORGANIZER => self.organizer.draw(s, x, y, w, self.accent),
            HARDWARE_CENTER => self.hardware_center.draw(s, x, y, w, h, n, self.accent),
            0 => {
                s.rect(x + 1, y + 38, 142, h - 39, SURFACE);
                s.rect(x + 143, y + 38, 1, h - 39, DIVIDER);
                s.text(x + 18, y + 58, "Locations", INK, 2);
                for (row, label) in ["Documents", "Image library"].iter().enumerate() {
                    let yy = y + 94 + row as i32 * 35;
                    let selected = row == usize::from(self.file_images);
                    if selected {
                        s.rounded(x + 9, yy - 5, 126, 30, 6, SELECTED, 255)
                    }
                    s.symbol(
                        x + 13,
                        yy - 1,
                        if row == 0 { 0 } else { IMAGES },
                        self.accent,
                    );
                    s.text(x + 38, yy, label, if selected { INK } else { MUTED }, 1);
                }
                button(s, x + 163, y + 48, 80, "Open", self.accent);
                s.input_box(
                    x + 163,
                    y + 87,
                    w - 180,
                    28,
                    self.file_search.len > 0,
                    self.accent,
                );
                let search = if self.file_search.len == 0 {
                    "Search files"
                } else {
                    self.file_search.str()
                };
                s.text(x + 172, y + 93, clip_label(search, w - 199, 1), MUTED, 1);
                let items = self.file_items();
                let (first, count) = self.file_viewport();
                let split = w >= 480;
                let list_width = if split { 166 } else { w - 180 };
                s.rounded(x + 163, y + 126, list_width, h - 147, 6, 0xffffff, 255);
                if items.is_empty() {
                    s.text(x + 174, y + 134, "No matching files", MUTED, 1);
                }
                for (row, item) in items.iter().enumerate().skip(first).take(count) {
                    let yy = y + 126 + (row - first) as i32 * 36;
                    let (path, icon) = match item {
                        FileChoice::Parent => ("Up one folder", 0),
                        FileChoice::Folder(path) => (path.str(), 0),
                        FileChoice::Image(index) => (self.images.files[*index].0, IMAGES),
                        FileChoice::Document(index) => (
                            storage::visible_name(unsafe { storage::FILES[*index].name.str() })
                                .unwrap_or(""),
                            1,
                        ),
                    };
                    let name = path.rsplit('/').next().unwrap_or(path);
                    if row == self.file_cursor {
                        s.rounded(x + 165, yy + 2, list_width - 4, 30, 5, SELECTED, 255);
                    }
                    s.symbol(x + 171, yy + 7, icon, self.accent);
                    s.text(
                        x + 198,
                        yy + 7,
                        clip_label(name, list_width - 43, 1),
                        INK,
                        1,
                    );
                }
                if split {
                    match items.get(self.file_cursor) {
                        Some(FileChoice::Image(index)) => {
                            if let Some((_, image)) = self
                                .file_preview
                                .as_ref()
                                .filter(|(selected, _)| selected == index)
                            {
                                s.image(
                                    x + 339,
                                    y + 128,
                                    w - 351,
                                    (h - 143).min(176),
                                    image,
                                    false,
                                );
                            }
                        }
                        Some(FileChoice::Document(index)) => {
                            let data = unsafe { &storage::FILES[*index].data };
                            let preview = if data.len() == 0 {
                                "Empty file"
                            } else if data.str().is_empty() {
                                "Binary file"
                            } else {
                                data.str()
                            };
                            s.text(x + 342, y + 128, "Preview", INK, 1);
                            s.rect(x + 342, y + 153, w - 359, 1, DIVIDER);
                            s.wrapped_small(x + 342, y + 162, w - 359, h - 179, preview, MUTED);
                        }
                        _ => {}
                    }
                }
                let mut count = Text::<32>::new();
                let _ = write!(count, "{} items", items.len());
                s.text(x + 18, y + h - 38, count.str(), MUTED, 1);
            }
            1 => {
                button(s, x + 16, y + 48, 72, "Save", self.accent);
                button(s, x + 96, y + 48, 72, "New note", SURFACE);
                s.text(
                    x + 188,
                    y + 56,
                    clip_label(self.filename.str(), w - 206, 2),
                    INK,
                    2,
                );
                s.input_box(x + 16, y + 94, w - 32, h - 141, false, self.accent);
                let text = self.note.str();
                let tail = tail_lines(text, ((h - 165) / 23).max(1) as usize);
                s.wrapped(x + 34, y + 109, w - 68, h - 162, tail, INK);
                let mut count = Text::<48>::new();
                let _ = write!(count, "{} words", text.split_whitespace().count());
                s.text(x + w - 105, y + h - 34, count.str(), MUTED, 1);
                s.text(
                    x + 22,
                    y + h - 34,
                    if self.notice.len > 0 {
                        self.notice.str()
                    } else {
                        ""
                    },
                    MUTED,
                    1,
                );
            }
            2 => {
                s.rounded(x + 12, y + 48, w - 24, h - 63, 8, 0x142337, 255);
                let text = self.terminal.str();
                let visible = tail_lines(text, ((h - 125) / 18) as usize);
                s.wrapped_small(x + 26, y + 59, w - 52, h - 122, visible, 0xe2edf8);
                s.rect(x + 13, y + h - 52, w - 26, 1, 0x435870);
                s.text(x + 26, y + h - 45, ">", 0x8ce2cd, 2);
                s.text(
                    x + 49,
                    y + h - 45,
                    clip_label(self.command.str(), w - 71, 2),
                    0xffffff,
                    2,
                );
            }
            3 => {
                s.input_box(x + 14, y + 48, w - 110, 34, false, self.accent);
                let url = self.url.str();
                s.text(
                    x + 23,
                    y + 56,
                    if url.is_empty() {
                        "Enter a web address"
                    } else {
                        clip_label(url, w - 136, 1)
                    },
                    INK,
                    1,
                );
                button(s, x + w - 86, y + 48, 72, "Go", self.accent);
                for (slot, label) in [
                    "Tab 1",
                    "Tab 2",
                    "Tab 3",
                    "Tab 4",
                    "Bookmark",
                    ["Links", "Bookmarks", "History", "Page"][self.web_list as usize],
                    "Download",
                ]
                .iter()
                .enumerate()
                {
                    let left = x + 14 + (slot as i32 * (w - 28) + 6) / 7;
                    let right = x + 14 + ((slot + 1) as i32 * (w - 28) + 6) / 7;
                    s.rounded(
                        left,
                        y + 88,
                        right - left - 3,
                        24,
                        4,
                        if self.tab == slot { SELECTED } else { PANEL },
                        255,
                    );
                    s.centered(
                        left,
                        y + 88,
                        right - left - 3,
                        24,
                        clip_label(label, right - left - 6, 1),
                        INK,
                        1,
                    );
                }
                s.input_box(x + 14, y + 119, w - 28, h - 167, false, self.accent);
                let image_height = if self.web_picture.is_some() {
                    if self.page.len == 0 {
                        h - 182
                    } else {
                        (h / 2).max(72)
                    }
                } else {
                    0
                };
                if self.web_list != 0 {
                    let label = ["", "Page links", "Bookmarks", "History"][self.web_list as usize];
                    s.text(x + 24, y + 125, label, INK, 1);
                    for row in 0..Self::web_rows(h) {
                        let Some((label, url)) = self.web_row(self.web_list_offset + row) else {
                            if row == 0 {
                                s.text(x + 24, y + 150, "No entries", MUTED, 1);
                            }
                            break;
                        };
                        let label = if label.is_empty() { url.str() } else { label };
                        s.text(
                            x + 24,
                            y + 150 + row as i32 * 23,
                            clip_label(label, w - 48, 1),
                            self.accent,
                            1,
                        );
                    }
                } else if let Some(image) = &self.web_picture {
                    s.image(x + 26, y + 125, w - 52, image_height, image, false);
                }
                if self.web_list == 0 && self.page.len > 0 {
                    let page = self.page.str();
                    let mut start = self.scroll.min(page.len());
                    while !page.is_char_boundary(start) {
                        start -= 1;
                    }
                    s.wrapped(
                        x + 26,
                        y + 132 + image_height,
                        w - 52,
                        (h - 189 - image_height).max(0),
                        &page[start..],
                        INK,
                    );
                }
                let can_play = self.audio_url.is_some() || !self.audio_clip.is_empty();
                let status = if !self.web_status.is_empty() {
                    self.web_status
                } else if self.url.str().starts_with("file:") {
                    "Local document"
                } else {
                    n.status
                };
                s.text(
                    x + 20,
                    y + h - 31,
                    clip_label(status, w - if can_play { 175 } else { 40 }, 1),
                    MUTED,
                    1,
                );
                if can_play {
                    button(s, x + w - 154, y + h - 43, 138, "Play sound", self.accent);
                }
            }
            4 => {
                s.input_box(x + 24, y + 48, w - 48, 79, false, self.accent);
                s.text(
                    x + 39,
                    y + 57,
                    clip_label(self.calc.str(), w - 78, 2),
                    INK,
                    2,
                );
                s.text(
                    x + 39,
                    y + 86,
                    clip_label(self.answer.str(), w - 78, 3),
                    self.accent,
                    3,
                );
                for (index, label) in [
                    "7", "8", "9", "/", "4", "5", "6", "*", "1", "2", "3", "-", "Clear", "0", "=",
                    "+",
                ]
                .into_iter()
                .enumerate()
                {
                    let r = calculator_key(w, h, index);
                    let color = if label == "=" {
                        self.accent
                    } else if index % 4 == 3 {
                        SELECTED
                    } else {
                        SURFACE
                    };
                    s.rounded(x + r.x, y + r.y, r.w, r.h, 6, color, 255);
                    s.centered(
                        x + r.x,
                        y + r.y,
                        r.w,
                        r.h,
                        label,
                        if label == "=" { 0xffffff } else { INK },
                        2,
                    );
                }
            }
            5 => {
                for (i, c) in [
                    0x176f80, 0x181f30, 0xe15f63, 0xeac75b, 0x51c29b, 0x759fe8, 0xf2f0e9,
                ]
                .iter()
                .enumerate()
                {
                    s.round(x + 20 + i as i32 * 40, y + 53, 30, 28, *c);
                    if *c == self.paint_color {
                        s.rect(x + 20 + i as i32 * 40, y + 86, 30, 2, self.accent)
                    }
                }
                button(s, x + 370, y + 50, 100, "CLEAR", SURFACE);
                unsafe {
                    for yy in 0..180 {
                        for xx in 0..320 {
                            s.rect(
                                x + 20 + xx * 2,
                                y + 100 + yy * 2,
                                2,
                                2,
                                PAINT[(yy * 320 + xx) as usize],
                            )
                        }
                    }
                }
            }
            6 => {
                button(s, x + 20, y + 50, 160, "NEW GAME", SURFACE);
                let mut score: Text<64> = Text::new();
                let _ = write!(
                    score,
                    "SCORE {}  {}",
                    self.snake.len - 4,
                    if self.snake.alive {
                        "ARROWS TO MOVE"
                    } else {
                        "GAME OVER"
                    }
                );
                s.text(x + 200, y + 62, score.str(), self.accent, 1);
                s.rect(x + 20, y + 100, 576, 384, 0x091a27);
                for i in 0..self.snake.len {
                    let (xx, yy) = self.snake.body[i];
                    s.round(
                        x + 21 + xx * 24,
                        y + 101 + yy * 24,
                        22,
                        22,
                        if i == 0 { 0xd9fff0 } else { self.accent },
                    )
                }
                s.round(
                    x + 22 + self.snake.food.0 * 24,
                    y + 102 + self.snake.food.1 * 24,
                    20,
                    20,
                    0xa75a35,
                );
            }
            7 => {
                s.text(x + 24, y + 62, "Local packages", self.accent, 2);
                s.wrapped(
                    x + 24,
                    y + 94,
                    w - 48,
                    38,
                    "Install document apps from the local disk.",
                    MUTED,
                );
                for (index, package) in crate::packages::CATALOG.iter().enumerate() {
                    let mut label = Text::<80>::new();
                    let _ = write!(label, "{}  /  v1  /  Signed", package.name);
                    button(
                        s,
                        x + 24,
                        y + 138 + index as i32 * 54,
                        316,
                        label.str(),
                        self.accent,
                    );
                }
                s.wrapped(x + 24, y + 285, w - 48, 100, self.notice.str(), INK);
            }
            8 => {
                button(
                    s,
                    x + 24,
                    y + 46,
                    104,
                    "Ethernet",
                    if self.ethernet_tab {
                        self.accent
                    } else {
                        SURFACE
                    },
                );
                button(
                    s,
                    x + 136,
                    y + 46,
                    104,
                    "Wi-Fi",
                    if self.ethernet_tab {
                        SURFACE
                    } else {
                        self.accent
                    },
                );
                if self.ethernet_tab {
                    self.ethernet.draw(s, x, y, w, n, self.accent);
                    s.clip = clip;
                    return;
                }
                if self.scanning_results {
                    button(s, x + 254, y + 46, 112, "Back", SURFACE);
                    button(s, x + 374, y + 46, 112, "Scan again", self.accent);
                    s.text(x + 24, y + 95, self.wifi_status, MUTED, 1);
                    let rows = ((h - 128) / 29).max(0) as usize;
                    for (row, point) in self
                        .access_points
                        .iter()
                        .skip(self.access_point_offset)
                        .take(rows)
                        .enumerate()
                    {
                        let yy = y + 127 + row as i32 * 29;
                        let mut label = Text::<96>::new();
                        let security = match point.security {
                            0 => "Open",
                            3 => "WPA2",
                            6 => "WPA3",
                            7 => "WPA2/WPA3",
                            _ => "Secured",
                        };
                        if self.saved_wifi_view {
                            label.set(point.ssid.str());
                        } else {
                            let _ = write!(
                                label,
                                "{}   {} dBm   {}",
                                point.ssid.str(),
                                point.signal,
                                security
                            );
                        }
                        s.rounded(
                            x + 24,
                            yy,
                            w - 48,
                            26,
                            5,
                            if row == 0 { 0xc4dfcf } else { SURFACE },
                            255,
                        );
                        s.text(x + 34, yy + 5, clip_label(label.str(), w - 68, 1), INK, 1);
                    }
                    s.clip = clip;
                    return;
                }
                button(s, x + 254, y + 46, 112, "Scan Wi-Fi", SURFACE);
                let mut status: Text<128> = Text::new();
                let _ = write!(
                    status,
                    "Ethernet: {}   |   Wi-Fi: {}",
                    n.adapter_name(),
                    if n.wifi.joined {
                        "connected"
                    } else {
                        "disconnected"
                    }
                );
                s.text(x + 24, y + 92, status.str(), MUTED, 1);
                s.text(x + 24, y + 126, "Join Wi-Fi", self.accent, 2);
                s.text(x + 24, y + 154, "Network name", INK, 1);
                s.rounded(
                    x + 24,
                    y + 177,
                    w - 48,
                    34,
                    6,
                    if !self.wifi_password_field {
                        0xd8ece2
                    } else {
                        0xffffff
                    },
                    255,
                );
                s.text(x + 35, y + 185, self.wifi_ssid.str(), INK, 1);
                s.text(x + 24, y + 224, "Password", INK, 1);
                s.rounded(
                    x + 24,
                    y + 247,
                    w - 48,
                    34,
                    6,
                    if self.wifi_password_field {
                        0xd8ece2
                    } else {
                        0xffffff
                    },
                    255,
                );
                let mut hidden: Text<64> = Text::new();
                for _ in 0..self.wifi_pass.len {
                    hidden.push(b'*');
                }
                s.text(x + 35, y + 255, hidden.str(), INK, 1);
                button(s, x + 24, y + 295, 112, "Connect", self.accent);
                button(s, x + 148, y + 295, 110, "Saved", SURFACE);
                button(s, x + 268, y + 295, 110, "Disconnect", SURFACE);
                s.text(
                    x + 24,
                    y + 335,
                    clip_label(self.wifi_status, w - 48, 1),
                    MUTED,
                    1,
                );
                button(s, x + 24, y + 378, 131, "WPA2 hotspot", SURFACE);
                button(s, x + 165, y + 378, 131, "WPA3 hotspot", SURFACE);
                let mut native = Text::new();
                self.native_wifi.describe(&mut native);
                s.text(
                    x + 24,
                    y + 354,
                    clip_label(native.str(), w - 48, 1),
                    MUTED,
                    1,
                );
            }
            9 => self.draw_settings(s, n, x, y, w, h),
            10 => {
                button(s, x + 20, y + 50, 160, "NEW GAME", SURFACE);
                s.text(
                    x + 210,
                    y + 61,
                    match self.winner {
                        1 => "X WINS",
                        2 => "O WINS",
                        3 => "DRAW",
                        _ => {
                            if self.turn == 1 {
                                "X TO PLAY"
                            } else {
                                "O TO PLAY"
                            }
                        }
                    },
                    self.accent,
                    2,
                );
                for row in 0..3 {
                    for col in 0..3 {
                        let xx = x + 40 + col * 100;
                        let yy = y + 110 + row * 100;
                        s.round(xx, yy, 94, 94, SURFACE);
                        let v = self.ttt[(row * 3 + col) as usize];
                        if v != 0 {
                            s.centered(
                                xx,
                                yy,
                                94,
                                94,
                                if v == 1 { "X" } else { "O" },
                                if v == 1 { self.accent } else { 0xffb870 },
                                6,
                            )
                        }
                    }
                }
            }
            IMAGES => {
                let sidebar = 174;
                s.rect(x + 1, y + 39, sidebar, h - 47, 0xe7ede2);
                s.text(x + 15, y + 54, "Library", INK, 2);
                let (first, count) = self.image_rows();
                for (row, (name, _)) in self.images.files.iter().skip(first).take(count).enumerate()
                {
                    let index = first + row;
                    let yy = y + 82 + row as i32 * 34;
                    let filename = clip_label(name, 120, 1);
                    if index == self.images.selected {
                        s.rounded(x + 10, yy, 154, 31, 5, 0xc4dfcf, 255);
                    }
                    s.symbol(x + 15, yy + 5, IMAGES, 0x32695f);
                    s.text(x + 42, yy + 7, filename, MUTED, 1);
                }
                let name = self.images.files[self.images.selected].0;
                s.text(
                    x + 190,
                    y + 55,
                    clip_label(name, (w - 314).max(1), 2),
                    INK,
                    2,
                );
                button(
                    s,
                    x + w - 102,
                    y + 46,
                    86,
                    if self.images.zoom {
                        "Fit"
                    } else {
                        "Actual size"
                    },
                    SURFACE,
                );
                let (px, py, pw, ph) = (x + 190, y + 91, w - 206, h - 130);
                s.rect(px, py, pw, ph, 0x203438);
                if let Some(image) = &self.images.picture {
                    s.image(px, py, pw, ph, image, self.images.zoom);
                    let mut size: Text<40> = Text::new();
                    let _ = write!(size, "{} x {} px", image.width, image.height);
                    s.centered(px, y + h - 35, pw, 28, size.str(), MUTED, 1);
                } else {
                    s.centered(px, py, pw, ph, "Could not open this image", 0xe5eee7, 2);
                }
            }
            11 => s.wrapped(x + 24, y + 60, w - 48, h - 85, self.package.str(), INK),
            _ => (),
        }
        s.clip = clip;
    }
}

fn ethernet_details(out: &mut impl Write, n: &Network) {
    let dotted = |a: [u8; 4]| {
        let mut label = Text::<16>::new();
        let _ = write!(label, "{}.{}.{}.{}", a[0], a[1], a[2], a[3]);
        label
    };
    let _ = write!(out, "Ethernet: {}  ", n.adapter_name());
    if let Some(speed) = n.link_speed() {
        let _ = writeln!(out, "{} Mbps", speed);
    } else {
        let _ = writeln!(out, "Link down");
    }
    let _ = writeln!(
        out,
        "{}  |  VLAN {}\nIPv4 {} / {}\nRouter {}   DNS {}\nRX {}  TX {}",
        n.ethernet_status,
        n.vlan,
        dotted(n.config.ip).str(),
        dotted(n.config.mask).str(),
        dotted(n.config.router).str(),
        dotted(n.config.dns).str(),
        n.received,
        n.sent
    );
}

fn read_urls(name: &str) -> alloc::vec::Vec<Text<384>> {
    let Some(index) = storage::find(name) else {
        return alloc::vec::Vec::new();
    };
    let data = unsafe { storage::FILES[index].data.clone() };
    data.str()
        .lines()
        .take(24)
        .filter(|line| line.len() <= 384 && !line.is_empty())
        .map(|line| {
            let mut url = Text::new();
            url.set(line);
            url
        })
        .collect()
}

fn save_urls(name: &str, urls: &mut alloc::vec::Vec<Text<384>>) {
    urls.truncate(24);
    while urls.iter().map(|url| url.len + 1).sum::<usize>() > storage::CAP {
        urls.pop();
    }
    let mut data = storage::Data::default();
    for url in urls {
        data.append(url.str());
        data.push(b'\n');
    }
    if storage::put(name, &data.bytes[..data.len()]).is_ok() {
        storage::sync();
    }
}

pub(crate) fn clip_label(label: &str, width: i32, size: i32) -> &str {
    let mut end = label.len();
    while end > 0 && crate::graphics::text_width(&label[..end], size) > width {
        end = label[..end]
            .char_indices()
            .next_back()
            .map(|(pos, _)| pos)
            .unwrap_or(0);
    }
    &label[..end]
}

pub(crate) fn button(s: &mut Screen, x: i32, y: i32, w: i32, label: &str, c: u32) {
    s.rounded(x, y, w, 34, 6, c, 255);
    s.centered(
        x,
        y,
        w,
        34,
        label,
        if c == SURFACE { INK } else { 0xffffff },
        1,
    );
}
fn tail_lines(s: &str, n: usize) -> &str {
    let mut count = 0;
    for (i, b) in s.bytes().enumerate().rev() {
        if b == b'\n' {
            count += 1;
            if count > n {
                return &s[i + 1..];
            }
        }
    }
    s
}
fn paint_line(mut x: i32, mut y: i32, x1: i32, y1: i32, c: u32) {
    let dx = (x1 - x).abs();
    let sx = if x < x1 { 1 } else { -1 };
    let dy = -(y1 - y).abs();
    let sy = if y < y1 { 1 } else { -1 };
    let mut e = dx + dy;
    loop {
        if x >= 0 && y >= 0 && x < 320 && y < 180 {
            unsafe { PAINT[(y * 320 + x) as usize] = c }
        }
        if x == x1 && y == y1 {
            break;
        }
        let e2 = 2 * e;
        if e2 >= dy {
            e += dy;
            x += sx
        }
        if e2 <= dx {
            e += dx;
            y += sy
        }
    }
}
pub fn run(mut s: Screen, mut network: Network) -> ! {
    let mut desktop = Desktop::new(s.w, s.h, crate::users::Session::new());
    let mut input = Input::new();
    let mut dirty = false;
    let mut last = 0;
    let mut clock = 0;
    let mut previous_bytes = (0, 0);
    desktop.draw(&mut s, &network);
    s.present();
    let mut shown = (desktop.mx, desktop.my);
    s.pointer(None, shown);
    unsafe { hardware::log(b"KreativeOS: desktop ready\r\n") };
    desktop.users.log_initial_view();
    let mut auth_region = desktop.users.region(s.w as i32, s.h as i32);
    loop {
        for _ in 0..32 {
            if let Some(e) = input.poll() {
                let previous_account = (desktop.users.current(), desktop.users.system());
                match e {
                    Event::Key(k) => {
                        desktop.key(k, &mut network);
                        dirty = true;
                    }
                    Event::Mouse(x, y, buttons, wheel) => {
                        let previous = desktop.pointer_hover(s.w as i32, s.h as i32);
                        let interactive = buttons != desktop.buttons
                            || wheel != 0
                            || desktop.drag.is_some()
                            || desktop.painting;
                        desktop.mouse(x, y, buttons, wheel, &s, &mut network);
                        if interactive
                            || (desktop.magnifier && (x != 0 || y != 0))
                            || previous != desktop.pointer_hover(s.w as i32, s.h as i32)
                            || desktop.menu
                        {
                            dirty = true;
                        }
                    }
                }
                if (desktop.users.current(), desktop.users.system()) != previous_account {
                    crate::audio::stop();
                    desktop.scheduler.clear();
                    network.clear_browser_session();
                    if desktop.users.current().is_some() {
                        storage::set_standard_account(desktop.users.standard_name());
                        let pointer = (desktop.mx, desktop.my, desktop.buttons);
                        let users =
                            core::mem::replace(&mut desktop.users, crate::users::Session::new());
                        desktop = Desktop::new(s.w, s.h, users);
                        (desktop.mx, desktop.my, desktop.buttons) = pointer;
                        unsafe { PAINT.fill(0xf2f0e9) };
                        if desktop.users.system() {
                            desktop.settings_page = 1;
                            desktop.open(9);
                            desktop.open(2);
                            desktop.terminal.set("System maintenance\nservice | scrub free | scrub cancel | memory set N\n");
                            unsafe { hardware::log(b"KreativeOS: system1 desktop ready\r\n") };
                        }
                        unsafe { hardware::log(b"KreativeOS: graphical session ready\r\n") };
                    }
                    dirty = true;
                }
            }
        }
        desktop.checkpoint_draft(false);
        crate::system1::tick(hardware::millis());
        dirty |= desktop.scheduler.step(&mut desktop.terminal);
        network.poll();
        if network.tls_pending {
            desktop.draw(&mut s, &network);
            s.present();
            auth_region = desktop.users.region(s.w as i32, s.h as i32);
            shown = (desktop.mx, desktop.my);
            s.pointer(None, shown);
            crate::secure::fetch(&mut network);
        }
        if network.changed {
            network.changed = false;
            if desktop.users.current().is_some() {
                desktop.receive_web(&mut network);
            }
            dirty = true
        }
        if desktop.visible(6) && hardware::millis() - desktop.snake.at >= 140 {
            desktop.snake.step();
            dirty = true
        }
        if hardware::millis() / 1000 != clock {
            let now = hardware::millis() / 1000;
            let elapsed = (now - clock).max(1);
            let bytes = (network.received_bytes, network.sent_bytes);
            desktop.byte_rates = (
                (bytes.0 - previous_bytes.0) / elapsed,
                (bytes.1 - previous_bytes.1) / elapsed,
            );
            previous_bytes = bytes;
            let link = network.link_speed();
            if link != desktop.last_link {
                desktop.alert(if link.is_some() {
                    "Ethernet link connected"
                } else {
                    "Ethernet link disconnected"
                });
                desktop.last_link = link;
            }
            clock = now;
            dirty = true
        }
        if dirty && hardware::millis() - last >= 16 {
            desktop.draw(&mut s, &network);
            let region = desktop.users.region(s.w as i32, s.h as i32);
            if let Some(bounds) = region.filter(|_| auth_region == region) {
                s.present_region(bounds.x, bounds.y, bounds.w, bounds.h);
                s.pointer(Some(shown), (desktop.mx, desktop.my));
            } else {
                s.present();
                s.pointer(None, (desktop.mx, desktop.my));
            }
            auth_region = region;
            shown = (desktop.mx, desktop.my);
            dirty = false;
            last = hardware::millis()
        }
        let pointer = (desktop.mx, desktop.my);
        if shown != pointer {
            s.pointer(Some(shown), pointer);
            shown = pointer;
        }
        core::hint::spin_loop();
    }
}
// Checked arithmetic with precedence, unary signs and parentheses.
fn calculate(s: &str) -> Result<i64, &'static str> {
    struct Parser<'a> {
        b: &'a [u8],
        p: usize,
    }
    impl<'a> Parser<'a> {
        fn space(&mut self) {
            while self.b.get(self.p) == Some(&b' ') {
                self.p += 1
            }
        }
        fn expr(&mut self, min: u8, depth: u8) -> Result<i64, &'static str> {
            if depth > 16 {
                return Err("Too many parentheses");
            }
            self.space();
            let mut left = match self.b.get(self.p).copied() {
                Some(b'(') => {
                    self.p += 1;
                    let v = self.expr(0, depth + 1)?;
                    self.space();
                    if self.b.get(self.p) != Some(&b')') {
                        return Err("Missing closing parenthesis");
                    }
                    self.p += 1;
                    v
                }
                Some(b'-') => {
                    self.p += 1;
                    self.expr(3, depth + 1)?.checked_neg().ok_or("Overflow")?
                }
                Some(b'0'..=b'9') => {
                    let mut n = 0i64;
                    while let Some(c @ b'0'..=b'9') = self.b.get(self.p).copied() {
                        n = n
                            .checked_mul(10)
                            .and_then(|n| n.checked_add((c - b'0') as i64))
                            .ok_or("Overflow")?;
                        self.p += 1
                    }
                    n
                }
                _ => return Err("Enter a number"),
            };
            loop {
                self.space();
                let op = self.b.get(self.p).copied().unwrap_or(0);
                let prec = match op {
                    b'+' | b'-' => 1,
                    b'*' | b'/' => 2,
                    _ => break,
                };
                if prec < min {
                    break;
                }
                self.p += 1;
                let rhs = self.expr(prec + 1, depth + 1)?;
                left = match op {
                    b'+' => left.checked_add(rhs),
                    b'-' => left.checked_sub(rhs),
                    b'*' => left.checked_mul(rhs),
                    b'/' => {
                        if rhs == 0 {
                            return Err("Division by zero");
                        }
                        left.checked_div(rhs)
                    }
                    _ => None,
                }
                .ok_or("Overflow")?
            }
            Ok(left)
        }
    }
    let mut p = Parser {
        b: s.as_bytes(),
        p: 0,
    };
    let v = p.expr(0, 0)?;
    p.space();
    if p.p != s.len() {
        return Err("Unexpected character");
    }
    Ok(v)
}
