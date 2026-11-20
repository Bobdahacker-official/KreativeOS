//! Intel ICH AC'97 PCM output with a bounded DMA buffer.
use crate::hardware::*;

pub const DMA_SIZE: usize = 2 * 1024 * 1024;
pub static mut DMA: usize = 0;
static mut PORT: u16 = 0;
static mut MIXER: u16 = 0;

pub unsafe fn init() {
    if DMA == 0 {
        return;
    }
    for bus in 0..=255 {
        for slot in 0..32 {
            for func in 0..8 {
                if pci(bus, slot, func, 0) != 0x2415_8086 {
                    continue;
                }
                let mixer = pci(bus, slot, func, 0x10);
                let engine = pci(bus, slot, func, 0x14);
                if mixer & 1 == 0 || engine & 1 == 0 {
                    continue;
                }
                MIXER = (mixer & 0xfffc) as u16;
                PORT = (engine & 0xfffc) as u16;
                let cmd = pci(bus, slot, func, 4);
                pci_write(bus, slot, func, 4, (cmd & 0xffff) | 5);
                out32(PORT + 0x2c, in32(PORT + 0x2c) | 2);
                let start = millis();
                while in32(PORT + 0x30) & 0x100 == 0 && millis() - start < 150 {}
                out16(MIXER, 0);
                out16(MIXER + 0x02, 0);
                out16(MIXER + 0x18, 0x0808);
                if in16(MIXER + 0x28) & 1 != 0 {
                    out16(MIXER + 0x2a, 1);
                    out16(MIXER + 0x2c, 48000);
                }
                log(b"KreativeOS: AC97 audio ready\r\n");
                return;
            }
        }
    }
}

pub fn available() -> bool {
    unsafe { PORT != 0 }
}
pub fn volume(step: i8) -> Option<u8> {
    unsafe {
        if MIXER == 0 {
            return None;
        }
        let register = MIXER + 0x02;
        let previous = in16(register);
        let level = if previous & 0x8000 != 0 {
            0
        } else {
            32 - (previous & 31) as u8
        };
        let level = (i16::from(level) + i16::from(step)).clamp(0, 32) as u8;
        if step != 0 {
            let attenuation = u16::from(32u8.saturating_sub(level).min(31));
            out16(
                register,
                if level == 0 {
                    0x8000
                } else {
                    attenuation * 0x101
                },
            );
        }
        Some((u16::from(level) * 100 / 32) as u8)
    }
}
pub fn stop() {
    unsafe {
        if PORT != 0 {
            out8(PORT + 0x1b, 0);
        }
    }
}

pub struct Wave<'a> {
    channels: usize,
    rate: usize,
    data: &'a [u8],
}

fn value16(s: &[u8], off: usize) -> Option<u16> {
    Some(u16::from_le_bytes(s.get(off..off + 2)?.try_into().ok()?))
}
fn value32(s: &[u8], off: usize) -> Option<u32> {
    Some(u32::from_le_bytes(s.get(off..off + 4)?.try_into().ok()?))
}
pub fn wave(bytes: &[u8]) -> Option<Wave<'_>> {
    if bytes.get(..4)? != b"RIFF" || bytes.get(8..12)? != b"WAVE" {
        return None;
    }
    let mut format = None;
    let mut data = None;
    let mut offset = 12;
    while offset + 8 <= bytes.len() {
        let size = value32(bytes, offset + 4)? as usize;
        let end = offset.checked_add(8)?.checked_add(size)?;
        let chunk = bytes.get(offset + 8..end)?;
        match &bytes[offset..offset + 4] {
            b"fmt " => {
                format = Some((
                    value16(chunk, 0)?,
                    value16(chunk, 2)?,
                    value32(chunk, 4)?,
                    value16(chunk, 14)?,
                ))
            }
            b"data" => data = Some(chunk),
            _ => {}
        }
        offset = end.checked_add(size % 2)?;
    }
    let (encoding, channels, rate, depth) = format?;
    let data = data?;
    if encoding != 1
        || depth != 16
        || ![1, 2].contains(&channels)
        || !(8000..=96000).contains(&rate)
        || data.len() < channels as usize * 2
    {
        return None;
    }
    Some(Wave {
        channels: channels as usize,
        rate: rate as usize,
        data,
    })
}

pub fn play(bytes: &[u8]) -> Result<(), &'static str> {
    let w = wave(bytes).ok_or("Expected 16-bit PCM WAV")?;
    if !available() {
        return Err("No AC97 audio device");
    }
    let source_frames = w.data.len() / (w.channels * 2);
    let frames = (source_frames * 48000 / w.rate).min((DMA_SIZE - 4096) / 4);
    if frames == 0 {
        return Err("Empty audio clip");
    }
    stop();
    unsafe {
        out8(PORT + 0x1b, 2);
        let start = millis();
        while in8(PORT + 0x1b) & 2 != 0 {
            if millis() - start > 100 {
                return Err("Audio controller did not reset");
            }
        }
        out16(PORT + 0x16, 0x001c);
        let data = (DMA + 4096) as *mut i16;
        for frame in 0..frames {
            let pos = frame * w.rate;
            let i = (pos / 48000).min(source_frames - 1);
            let next = (i + 1).min(source_frames - 1);
            let f = (pos % 48000) as i32;
            for channel in 0..2 {
                let read = |frame| {
                    let off = (frame * w.channels + channel.min(w.channels - 1)) * 2;
                    i16::from_le_bytes([w.data[off], w.data[off + 1]]) as i32
                };
                let value = (read(i) * (48000 - f) + read(next) * f) / 48000;
                data.add(frame * 2 + channel).write(value as i16);
            }
        }
        let words = frames * 2;
        let descriptors = words.div_ceil(32768);
        for index in 0..descriptors {
            let base = (DMA + index * 8) as *mut u32;
            base.write((DMA + 4096 + index * 65536) as u32);
            base.add(1).write((words - index * 32768).min(32768) as u32);
        }
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        out32(PORT + 0x10, DMA as u32);
        out8(PORT + 0x15, (descriptors - 1) as u8);
        out8(PORT + 0x1b, 1);
        log(b"KreativeOS: playing HTTPS WAV\r\n");
    }
    Ok(())
}
