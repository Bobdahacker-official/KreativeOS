use crate::{
    graphics::Screen,
    hardware::{self, Event, Input},
    net::{self, Network},
    storage,
    text::Text,
};
use core::fmt::Write;
const INK: u32 = 0xe4edf7;
const MUTED: u32 = 0x94a8bd;
const PANEL: u32 = 0x152538;
const SURFACE: u32 = 0x1d3046;
const NAMES: [&str; 11] = [
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
];
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
    windows: [Window; 12],
    order: [usize; 12],
    focus: usize,
    menu: bool,
    mx: i32,
    my: i32,
    buttons: u8,
    drag: Option<(usize, i32, i32)>,
    accent: u32,
    note: Text<2048>,
    filename: Text<32>,
    terminal: Text<4096>,
    command: Text<128>,
    url: Text<384>,
    page: Text<12288>,
    scroll: usize,
    calc: Text<128>,
    answer: Text<128>,
    notice: Text<128>,
    snake: Snake,
    paint_color: u32,
    last_paint: Option<(i32, i32)>,
    installed: bool,
    package: Text<2048>,
    ttt: [u8; 9],
    turn: u8,
    winner: u8,
}
impl Desktop {
    fn new(w: usize, h: usize) -> Self {
        let mut d = Self {
            windows: [Window::new(); 12],
            order: [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
            focus: 0,
            menu: false,
            mx: 500,
            my: 380,
            buttons: 0,
            drag: None,
            accent: 0x51dbc4,
            note: Text::new(),
            filename: Text::new(),
            terminal: Text::new(),
            command: Text::new(),
            url: Text::new(),
            page: Text::new(),
            scroll: 0,
            calc: Text::new(),
            answer: Text::new(),
            notice: Text::new(),
            snake: Snake::new(),
            paint_color: 0x176f80,
            last_paint: None,
            installed: false,
            package: Text::new(),
            ttt: [0; 9],
            turn: 1,
            winner: 0,
        };
        for i in 0..12 {
            d.windows[i].w = (w as i32 - 90).min(760);
            d.windows[i].h = (h as i32 - 115).min(530);
            d.windows[i].x = 45 + (i as i32 % 4) * 18;
            d.windows[i].y = 40 + (i as i32 % 3) * 16
        }
        d.filename.set("Welcome.txt");
        if let Some(i) = storage::find("Welcome.txt") {
            unsafe { d.note = storage::FILES[i].data }
        }
        d.terminal
            .set("KreativeOS shell\nType help to see commands.\n");
        d.url.set("file:Readme.html");
        d.load_local();
        d.open(1);
        if let Some(i) = storage::find("Guide.app") {
            unsafe { d.package = storage::FILES[i].data }
            d.installed = true
        }
        d
    }
    fn open(&mut self, id: usize) {
        self.windows[id].open = true;
        self.windows[id].min = false;
        self.focus = id;
        let pos = self.order.iter().position(|&x| x == id).unwrap_or(0);
        for i in pos..11 {
            self.order[i] = self.order[i + 1]
        }
        self.order[11] = id;
        self.menu = false
    }
    fn load_local(&mut self) {
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
        self.notice.set(
            if storage::put(self.filename.str(), &self.note.bytes[..self.note.len]).is_ok() {
                if storage::sync() {
                    "Saved to disk"
                } else {
                    "Saved in memory; data disk unavailable"
                }
            } else {
                "Save failed"
            },
        )
    }
    fn package_install(&mut self) {
        let Some(i) = storage::find("Guide.kapp") else {
            self.notice.set("Guide.kapp was not found");
            return;
        };
        let file = unsafe { storage::FILES[i].data };
        let s = file.str();
        if !s.starts_with("KAPP1\n") || !s.contains("\nversion=1\n") {
            self.notice.set("Invalid KAPP1 package");
            return;
        }
        let Some((_, body)) = s.split_once("\n---\n") else {
            self.notice.set("Missing package document");
            return;
        };
        self.package.set(body);
        self.installed = true;
        let _ = storage::put("Guide.app", body.as_bytes());
        self.notice.set(if storage::sync() {
            "Kreative Guide installed to disk"
        } else {
            "Kreative Guide installed in memory"
        })
    }
    fn key(&mut self, key: u8, n: &mut Network) {
        if (0x90..=0x99).contains(&key) {
            self.open((key - 0x90) as usize);
            return;
        }
        if key == 27 {
            self.menu = !self.menu;
            return;
        }
        if key == b'\t' {
            for k in 1..=12 {
                let id = (self.focus + k) % 12;
                if self.windows[id].open && !self.windows[id].min {
                    self.open(id);
                    break;
                }
            }
            return;
        }
        if !self.windows[self.focus].open {
            return;
        }
        match self.focus {
            1 => match key {
                8 => self.note.pop(),
                b'\n' | 32..=126 => self.note.push(key),
                _ => (),
            },
            2 => match key {
                8 => self.command.pop(),
                b'\n' => self.execute(n),
                32..=126 => self.command.push(key),
                _ => (),
            },
            3 => match key {
                8 => self.url.pop(),
                b'\n' => {
                    if self.url.str().starts_with("file:") {
                        self.load_local()
                    } else {
                        n.browse(self.url.str());
                        self.scroll = 0
                    }
                }
                0x80 => self.scroll = self.scroll.saturating_sub(80),
                0x81 => self.scroll = (self.scroll + 80).min(self.page.len),
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
            _ => (),
        }
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
        if self.terminal.len > 3000 {
            self.terminal.clear()
        }
        let cmd = self.command;
        self.command.clear();
        self.terminal.append("\n> ");
        self.terminal.append(cmd.str());
        self.terminal.push(b'\n');
        let (verb, arg) = cmd
            .str()
            .trim()
            .split_once(' ')
            .unwrap_or((cmd.str().trim(), ""));
        match verb{
 "help"=>self.terminal.append("help | ls | cat NAME | touch NAME | rm NAME\nedit NAME | save | clear | open APP\nbrowse URL | net | uptime | calc EXPR\ninstall Guide.kapp | reboot\n"),
 "ls"=>unsafe{for f in &storage::FILES{if f.used{let _=writeln!(self.terminal,"{}  {} bytes",f.name.str(),f.data.len);}}},
 "cat"=>{if let Some(i)=storage::find(arg){unsafe{self.terminal.append(storage::FILES[i].data.str())}}else{self.terminal.append("File not found")}},
 "touch"=>{match storage::put(arg,b""){Ok(())=>{self.terminal.append(if storage::sync(){"Created on disk"}else{"Created in memory"})},Err(e)=>self.terminal.append(e)}},
 "rm"=>{if storage::remove(arg){self.terminal.append(if storage::sync(){"Removed from disk"}else{"Removed from memory"})}else{self.terminal.append("File not found")}},
 "edit"=>{self.filename.set(arg);self.note.clear();if let Some(i)=storage::find(arg){unsafe{self.note=storage::FILES[i].data}}self.open(1)},
 "save"=>{self.save();self.terminal.append(self.notice.str())},"clear"=>self.terminal.clear(),
 "open"=>{if let Some(i)=NAMES.iter().position(|s|s.eq_ignore_ascii_case(arg)){self.open(i)}else{self.terminal.append("Unknown app")}},
 "browse"=>{self.url.set(arg);if arg.starts_with("file:"){self.load_local()}else{n.browse(arg)}self.open(3)},
 "net"=>{let _=write!(self.terminal,"{}\nIP 10.0.2.15/24  Gateway 10.0.2.2\nDNS 10.0.2.3\nRX {} TX {}\n",n.status,n.received,n.sent);},
 "uptime"=>{let _=write!(self.terminal,"{} seconds",hardware::millis()/1000);},
 "calc"=>{self.calc.set(arg);self.calculate();self.terminal.append(self.answer.str())},
 "install"=>{if arg=="Guide.kapp"{self.package_install();self.terminal.append(self.notice.str())}else{self.terminal.append("Package not found: use Guide.kapp")}},
 "reboot"=>hardware::reboot(),""=>(),_=>self.terminal.append("Unknown command. Type help.")}
    }
    fn mouse(&mut self, dx: i32, dy: i32, buttons: u8, s: &Screen, n: &mut Network) {
        self.mx = (self.mx + dx).clamp(0, s.w as i32 - 1);
        self.my = (self.my + dy).clamp(0, s.h as i32 - 1);
        let click = buttons & 1 != 0 && self.buttons & 1 == 0;
        self.buttons = buttons;
        if buttons & 1 == 0 {
            self.drag = None;
            self.last_paint = None
        }
        if let Some((id, ox, oy)) = self.drag {
            self.windows[id].x = (self.mx - ox).clamp(0, s.w as i32 - 100);
            self.windows[id].y = (self.my - oy).clamp(0, s.h as i32 - 90);
            return;
        }
        if click {
            let x = self.mx;
            let y = self.my;
            let bar = s.h as i32 - 54;
            if y >= bar {
                if x < 74 {
                    self.menu = !self.menu;
                    return;
                }
                let count = self.windows.iter().filter(|a| a.open).count().max(1) as i32;
                let width = ((s.w as i32 - 190) / count).min(74);
                if x >= 80 && x < s.w as i32 - 110 {
                    let slot = ((x - 80) / width) as usize;
                    if let Some(id) = (0..12).filter(|&i| self.windows[i].open).nth(slot) {
                        self.open(id)
                    }
                }
                return;
            }
            if self.menu {
                let top = bar - 420;
                if x >= 14 && x < 350 && y >= top {
                    let id = ((y - top - 56) / 29) as usize;
                    if y >= top + 56 && id < 11 {
                        self.open(id);
                        return;
                    }
                    if y >= top + 56 && id == 11 && self.installed {
                        self.open(11);
                        return;
                    }
                }
                self.menu = false;
                return;
            }
            for j in (0..12).rev() {
                let id = self.order[j];
                let w = self.windows[id];
                if !w.open || w.min || !w.inside(x, y) {
                    continue;
                }
                self.open(id);
                let lx = x - w.x;
                let ly = y - w.y;
                if ly < 38 {
                    if lx > w.w - 36 {
                        self.windows[id].open = false
                    } else if lx > w.w - 70 {
                        self.windows[id].min = true
                    } else {
                        self.drag = Some((id, lx, ly))
                    }
                    return;
                }
                self.app_click(id, lx, ly, n);
                break;
            }
        }
        if buttons & 1 != 0 && self.focus == 5 {
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
                }
            }
        }
    }
    fn app_click(&mut self, id: usize, x: i32, y: i32, n: &mut Network) {
        match id {
            0 => {
                if y >= 100 {
                    let row = ((y - 100) / 28) as usize;
                    let found = unsafe {
                        storage::FILES
                            .iter()
                            .enumerate()
                            .filter(|(_, f)| f.used)
                            .nth(row)
                            .map(|(i, _)| i)
                    };
                    if let Some(i) = found {
                        let f = unsafe { storage::FILES[i] };
                        if f.name.str().ends_with(".html") {
                            self.url.set("file:");
                            self.url.append(f.name.str());
                            self.load_local();
                            self.open(3)
                        } else {
                            self.filename = f.name;
                            self.note = f.data;
                            self.open(1)
                        }
                    }
                }
            }
            1 => {
                if y < 84 {
                    if x < 115 {
                        self.save()
                    } else if x < 220 {
                        self.filename.set("Untitled.txt");
                        self.note.clear();
                        self.notice.clear()
                    }
                }
            }
            3 => {
                if y < 88 && x > self.windows[id].w - 100 {
                    if self.url.str().starts_with("file:") {
                        self.load_local()
                    } else {
                        n.browse(self.url.str());
                        self.scroll = 0
                    }
                } else if y > self.windows[id].h - 40 {
                    self.scroll = (self.scroll + 200).min(self.page.len)
                }
            }
            4 => {
                if y >= 160 && y < 400 && x >= 24 && x < 424 {
                    let col = ((x - 24) / 100) as usize;
                    let row = ((y - 160) / 60) as usize;
                    let keys = b"789/456*123-C0=+";
                    let key = keys[row * 4 + col];
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
                if y >= 130 && y < 190 {
                    self.package_install()
                } else if y >= 200 && y < 260 && self.installed {
                    self.open(11)
                }
            }
            9 => {
                if y >= 110 && y < 170 {
                    self.accent = if x < 200 {
                        0x51dbc4
                    } else if x < 400 {
                        0xa29bfe
                    } else {
                        0xffb870
                    }
                } else if y >= 220 && y < 270 {
                    self.notice.set(if storage::sync() {
                        "All files saved"
                    } else {
                        "No writable data disk"
                    })
                } else if y >= 300 && y < 350 {
                    hardware::reboot()
                }
            }
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
            _ => (),
        }
    }
    fn draw(&self, s: &mut Screen, n: &Network) {
        let w = s.w as i32;
        let h = s.h as i32;
        for y in 0..h {
            let v = (y * 20 / h) as u32;
            s.rect(0, y, w, 1, ((8 + v) << 16) | ((21 + v) << 8) | (36 + v))
        }
        for i in 0..6 {
            s.line(w - 430 + i * 48, 0, w, h - 120 - i * 55, 0x18374b)
        }
        s.text(32, 32, "KREATIVE", 0x86b9c6, 3);
        s.text(34, 62, "SPACE FOR YOUR IDEAS", 0x547d92, 1);
        for &id in &self.order {
            let a = self.windows[id];
            if a.open && !a.min {
                self.window(s, n, id, a)
            }
        }
        s.rect(0, h - 56, w, 56, 0x0b1725);
        s.rect(0, h - 56, w, 1, 0x294559);
        s.round(12, h - 44, 52, 34, self.accent);
        s.text(28, h - 36, "K", 0x102435, 3);
        let count = self.windows.iter().filter(|a| a.open).count().max(1) as i32;
        let width = ((w - 190) / count).min(74);
        let mut slot = 0;
        for id in 0..12 {
            if self.windows[id].open {
                let x = 80 + slot * width;
                slot += 1;
                s.round(
                    x,
                    h - 44,
                    width - 6,
                    34,
                    if self.focus == id && !self.windows[id].min {
                        SURFACE
                    } else {
                        0x0b1725
                    },
                );
                s.text(
                    x + 7,
                    h - 31,
                    if id < 11 { NAMES[id] } else { "Guide" },
                    INK,
                    1,
                );
                if self.focus == id {
                    s.rect(x + 20, h - 8, 28, 3, self.accent)
                }
            }
        }
        let (hh, mm, _) = hardware::clock();
        let mut time: Text<16> = Text::new();
        let _ = write!(time, "{:02}:{:02}", hh, mm);
        s.text(w - 80, h - 36, time.str(), INK, 2);
        s.text(
            w - 80,
            h - 17,
            if n.io == 0 { "OFFLINE" } else { "ETHERNET" },
            MUTED,
            1,
        );
        if self.menu {
            let top = h - 54 - 420;
            s.round(14, top, 336, 410, PANEL);
            s.text(34, top + 18, "YOUR APPS", self.accent, 2);
            for i in 0..11 {
                s.text(36, top + 56 + i as i32 * 29, NAMES[i], INK, 2)
            }
            if self.installed {
                s.text(36, top + 56 + 11 * 29, "Kreative Guide", self.accent, 2)
            }
        }
        for i in 0..18 {
            s.line(self.mx, self.my + i, self.mx + i / 2, self.my + i, 0xf6ffff)
        }
        s.line(self.mx, self.my, self.mx + 9, self.my + 17, 0x0a1724);
        s.line(self.mx, self.my, self.mx, self.my + 19, 0x0a1724);
        s.line(
            self.mx + 5,
            self.my + 13,
            self.mx + 10,
            self.my + 24,
            0xf6ffff,
        );
    }
    fn window(&self, s: &mut Screen, n: &Network, id: usize, a: Window) {
        let x = a.x;
        let y = a.y;
        let w = a.w;
        let h = a.h;
        s.round(x + 6, y + 7, w, h, 0x06101b);
        s.round(x, y, w, h, PANEL);
        s.rect(x + 1, y + 37, w - 2, 1, 0x365066);
        s.text(
            x + 18,
            y + 12,
            if id < 11 { NAMES[id] } else { "Kreative Guide" },
            if id == self.focus { self.accent } else { INK },
            2,
        );
        s.text(x + w - 59, y + 13, "_", MUTED, 2);
        s.text(x + w - 27, y + 12, "X", 0xf1959b, 2);
        match id {
            0 => {
                s.text(x + 20, y + 57, "LOCAL FILES", self.accent, 2);
                s.text(x + 20, y + 80, "SELECT A FILE TO OPEN", MUTED, 1);
                let mut row = 0;
                unsafe {
                    for f in &storage::FILES {
                        if f.used {
                            let yy = y + 100 + row * 28;
                            if yy + 26 > y + h {
                                break;
                            }
                            if row % 2 == 0 {
                                s.rect(x + 12, yy - 4, w - 24, 27, SURFACE)
                            }
                            s.text(x + 24, yy, f.name.str(), INK, 2);
                            let mut size: Text<32> = Text::new();
                            let _ = write!(size, "{} B", f.data.len);
                            s.text(x + w - 110, yy, size.str(), MUTED, 1);
                            row += 1
                        }
                    }
                }
            }
            1 => {
                button(s, x + 16, y + 48, 90, "SAVE", self.accent);
                button(s, x + 120, y + 48, 90, "NEW", SURFACE);
                s.text(x + 230, y + 59, self.filename.str(), MUTED, 1);
                s.rect(x + 16, y + 94, w - 32, h - 150, 0x0d1c2b);
                s.wrapped(x + 26, y + 106, w - 52, h - 174, self.note.str(), INK);
                s.text(x + 20, y + h - 35, self.notice.str(), self.accent, 1);
            }
            2 => {
                s.rect(x + 12, y + 48, w - 24, h - 95, 0x07111d);
                let text = self.terminal.str();
                let visible = tail_lines(text, ((h - 125) / 18) as usize);
                s.wrapped(x + 22, y + 58, w - 44, h - 120, visible, 0xa5e8c9);
                s.text(x + 22, y + h - 32, ">", self.accent, 2);
                s.text(x + 46, y + h - 32, self.command.str(), INK, 2);
            }
            3 => {
                s.rect(x + 14, y + 48, w - 110, 32, 0x0b1a29);
                let u = self.url.str();
                s.text(
                    x + 21,
                    y + 58,
                    if u.len() > ((w - 130) / 6) as usize {
                        &u[u.len() - ((w - 130) / 6) as usize..]
                    } else {
                        u
                    },
                    INK,
                    1,
                );
                button(s, x + w - 86, y + 48, 72, "GO", self.accent);
                s.rect(x + 14, y + 95, w - 28, h - 143, 0xe7edf0);
                let start = self.scroll.min(self.page.len);
                s.wrapped(
                    x + 26,
                    y + 108,
                    w - 52,
                    h - 165,
                    &self.page.str()[start..],
                    0x21394c,
                );
                s.text(
                    x + 20,
                    y + h - 31,
                    if self.url.str().starts_with("file:") {
                        "LOCAL DOCUMENT"
                    } else {
                        n.status
                    },
                    MUTED,
                    1,
                );
                s.text(x + w - 116, y + h - 31, "SCROLL DOWN", self.accent, 1);
            }
            4 => {
                s.text(x + 24, y + 63, self.calc.str(), INK, 2);
                s.text(x + 24, y + 105, self.answer.str(), self.accent, 3);
                let keys = b"789/456*123-C0=+";
                for row in 0..4 {
                    for col in 0..4 {
                        let xx = x + 24 + col * 100;
                        let yy = y + 160 + row * 60;
                        s.round(xx, yy, 90, 50, SURFACE);
                        let b = [keys[(row * 4 + col) as usize]];
                        s.text(xx + 36, yy + 15, core::str::from_utf8(&b).unwrap(), INK, 3)
                    }
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
                    0xffb870,
                );
            }
            7 => {
                s.text(x + 24, y + 62, "LOCAL PACKAGES", self.accent, 2);
                s.wrapped(
                    x + 24,
                    y + 94,
                    w - 48,
                    38,
                    "Install document apps from the local disk.",
                    MUTED,
                );
                button(s, x + 24, y + 138, 300, "INSTALL GUIDE.KAPP", self.accent);
                if self.installed {
                    button(s, x + 24, y + 210, 300, "OPEN KREATIVE GUIDE", SURFACE)
                }
                s.wrapped(x + 24, y + 285, w - 48, 100, self.notice.str(), INK);
            }
            8 => {
                s.text(x + 24, y + 64, "CONNECTIONS", self.accent, 2);
                let mut details: Text<1024> = Text::new();
                let _=write!(details,"{}\n\nEthernet: RTL8139\nIPv4: 10.0.2.15 / 24\nGateway: 10.0.2.2\nDNS: 10.0.2.3\n\nMAC: {:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}\nFrames received: {}\nFrames sent: {}\n\nWi-Fi PCI devices: {}\nWi-Fi radio driver: not installed",n.status,n.mac[0],n.mac[1],n.mac[2],n.mac[3],n.mac[4],n.mac[5],n.received,n.sent,n.wifi_devices);
                s.wrapped(x + 24, y + 106, w - 48, h - 130, details.str(), INK);
            }
            9 => {
                s.text(x + 24, y + 64, "MAKE IT YOURS", self.accent, 2);
                button(s, x + 24, y + 120, 150, "MINT", 0x51dbc4);
                button(s, x + 220, y + 120, 150, "LILAC", 0xa29bfe);
                button(s, x + 420, y + 120, 150, "AMBER", 0xffb870);
                button(s, x + 24, y + 228, 280, "SAVE ALL FILES", SURFACE);
                button(s, x + 24, y + 308, 280, "RESTART COMPUTER", SURFACE);
                s.text(x + 24, y + h - 40, self.notice.str(), self.accent, 1);
            }
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
                            s.text(
                                xx + 31,
                                yy + 25,
                                if v == 1 { "X" } else { "O" },
                                if v == 1 { self.accent } else { 0xffb870 },
                                6,
                            )
                        }
                    }
                }
            }
            11 => s.wrapped(x + 24, y + 60, w - 48, h - 85, self.package.str(), INK),
            _ => (),
        }
    }
}
fn button(s: &mut Screen, x: i32, y: i32, w: i32, label: &str, c: u32) {
    s.round(x, y, w, 34, c);
    s.text(
        x + 12,
        y + 12,
        label,
        if c == SURFACE { INK } else { 0x102437 },
        1,
    )
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
    let mut desktop = Desktop::new(s.w, s.h);
    let mut input = Input::new();
    let mut dirty = true;
    let mut last = 0;
    let mut clock = 0;
    unsafe { hardware::log(b"KreativeOS: desktop ready\r\n") };
    loop {
        for _ in 0..32 {
            if let Some(e) = input.poll() {
                dirty = true;
                match e {
                    Event::Key(k) => desktop.key(k, &mut network),
                    Event::Mouse(x, y, b) => desktop.mouse(x, y, b, &s, &mut network),
                }
            }
        }
        network.poll();
        if network.changed {
            network.changed = false;
            if !desktop.url.str().starts_with("file:") {
                net::readable(network.body.str(), &mut desktop.page)
            }
            dirty = true
        }
        if desktop.windows[6].open
            && !desktop.windows[6].min
            && hardware::millis() - desktop.snake.at >= 140
        {
            desktop.snake.step();
            dirty = true
        }
        if hardware::millis() / 1000 != clock {
            clock = hardware::millis() / 1000;
            dirty = true
        }
        if dirty && hardware::millis() - last >= 16 {
            desktop.draw(&mut s, &network);
            s.present();
            dirty = false;
            last = hardware::millis()
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
