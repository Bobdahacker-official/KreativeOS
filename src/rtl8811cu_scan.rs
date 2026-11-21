//! Receive-only 2.4 GHz scanning for the RTL8811CU USB adapter.
use super::*;

const TABLES: &[u8] = include_bytes!("../assets/rtl8821c-phy.bin");
pub(super) const RX_TRANSFER: usize = 4096;
const RX_DESCRIPTOR: usize = 24;
pub(super) const RX_ENABLE: u32 = (1 << 7) | (1 << 4) | (1 << 3) | (1 << 1);
pub(super) const TX_AND_RX: u32 = 0xff;
pub(super) const SCAN_CHANNELS: [u8; 11] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];

pub fn passive_scan(
    usb: &mut impl Usb,
    status: Probe,
    initialized: &mut bool,
) -> Result<Vec<AccessPoint>> {
    let Probe::FirmwareRunning {
        cut,
        board: Ok(board),
        ..
    } = status
    else {
        return Err("Native scan needs working firmware and factory board data");
    };
    let mut io = Registers(usb);
    if !*initialized {
        if let Err(error) = io.setup_receiver(cut, board) {
            let _ = io.update(0x0100, 1, TX_AND_RX, 0);
            let _ = io.update(0x001f, 1, 7, 0);
            let _ = io.update(0x00ef, 1, 7, 0);
            return Err(error);
        }
        *initialized = true;
    }
    let result = io.scan_channels(board);
    let stop = io.update(0x0100, 1, TX_AND_RX, 0);
    stop?;
    result
}

fn delay(micros: u64) {
    let start = hardware::micros();
    while hardware::micros().wrapping_sub(start) < micros {
        core::hint::spin_loop();
    }
}

fn phy_tables(mut source: &[u8]) -> Option<[&[u8]; 5]> {
    source = source.strip_prefix(b"RTP1")?;
    let mut tables = [&[][..]; 5];
    for table in &mut tables {
        let rows = u32::from_le_bytes(source.get(..4)?.try_into().ok()?) as usize;
        source = source.get(4..)?;
        let length = rows.checked_mul(8)?;
        *table = source.get(..length)?;
        source = source.get(length..)?;
    }
    source.is_empty().then_some(tables)
}

fn applicable_rows(table: &[u8], cut: u8, board: Board) -> impl Iterator<Item = (u32, u32)> + '_ {
    let mut current = 0;
    let mut apply = true;
    let mut branch_taken = false;
    let cut = u32::from(if cut == 0 { 15 } else { cut });
    let package = u32::from(if board.package == 0 {
        15
    } else {
        board.package
    });
    table.chunks_exact(8).filter_map(move |row| {
        let address = u32::from_le_bytes(row[..4].try_into().unwrap());
        let value = u32::from_le_bytes(row[4..].try_into().unwrap());
        match address >> 30 {
            2 | 3 => match (address >> 28) & 3 {
                3 => {
                    apply = true;
                    branch_taken = false;
                }
                2 => apply = !branch_taken,
                _ => current = address,
            },
            1 => {
                let table_cut = (current >> 24) & 15;
                let table_package = (current >> 12) & 15;
                let interface = (current >> 8) & 15;
                apply = !branch_taken
                    && (table_cut == 0 || table_cut == cut)
                    && (table_package == 0 || table_package == package)
                    && (interface == 0 || interface == 2)
                    && current & 255 == u32::from(board.radio_front_end);
                branch_taken |= apply;
            }
            _ if apply => return Some((address, value)),
            _ => {}
        }
        None
    })
}

