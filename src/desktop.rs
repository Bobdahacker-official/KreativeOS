use crate::{
    graphics::Screen,
    hardware::{self, Event, Input},
    net::{self, Network},
    pictures::Catalog,
    storage,
    text::Text,
};
use core::fmt::Write;
const INK: u32 = 0x20363a;
const MUTED: u32 = 0x566a69;
const PANEL: u32 = 0xf4f5ed;
const SURFACE: u32 = 0xe5ebe0;
const NAMES: [&str; 13] = [
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
];
const GUIDE: usize = 11;
const IMAGES: usize = 12;
const APP_COUNT: usize = NAMES.len();
const BAR: i32 = 54;
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
    images: Catalog,
}
impl Desktop {
    fn new(w: usize, h: usize) -> Self {
        let mut d = Self {
            windows: [Window::new(); APP_COUNT],
            restore: [None; APP_COUNT],
            order: core::array::from_fn(|i| i),
            focus: 0,
            menu: false,
            mx: 500,
            my: 380,
            buttons: 0,
            drag: None,
            accent: 0x1d6c58,
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
            images: Catalog::new(),
        };
        for i in 0..APP_COUNT {
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
        d.open(0);
        d.open(IMAGES);
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
        for i in pos..APP_COUNT - 1 {
            self.order[i] = self.order[i + 1]
        }
        self.order[APP_COUNT - 1] = id;
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
            for k in 1..=APP_COUNT {
                let id = (self.focus + k) % APP_COUNT;
                if self.windows[id].open && !self.windows[id].min {
                    self.open(id);
                    break;
                }
            }
            return;
        }
        if !self.windows[self.focus].open || self.windows[self.focus].min {
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
                0x84 => self.url.clear(),
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
            IMAGES => match key {
                0x80 | 0x82 => self.step_image(-1),
                0x81 | 0x83 => self.step_image(1),
                b' ' => self.images.zoom = !self.images.zoom,
                _ => {}
            },
            _ => (),
        }
    }
    fn step_image(&mut self, direction: isize) {
        let count = self.images.files.len() as isize;
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
    fn mouse(&mut self, dx: i32, dy: i32, buttons: u8, wheel: i32, s: &Screen, n: &mut Network) {
        self.mx = (self.mx + dx).clamp(0, s.w as i32 - 1);
        self.my = (self.my + dy).clamp(0, s.h as i32 - 1);
        if wheel != 0 {
            let focused = self.windows[self.focus];
            if focused.open && !focused.min && focused.inside(self.mx, self.my) {
                match self.focus {
                    3 if wheel > 0 => {
                        self.scroll = self.scroll.saturating_sub(wheel as usize * 100)
                    }
                    3 => {
                        self.scroll =
                            (self.scroll + wheel.unsigned_abs() as usize * 100).min(self.page.len)
                    }
                    IMAGES => self.step_image(if wheel > 0 { -1 } else { 1 }),
                    _ => {}
                }
            }
        }
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
            if y >= s.h as i32 - BAR {
                if x < 118 {
                    self.menu = !self.menu;
                } else {
                    let step = task_width(s.w as i32);
                    let slot = ((x - 124).max(0) / step) as usize;
                    if x >= 124 {
                        let id = self.task_ids().nth(slot);
                        if let Some(id) = id {
                            if self.focus == id && self.windows[id].open && !self.windows[id].min {
                                self.windows[id].min = true;
                            } else {
                                self.open(id);
                            }
                        }
                    }
                }
                return;
            }
            if self.menu {
                let top = s.h as i32 - BAR - 406;
                if (18..414).contains(&x) && y >= top + 58 && y < top + 378 {
                    let row = ((y - top - 58) / 48) as usize;
                    let column = ((x - 18) / 198) as usize;
                    let id = row * 2 + column;
                    if id < APP_COUNT && (id != GUIDE || self.installed) {
                        self.open(id);
                        return;
                    }
                }
                self.menu = false;
                return;
            }
            for j in (0..APP_COUNT).rev() {
                let id = self.order[j];
                let w = self.windows[id];
                if !w.open || w.min || !w.inside(x, y) {
                    continue;
                }
                self.open(id);
                let lx = x - w.x;
                let ly = y - w.y;
                if ly < 38 {
                    if lx >= w.w - 38 {
                        self.windows[id].open = false
                    } else if (w.w - 114..w.w - 76).contains(&lx) {
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
                if y >= 128 && x >= 154 {
                    let row = ((y - 128) / 42) as usize;
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
                        0x1d6c58
                    } else if x < 400 {
                        0x5655a0
                    } else {
                        0xa75a35
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
        let w = s.w as i32;
        let h = s.h as i32;
        s.wallpaper();
        s.symbol(28, 25, APP_COUNT, 0x9dd7bb);
        s.text(56, 22, "KreativeOS", 0xe2eee5, 3);
        for &id in &self.order {
            let window = self.windows[id];
            if window.open && !window.min {
                self.window(s, n, id, window);
            }
        }

        let bar_y = h - BAR;
        s.rect(0, bar_y, w, BAR, 0x11262d);
        s.rect(0, bar_y, w, 1, 0x45645b);
        s.rounded(
            12,
            bar_y + 8,
            106,
            38,
            6,
            if self.menu { 0x568978 } else { 0x284b4b },
            255,
        );
        s.symbol(22, bar_y + 16, APP_COUNT, 0xe4f5ea);
        s.centered(41, bar_y + 8, 68, 38, "Apps", 0xe4f5ea, 1);
        let step = task_width(w);
        for (slot, id) in self.task_ids().enumerate() {
            let left = 124 + slot as i32 * step;
            let selected = self.focus == id && self.windows[id].open && !self.windows[id].min;
            let hover = (left..left + step).contains(&self.mx) && self.my >= bar_y;
            if selected || hover {
                s.rounded(left + 2, bar_y + 7, step - 4, 40, 5, 0x315552, 255);
            }
            s.symbol(
                left + (step - 20) / 2,
                bar_y + 16,
                id,
                if selected { 0xa1e5bc } else { 0xd7eae2 },
            );
            if self.windows[id].open {
                s.rect(
                    left + (step - 12) / 2,
                    bar_y + 45,
                    12,
                    2,
                    if selected { 0x99e3ba } else { 0x72978d },
                );
            }
            if hover && !self.menu {
                let width = crate::graphics::text_width(NAMES[id], 1) + 22;
                let x = (left + (step - width) / 2).clamp(4, w - width - 4);
                s.rounded(x, bar_y - 35, width, 28, 5, 0xf4f5ed, 255);
                s.centered(x, bar_y - 35, width, 28, NAMES[id], INK, 1);
            }
        }
        let (hh, mm, _) = hardware::clock();
        let mut clock: Text<12> = Text::new();
        let _ = write!(clock, "{:02}:{:02}", hh, mm);
        s.rect(w - 89, bar_y + 11, 1, 32, 0x45645b);
        s.centered(w - 87, bar_y + 6, 80, 23, clock.str(), 0xe3f4ea, 2);
        s.centered(
            w - 87,
            bar_y + 30,
            80,
            16,
            if n.online() { "Ethernet" } else { "Offline" },
            0x9ab9ad,
            1,
        );

        if self.menu {
            let top = bar_y - 406;
            s.shadow(12, top, 408, 398);
            s.rounded(12, top, 408, 398, 9, PANEL, 255);
            s.text(29, top + 18, "Applications", INK, 3);
            for (id, label) in NAMES.iter().enumerate() {
                if id == GUIDE && !self.installed {
                    continue;
                }
                let left = 18 + (id % 2) as i32 * 198;
                let row = top + 58 + (id / 2) as i32 * 48;
                let over =
                    (left..left + 198).contains(&self.mx) && (row..row + 48).contains(&self.my);
                s.rounded(
                    left + 4,
                    row + 3,
                    190,
                    42,
                    6,
                    if over { 0xd2e6d8 } else { 0xe9eee5 },
                    255,
                );
                s.symbol(left + 15, row + 14, id, 0x246455);
                s.centered(left + 43, row + 3, 139, 42, label, INK, 1);
            }
        }
        s.cursor(self.mx, self.my);
    }
    fn window(&self, s: &mut Screen, n: &Network, id: usize, a: Window) {
        let x = a.x;
        let y = a.y;
        let w = a.w;
        let h = a.h;
        let active = self.focus == id;
        s.shadow(x, y, w, h);
        s.rounded(x, y, w, h, 8, PANEL, 255);
        s.rounded(
            x + 1,
            y + 1,
            w - 2,
            37,
            7,
            if active { 0x25464a } else { 0x36565a },
            255,
        );
        s.rect(
            x + 1,
            y + 35,
            w - 2,
            3,
            if active { 0x79b89a } else { 0x647f79 },
        );
        s.symbol(x + 11, y + 8, id, 0xb3d9c6);
        let title_x = x + 114;
        s.centered(title_x, y + 1, w - 228, 34, NAMES[id], 0xf2f7f1, 2);
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
                    if i == 2 { 0x824d42 } else { 0x52726d },
                    255,
                );
            }
            match *label {
                "_" => s.rect(left + 14, y + 21, 10, 2, 0xe5eee7),
                "+" => {
                    s.rect(left + 14, y + 13, 10, 2, 0xe5eee7);
                    s.rect(left + 14, y + 22, 10, 2, 0xe5eee7);
                    s.rect(left + 14, y + 13, 2, 11, 0xe5eee7);
                    s.rect(left + 22, y + 13, 2, 11, 0xe5eee7);
                }
                _ => {
                    s.stroke(left, y, (15, 14), (23, 22), 0xe5eee7);
                    s.stroke(left, y, (23, 14), (15, 22), 0xe5eee7);
                }
            }
        }
        match id {
            0 => {
                s.rect(x + 1, y + 39, 142, h - 54, 0xe7ede2);
                s.text(x + 18, y + 59, "Locations", MUTED, 1);
                for (row, label) in ["Documents"].iter().enumerate() {
                    let yy = y + 92 + row as i32 * 36;
                    if row == 0 {
                        s.rounded(x + 9, yy - 5, 126, 30, 7, 0xc4dfcf, 255)
                    }
                    s.text(
                        x + 22,
                        yy,
                        label,
                        if row == 0 { 0x32695f } else { MUTED },
                        1,
                    );
                }
                s.text(x + 164, y + 58, "Documents", INK, 3);
                s.text(x + 165, y + 88, "Your local workspace", MUTED, 1);
                let mut row = 0;
                unsafe {
                    for f in &storage::FILES {
                        if f.used {
                            let yy = y + 128 + row * 42;
                            if yy + 30 > y + h {
                                break;
                            }
                            s.rounded(
                                x + 163,
                                yy - 2,
                                w - 180,
                                36,
                                7,
                                if row % 2 == 0 { 0xffffff } else { PANEL },
                                255,
                            );
                            s.rounded(x + 173, yy + 3, 16, 21, 3, 0xdce5f8, 255);
                            s.text(x + 200, yy + 1, f.name.str(), INK, 1);
                            row += 1;
                        }
                    }
                }
                s.text(x + 18, y + h - 38, "On this computer", MUTED, 1);
            }
            1 => {
                button(s, x + 16, y + 48, 90, "Save", self.accent);
                button(s, x + 120, y + 48, 90, "New", SURFACE);
                s.text(x + 230, y + 58, self.filename.str(), MUTED, 1);
                s.rounded(x + 16, y + 94, w - 32, h - 144, 10, 0xffffff, 255);
                let (heading, body) = self
                    .note
                    .str()
                    .split_once('\n')
                    .unwrap_or((self.note.str(), ""));
                s.text(x + 36, y + 112, heading, INK, 3);
                s.wrapped(x + 36, y + 155, w - 72, h - 220, body, 0x6e7588);
                s.text(
                    x + 22,
                    y + h - 34,
                    if self.notice.len > 0 {
                        self.notice.str()
                    } else {
                        "All your ideas start somewhere."
                    },
                    MUTED,
                    1,
                );
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
                s.rounded(x + 14, y + 48, w - 110, 32, 9, 0xffffff, 255);
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
                s.rect(x + 14, y + 95, w - 28, h - 143, 0xffffff);
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
                        "Local document"
                    } else {
                        n.status
                    },
                    MUTED,
                    1,
                );
                s.text(x + w - 116, y + h - 31, "Scroll down", self.accent, 1);
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
                        s.centered(xx, yy, 90, 50, core::str::from_utf8(&b).unwrap(), INK, 3)
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
                button(s, x + 24, y + 138, 300, "INSTALL GUIDE.KAPP", self.accent);
                if self.installed {
                    button(s, x + 24, y + 210, 300, "OPEN KREATIVE GUIDE", SURFACE)
                }
                s.wrapped(x + 24, y + 285, w - 48, 100, self.notice.str(), INK);
            }
            8 => {
                s.text(x + 24, y + 64, "Connections", self.accent, 2);
                let mut details: Text<1024> = Text::new();
                let _=write!(details,"{}\n\nEthernet: {}\nIPv4: 10.0.2.15 / 24\nGateway: 10.0.2.2\nDNS: 10.0.2.3\n\nMAC: {:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}\nFrames received: {}\nFrames sent: {}\n\nWi-Fi PCI devices: {}\nWi-Fi radio driver: not installed",n.status,n.adapter_name(),n.mac[0],n.mac[1],n.mac[2],n.mac[3],n.mac[4],n.mac[5],n.received,n.sent,n.wifi_devices);
                s.wrapped(x + 24, y + 106, w - 48, h - 130, details.str(), INK);
            }
            9 => {
                s.text(x + 24, y + 64, "Make it yours", self.accent, 2);
                button(s, x + 24, y + 120, 150, "Jade", 0x1d6c58);
                button(s, x + 220, y + 120, 150, "Iris", 0x5655a0);
                button(s, x + 420, y + 120, 150, "Copper", 0xa75a35);
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
    }
}
fn task_width(width: i32) -> i32 {
    ((width - 216) / APP_COUNT as i32).clamp(28, 58)
}

fn clip_label(label: &str, width: i32, size: i32) -> &str {
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

fn button(s: &mut Screen, x: i32, y: i32, w: i32, label: &str, c: u32) {
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
    let mut desktop = Desktop::new(s.w, s.h);
    let mut input = Input::new();
    let mut dirty = false;
    let mut last = 0;
    let mut clock = 0;
    desktop.draw(&mut s, &network);
    s.present();
    unsafe { hardware::log(b"KreativeOS: desktop ready\r\n") };
    loop {
        for _ in 0..32 {
            if let Some(e) = input.poll() {
                dirty = true;
                match e {
                    Event::Key(k) => desktop.key(k, &mut network),
                    Event::Mouse(x, y, buttons, wheel) => {
                        desktop.mouse(x, y, buttons, wheel, &s, &mut network)
                    }
                }
            }
        }
        network.poll();
        if network.tls_pending {
            desktop.draw(&mut s, &network);
            s.present();
            crate::secure::fetch(&mut network);
        }
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
