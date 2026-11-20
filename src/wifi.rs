//! ESP-AT station mode and binary TCP transport over a 16550 on COM2.
use crate::{
    hardware::{self, in8, millis, out8},
    text::Text,
};
use core::fmt::Write;
use embedded_io::ErrorKind;

const PORT: u16 = 0x2f8;
const WAIT: u64 = 3500;

pub struct Module {
    initialized: bool,
    pub joined: bool,
    pub active: bool,
    available: usize,
    eof: bool,
}
impl Module {
    pub const fn new() -> Self {
        Self {
            initialized: false,
            joined: false,
            active: false,
            available: 0,
            eof: false,
        }
    }
    fn configure(&mut self) {
        unsafe {
            out8(PORT + 1, 0);
            out8(PORT + 3, 0x80);
            out8(PORT, 1);
            out8(PORT + 1, 0);
            out8(PORT + 3, 3);
            out8(PORT + 2, 0xc7);
            out8(PORT + 4, 0x0b);
        }
        let start = millis();
        while millis() - start < 20 {
            unsafe {
                if in8(PORT + 5) & 1 != 0 {
                    let _ = in8(PORT);
                }
            }
            core::hint::spin_loop();
        }
        self.initialized = true;
    }
    fn put(&self, data: &[u8], deadline: u64) -> Result<(), ErrorKind> {
        for &byte in data {
            unsafe {
                loop {
                    let line = in8(PORT + 5);
                    if line == 0xff {
                        return Err(ErrorKind::NotConnected);
                    }
                    if line & 0x20 != 0 {
                        break;
                    }
                    if millis() >= deadline {
                        return Err(ErrorKind::TimedOut);
                    }
                }
                out8(PORT, byte);
            }
        }
        Ok(())
    }
    fn byte(&self, deadline: u64) -> Result<u8, ErrorKind> {
        loop {
            unsafe {
                let line = in8(PORT + 5);
                if line == 0xff {
                    return Err(ErrorKind::NotConnected);
                }
                if line & 1 != 0 {
                    let byte = in8(PORT);
                    if line & 0x1e != 0 {
                        return Err(ErrorKind::InvalidData);
                    }
                    return Ok(byte);
                }
            }
            if millis() >= deadline {
                return Err(ErrorKind::TimedOut);
            }
            core::hint::spin_loop();
        }
    }
    fn send(&self, command: &str, deadline: u64) -> Result<(), ErrorKind> {
        self.put(command.as_bytes(), deadline)?;
        self.put(b"\r\n", deadline)
    }
    fn event(&mut self, line: &str) {
        if let Some(n) = line
            .strip_prefix("+IPD,")
            .and_then(|n| n.parse::<usize>().ok())
        {
            self.available = n.min(65535);
        }
        if line == "CLOSED" {
            self.eof = true;
        }
        if line == "WIFI DISCONNECT" {
            self.joined = false;
            self.eof = true;
        }
    }
    fn wait(&mut self, expected: &str, deadline: u64) -> Result<(), ErrorKind> {
        let mut line: Text<128> = Text::new();
        loop {
            let b = self.byte(deadline)?;
            if b == b'>' && expected == ">" {
                return Ok(());
            }
            if b == b'\n' {
                let msg = line.str().trim();
                if msg == expected {
                    return Ok(());
                }
                if ["ERROR", "FAIL", "SEND FAIL", "link is not valid"].contains(&msg) {
                    return Err(ErrorKind::Other);
                }
                self.event(msg);
                line.clear();
            } else {
                line.push(b);
            }
        }
    }
    fn command(&mut self, command: &str, timeout: u64) -> Result<(), ErrorKind> {
        let limit = millis() + timeout;
        self.send(command, limit)?;
        self.wait("OK", limit)
    }
    pub fn join(&mut self, ssid: &str, password: &str) -> Result<(), &'static str> {
        let printable = |s: &str| s.bytes().all(|b| (32..127).contains(&b));
        if ssid.is_empty()
            || ssid.len() > 32
            || password.len() > 63
            || !printable(ssid)
            || !printable(password)
        {
            return Err("Check the Wi-Fi name and password");
        }
        if self.active {
            self.close();
        }
        if !self.initialized {
            self.configure();
        }
        self.joined = false;
        let mut responded = false;
        for _ in 0..3 {
            if self.command("AT", 900).is_ok() {
                responded = true;
                break;
            }
        }
        if !responded {
            return Err("ESP-AT did not answer on COM2");
        }
        self.command("ATE0", WAIT)
            .map_err(|_| "Could not configure ESP-AT")?;
        self.command("AT+SYSSTORE=0", WAIT)
            .map_err(|_| "Could not disable module settings persistence")?;
        self.command("AT+CWMODE=1", WAIT)
            .map_err(|_| "Could not set station mode")?;
        let mut cmd: Text<256> = Text::new();
        cmd.append("AT+CWJAP=");
        quote(&mut cmd, ssid);
        cmd.push(b',');
        quote(&mut cmd, password);
        let result = self.command(cmd.str(), 20000);
        cmd.bytes.fill(0);
        result.map_err(|_| "Wi-Fi association failed")?;
        self.joined = true;
        unsafe { hardware::log(b"KreativeOS: ESP-AT Wi-Fi associated\r\n") };
        Ok(())
    }
    pub fn connect(&mut self, host: &str, port: u16) -> Result<(), ErrorKind> {
        if !self.joined {
            return Err(ErrorKind::NotConnected);
        }
        if self.active {
            self.close();
        }
        self.available = 0;
        self.eof = false;
        self.command("AT+CIPMUX=0", WAIT)?;
        if self.command("AT+CIPRECVTYPE=1", WAIT).is_err() {
            self.command("AT+CIPRECVMODE=1", WAIT)?;
        }
        let mut cmd: Text<180> = Text::new();
        let _ = write!(cmd, "AT+CIPSTART=\"TCP\",\"{}\",{}", host, port);
        self.command(cmd.str(), 12000)?;
        self.active = true;
        Ok(())
    }
    pub fn close(&mut self) {
        if self.active && !self.eof {
            let _ = self.command("AT+CIPCLOSE", 700);
        }
        self.active = false;
        self.eof = false;
        self.available = 0;
    }
    pub fn write(&mut self, data: &[u8]) -> Result<usize, ErrorKind> {
        if self.eof || !self.active {
            return Err(ErrorKind::NotConnected);
        }
        if data.is_empty() {
            return Ok(0);
        }
        let n = data.len().min(1024);
        let mut cmd: Text<42> = Text::new();
        let _ = write!(cmd, "AT+CIPSEND={}", n);
        let limit = millis() + WAIT;
        self.send(cmd.str(), limit)?;
        self.wait(">", limit)?;
        self.put(&data[..n], limit)?;
        self.wait("SEND OK", limit)?;
        Ok(n)
    }
    pub fn read(&mut self, out: &mut [u8]) -> Result<usize, ErrorKind> {
        if out.is_empty() {
            return Ok(0);
        }
        if !self.active {
            return Err(ErrorKind::NotConnected);
        }
        let limit = millis() + 10000;
        if self.available == 0 && !self.eof {
            let mut line: Text<128> = Text::new();
            while self.available == 0 && !self.eof {
                let byte = self.byte(limit)?;
                if byte == b'\n' {
                    self.event(line.str().trim());
                    line.clear();
                } else {
                    line.push(byte);
                }
            }
        }
        if self.available == 0 {
            return Ok(0);
        }
        let requested = out.len().min(self.available).min(1024);
        let mut cmd: Text<48> = Text::new();
        let _ = write!(cmd, "AT+CIPRECVDATA={}", requested);
        self.send(cmd.str(), limit)?;
        let mut line: Text<128> = Text::new();
        let count = loop {
            let byte = self.byte(limit)?;
            if byte == b'\n' {
                let msg = line.str().trim();
                if msg == "ERROR" {
                    return Err(ErrorKind::Other);
                }
                self.event(msg);
                line.clear();
            } else if byte == b',' {
                if let Some(size) = line.str().trim().strip_prefix("+CIPRECVDATA:") {
                    break size.parse::<usize>().map_err(|_| ErrorKind::InvalidData)?;
                }
                line.push(byte);
            } else {
                line.push(byte);
            }
        };
        if count > requested {
            return Err(ErrorKind::InvalidData);
        }
        for cell in &mut out[..count] {
            *cell = self.byte(limit)?;
        }
        self.available = self.available.saturating_sub(count);
        self.wait("OK", limit)?;
        if count == 0 && !self.eof {
            return Err(ErrorKind::TimedOut);
        }
        Ok(count)
    }
}

fn quote<const N: usize>(text: &mut Text<N>, value: &str) {
    text.push(b'"');
    for b in value.bytes() {
        if b == b'"' || b == b'\\' || b == b',' {
            text.push(b'\\');
        }
        text.push(b);
    }
    text.push(b'"');
}