impl<T: Usb> Registers<'_, T> {
    pub(super) fn setup_receiver(&mut self, cut: u8, board: Board) -> Result {
        if !matches!(board.radio_front_end, 0 | 2 | 4 | 6) {
            return Err("Unsupported RTL8811CU radio front-end profile");
        }
        let [mac, agc, btg, baseband, rf] =
            phy_tables(TABLES).ok_or("Invalid Realtek PHY tables")?;
        // Disable all transmit paths before enabling the baseband.
        self.update(0x0100, 1, TX_AND_RX, 0)?;
        self.write(0x0522, 1, 0xff)?;
        self.update(0x0002, 1, 0x43, 0x43)?;
        self.update(0x0002, 1, 3, 0)?;
        self.update(0x0002, 1, 3, 3)?;
        self.write(0x001f, 1, 7)?;
        delay(10);
        self.write(0x00ef, 1, 7)?;
        delay(10);
        self.update(0x0808, 4, 3 << 28, 0)?;
        let started = hardware::millis();
        for (kind, table) in [(0, mac), (1, baseband), (1, agc), (1, btg), (2, rf)] {
            if core::ptr::eq(table, btg) && !matches!(board.radio_front_end, 2 | 4) {
                continue;
            }
            for (address, value) in applicable_rows(table, cut, board) {
                if hardware::millis().wrapping_sub(started) >= 20_000 {
                    return Err("RTL8811CU PHY initialization timed out");
                }
                match (kind, address) {
                    (0, _) => self.write(address as u16, 1, value)?,
                    (1, 0xfe) => delay(50_000),
                    (1, 0xfd) => delay(5000),
                    (1, 0xfc) => delay(1000),
                    (1, 0xfb) => delay(50),
                    (1, 0xfa) => delay(5),
                    (1, 0xf9) => delay(1),
                    (1, _) => self.write(address as u16, 4, value)?,
                    (2, 0xffe) => delay(50_000),
                    (2, 0xfe) => delay(100),
                    (2, _) => self.write_rf(address as u8, value)?,
                    _ => unreachable!(),
                }
            }
        }
        let trim = u32::from(if board.crystal_trim == 0xff {
            0
        } else {
            board.crystal_trim & 0x3f
        });
        self.update(0x0024, 4, 0x7e00_0000, trim << 25)?;
        self.update(0x0028, 4, 0x7e, trim << 1)?;
        self.update(0x0a2c, 4, (1 << 18) | (1 << 22), 0)?;
        self.update(0x0808, 4, 3 << 28, 3 << 28)?;
        self.update(0x0100, 1, TX_AND_RX, 0)?;
        self.write(0x0522, 1, 0xff)?;
        self.write(0x0608, 4, (1 << 28) | 0x0f)?; // PHY report, accept beacons from every BSSID.
        self.write(0x060f, 1, 4)?; // Four eight-byte PHY report units.
        self.write(0x06a0, 2, 1 << 8)?; // Management subtype 8 (beacon).
        self.write(0x06a2, 2, 0)?; // Control frames.
        self.write(0x06a4, 2, 0)?;
        // Disable USB RX aggregation. The decoder also accepts aligned aggregates.
        self.update(0x010c, 1, 4, 0)?;
        Ok(())
    }

    fn write_rf(&mut self, address: u8, value: u32) -> Result {
        self.write(
            0x0c90,
            4,
            (u32::from(address) << 20) | (value & 0x000f_ffff),
        )?;
        delay(13);
        Ok(())
    }

    fn update_rf(&mut self, address: u8, mask: u32, value: u32) -> Result {
        let old = self.read(0x2800 + u16::from(address) * 4, 4)? & 0x000f_ffff;
        if old == 0x000f_ffff {
            return Err("RTL8811CU RF register unavailable");
        }
        self.write_rf(address, (old & !mask) | value)
    }

    pub(super) fn tune_20mhz(&mut self, channel: u8, board: Board) -> Result {
        self.update(0x0808, 4, 1 << 28, 1 << 28)?;
        self.update(0x0454, 1, 1 << 7, 0)?;
        self.update(0x0a80, 4, 1 << 18, 0)?;
        self.update(0x0814, 4, 0xfc00, 15 << 10)?;
        self.update(0x0c1c, 4, 0xf00, 0)?;
        self.update(0x0860, 4, 0x1ffe_0000, 0x96a << 17)?;
        self.update(0x08ac, 4, !0xffcf_fc00, 0x1001_0000)?;
        self.update(0x08c4, 4, 1 << 30, 1 << 30)?;
        self.write(0x0483, 1, 0)?;
        self.update(0x0668, 4, 0x180, 0)?;
        self.update(0x0024, 4, 3 << 20, 0)?;
        self.write(0x055c, 1, 80)?;
        self.write(0x0638, 1, 80)?;
        self.update(0x1080, 4, 1 << 16, 1 << 16)?;
        self.update(0x0000, 4, 1 << 26, 1 << 26)?;
        let btg = matches!(board.radio_front_end, 2 | 4);
        self.update(0x0cb8, 4, 0xf5_0000, if btg { 1 << 16 } else { 0x70_0000 })?;
        self.update(
            0x0a84,
            4,
            0xff_0000,
            if btg { 0x0e_0000 } else { 0x12_0000 },
        )?;
        self.update(0x0a80, 4, 0xffff, if btg { 0xfc84 } else { 0x7532 })?;
        self.update_rf(0xdf, 1 << 6, 1 << 6)?;
        self.update_rf(0x64, 15, 15)?;
        self.update_rf(0x18, 0x70fff, 0xc00 | u32::from(channel))?;
        self.update_rf(0xb8, 1 << 19, 0)?;
        self.update_rf(0xb8, 1 << 19, 1 << 19)?;
        self.update(0x0948, 4, 3 << 28, 2 << 28)?;
        self.update(0x094c, 4, 3 << 28, 2 << 28)?;
        self.update(0x0c20, 4, 1 << 31, 1 << 31)?;
        self.update(0x08f0, 4, 1 << 31, 0)?;
        Ok(())
    }

    fn scan_channels(&mut self, board: Board) -> Result<Vec<AccessPoint>> {
        let mut points: Vec<AccessPoint> = Vec::new();
        let mut data = [0u8; RX_TRANSFER];
        for channel in SCAN_CHANNELS {
            self.update(0x0100, 1, TX_AND_RX, 0)?;
            self.tune_20mhz(channel, board)?;
            self.update(0x0100, 1, TX_AND_RX, RX_ENABLE)?;
            let start = hardware::millis();
            while hardware::millis().wrapping_sub(start) < 125 {
                if let Some(length) = self.0.receive(&mut data)? {
                    for point in usb_beacons(&data[..length], channel) {
                        if let Some(existing) = points.iter_mut().find(|ap| {
                            ap.ssid.str() == point.ssid.str() && ap.security == point.security
                        }) {
                            existing.signal = existing.signal.max(point.signal);
                        } else if points.len() < 64 {
                            points.push(point);
                        }
                    }
                }
            }
        }
        points.sort_by(|first, second| second.signal.cmp(&first.signal));
        Ok(points)
    }
}

fn usb_beacons(data: &[u8], channel: u8) -> impl Iterator<Item = AccessPoint> + '_ {
    usb_frames(data).filter_map(move |(frame, signal)| beacon(frame, channel, signal))
}

pub(super) fn usb_frames(mut data: &[u8]) -> impl Iterator<Item = (&[u8], i16)> {
    core::iter::from_fn(move || loop {
        let header = data.get(..RX_DESCRIPTOR)?;
        let flags = u32::from_le_bytes(header[..4].try_into().ok()?);
        let length = (flags & 0x3fff) as usize;
        let info = ((flags >> 16) & 15) as usize * 8;
        let shift = ((flags >> 24) & 3) as usize;
        if length == 0 {
            return None;
        }
        let offset = RX_DESCRIPTOR + info + shift;
        let frame = data.get(offset..offset + length)?;
        let signal = if flags & (1 << 26) != 0 && info >= 8 && data[RX_DESCRIPTOR] & 15 == 1 {
            i16::from(data[RX_DESCRIPTOR + 1]) - 110
        } else {
            i16::MIN
        };
        let next = (offset + length + 7) & !7;
        data = data.get(next..).unwrap_or(&[]);
        // Firmware events, CRC errors and ICV errors are not 802.11 frames.
        if header[11] & 0x10 == 0 && flags & 0xc000 == 0 {
            return Some((frame, signal));
        }
    })
}

pub(super) fn beacon(frame: &[u8], channel: u8, signal: i16) -> Option<AccessPoint> {
    if frame.len() < 36 || frame[0] != 0x80 || frame[1] & 3 != 0 {
        return None;
    }
    let mut info = &frame[36..];
    let mut ssid = Text::new();
    let mut security = if frame[34] & 0x10 == 0 { 0 } else { 1 };
    let mut primary = None;
    while let Some((&id, rest)) = info.split_first() {
        let (&size, rest) = rest.split_first()?;
        let (bytes, tail) = rest.split_at_checked(usize::from(size))?;
        match id {
            0 if !bytes.is_empty() && bytes.len() <= 32 => {
                let name = core::str::from_utf8(bytes).ok()?;
                if name.chars().any(char::is_control) {
                    return None;
                }
                ssid.set(name);
            }
            3 | 61 if !bytes.is_empty() => {
                if primary.is_some_and(|old| old != bytes[0]) {
                    return None;
                }
                primary = Some(bytes[0]);
            }
            48 => security = rsn_security(bytes).unwrap_or(1),
            221 if bytes.starts_with(&[0, 0x50, 0xf2, 1]) && security < 2 => security = 2,
            _ => {}
        }
        info = tail;
    }
    (ssid.len != 0 && primary == Some(channel)).then_some(AccessPoint {
        ssid,
        signal,
        security,
    })
}

fn rsn_security(data: &[u8]) -> Option<u8> {
    if data.get(..2)? != [1, 0] {
        return None;
    }
    let pairwise = u16::from_le_bytes(data.get(6..8)?.try_into().ok()?) as usize;
    let akm = data.get(8 + pairwise * 4..)?;
    let count = u16::from_le_bytes(akm.get(..2)?.try_into().ok()?) as usize;
    let suites = akm.get(2..2 + count * 4)?.chunks_exact(4);
    let mut psk = false;
    let mut sae = false;
    for suite in suites {
        if suite.get(..3) == Some(&[0, 0x0f, 0xac]) {
            psk |= matches!(suite[3], 2 | 4 | 6);
            sae |= matches!(suite[3], 8 | 9);
        }
    }
    Some(match (psk, sae) {
        (true, true) => 7,
        (_, true) => 6,
        (true, _) => 3,
        _ => 1,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_visible_beacons_with_valid_usb_lengths() {
        let mut frame = alloc::vec![0u8; 36];
        frame[0] = 0x80;
        frame[34] = 0x10;
        frame.extend_from_slice(&[0, 4, b'T', b'e', b's', b't', 3, 1, 6]);
        frame.extend_from_slice(&[
            48, 22, 1, 0, 0, 15, 172, 4, 1, 0, 0, 15, 172, 4, 2, 0, 0, 15, 172, 2, 0, 15, 172, 8,
        ]);
        let mut packet = alloc::vec![0u8; RX_DESCRIPTOR + 8];
        let flags = (1u32 << 26) | (1 << 16) | frame.len() as u32;
        packet[..4].copy_from_slice(&flags.to_le_bytes());
        packet[RX_DESCRIPTOR] = 1;
        packet[RX_DESCRIPTOR + 1] = 55;
        packet.extend_from_slice(&frame);
        let got: Vec<_> = usb_beacons(&packet, 6).collect();
        assert_eq!(got.len(), 1);
        assert_eq!(
            (got[0].ssid.str(), got[0].security, got[0].signal),
            ("Test", 7, -55)
        );
        assert!(usb_beacons(&packet, 1).next().is_none());
        assert!(usb_beacons(&packet[..packet.len() - 1], 6).next().is_none());
        packet[11] = 0x10;
        assert!(usb_beacons(&packet, 6).next().is_none());
        packet[11] = 0;
        packet[1] |= 0x40;
        assert!(usb_beacons(&packet, 6).next().is_none());
    }

    #[test]
    fn vendor_tables_have_supported_receive_only_register_layouts() {
        let [mac, _, _, _, rf] = phy_tables(TABLES).unwrap();
        for radio_front_end in [0, 2, 4, 6] {
            let board = Board {
                radio_front_end,
                channel_plan: 0,
                country: None,
                crystal_trim: 0,
                package: 0,
                station_address: [2, 1, 2, 3, 4, 5],
                cck_power: [20; 6],
                ofdm_power: [20; 5],
                ofdm_difference: Some(0),
            };
            let mac_rows: Vec<_> = applicable_rows(mac, 1, board).collect();
            assert!(mac_rows.len() > 64);
            assert!(!mac_rows
                .iter()
                .any(|&(address, _)| matches!(address, 0x100 | 0x101 | 0x522)));
            let rf_rows: Vec<_> = applicable_rows(rf, 1, board).collect();
            assert!(rf_rows.iter().any(|&(address, _)| address == 0x18));
            assert!(rf_rows
                .iter()
                .all(|&(address, _)| address <= 255 || address == 0xffe));
        }
    }
}
