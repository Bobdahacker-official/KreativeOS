//! Infrastructure station mode for the RTL8811CU.
use super::{
    scan::{beacon, usb_frames, RX_ENABLE, RX_TRANSFER, SCAN_CHANNELS, TX_AND_RX},
    wpa::{accepts_rsn, Handshake, Keys, RSN_PSK},
    *,
};
use alloc::collections::VecDeque;

const LLC: &[u8; 6] = &[0xaa, 0xaa, 3, 0, 0, 0];
const EAPOL: [u8; 2] = [0x88, 0x8e];
const RATES: [u8; 12] = [2, 4, 11, 22, 12, 18, 24, 36, 48, 72, 96, 108];

#[derive(Clone, Copy)]
struct Candidate {
    bssid: [u8; 6],
    channel: u8,
    basic_rates: u16,
    rates: u16,
    capability: u16,
}

pub struct Station {
    pub mac: [u8; 6],
    ap: [u8; 6],
    rate: u8,
    seq: u16,
    keys: Option<Keys>,
    handshake: Option<Handshake>,
    received: VecDeque<Vec<u8>>,
    alive: u64,
}

pub fn join(
    usb: &mut impl Usb,
    probe: Probe,
    initialized: &mut bool,
    ssid: &str,
    pass: &str,
) -> Result<Station> {
    let Probe::FirmwareRunning {
        cut,
        board: Ok(board),
        ..
    } = probe
    else {
        return Err("Realtek join requires firmware and factory board data");
    };
    if ssid.is_empty() || ssid.len() > 32 {
        return Err("Enter a valid Wi-Fi network name");
    }
    let mut io = Registers(usb);
    let result = (|| {
        if !*initialized {
            io.setup_receiver(cut, board)?;
            *initialized = true;
        }
        let ap = io.find_network(ssid, !pass.is_empty(), board)?;
        let auth = if pass.is_empty() {
            None
        } else {
            Some(Handshake::new(
                ssid.as_bytes(),
                pass.as_bytes(),
                board.station_address,
                ap.bssid,
            )?)
        };
        io.enable_station(board, ap)?;
        let mut sta = Station {
            mac: board.station_address,
            ap: ap.bssid,
            rate: ap.basic_rates.trailing_zeros() as u8,
            seq: 0,
            keys: None,
            handshake: None,
            received: VecDeque::new(),
            alive: hardware::millis(),
        };
        let auth_frame = sta.management(0xb0, &[0, 0, 1, 0, 0, 0]);
        io.management_exchange(&sta, &auth_frame, 0xb0, 6, |body| {
            body.get(..4) == Some(&[0, 0, 2, 0])
        })?;
        let mut elements = Vec::new();
        elements.extend_from_slice(&(ap.capability & 0x0431).to_le_bytes());
        elements.extend_from_slice(&10u16.to_le_bytes());
        elements.extend_from_slice(&[0, ssid.len() as u8]);
        elements.extend_from_slice(ssid.as_bytes());
        let rates: Vec<_> = RATES
            .iter()
            .enumerate()
            .filter(|(i, _)| ap.rates & (1 << i) != 0)
            .map(|(i, &rate)| {
                rate | if ap.basic_rates & (1 << i) != 0 {
                    0x80
                } else {
                    0
                }
            })
            .collect();
        let head = rates.len().min(8);
        elements.extend_from_slice(&[1, head as u8]);
        elements.extend_from_slice(&rates[..head]);
        if rates.len() > head {
            elements.extend_from_slice(&[50, (rates.len() - head) as u8]);
            elements.extend_from_slice(&rates[head..]);
        }
        if auth.is_some() {
            elements.extend_from_slice(&RSN_PSK);
        }
        let assoc = sta.management(0x00, &elements);
        io.management_exchange(&sta, &assoc, 0x10, 4, |body| {
            body.get(4..6)
                .is_some_and(|aid| u16::from_le_bytes(aid.try_into().unwrap()) & 0x3fff != 0)
                || body[2..4] != [0, 0]
        })?;
        if let Some(mut handshake) = auth {
            io.finish_wpa2(&mut sta, &mut handshake)?;
            sta.handshake = Some(handshake);
        }
        sta.alive = hardware::millis();
        Ok(sta)
    })();
    if result.is_err() {
        let _ = io.disable_station();
    }
    result
}

pub fn stop(usb: &mut impl Usb, station: &mut Station) {
    if station.keys.is_none() {
        let frame = station.management(0xa0, &[8, 0]);
        let _ = Registers(usb).send_packet(&frame, 18, station.rate);
    }
    let _ = Registers(usb).disable_station();
}

fn candidate(frame: &[u8], channel: u8, ssid: &str, secured: bool) -> Option<Candidate> {
    let point = beacon(frame, channel, i16::MIN)?;
    if point.ssid.str() != ssid {
        return None;
    }
    let capability = u16::from_le_bytes(frame[34..36].try_into().ok()?);
    if capability & 3 != 1 || (capability & 0x10 != 0) != secured {
        return None;
    }
    let mut info = &frame[36..];
    let mut rsn = false;
    let mut basic_rates = 0;
    let mut rates = 0;
    while let Some((&tag, tail)) = info.split_first() {
        let (&length, tail) = tail.split_first()?;
        let (value, tail) = tail.split_at_checked(usize::from(length))?;
        if matches!(tag, 1 | 50) {
            for &rate in value {
                if let Some(index) = RATES.iter().position(|&r| r == rate & 0x7f) {
                    rates |= 1 << index;
                    if rate & 0x80 != 0 {
                        basic_rates |= 1 << index
                    }
                } else if rate & 0x80 != 0 {
                    return None;
                }
            }
        }
        if tag == 48 {
            rsn = accepts_rsn(value)
        }
        info = tail;
    }
    if basic_rates == 0 || secured != rsn {
        return None;
    }
    let bssid: [u8; 6] = frame[16..22].try_into().ok()?;
    if bssid != frame[10..16] || bssid == [0; 6] || bssid[0] & 1 != 0 {
        return None;
    }
    Some(Candidate {
        bssid,
        channel,
        basic_rates,
        rates,
        capability,
    })
}

impl<T: Usb> Registers<'_, T> {
    fn find_network(&mut self, ssid: &str, secured: bool, board: Board) -> Result<Candidate> {
        let mut data = [0; RX_TRANSFER];
        let mut strongest: Option<(Candidate, i16)> = None;
        for channel in SCAN_CHANNELS {
            self.update(0x0100, 1, TX_AND_RX, 0)?;
            self.tune_20mhz(channel, board)?;
            self.write(0x06a0, 2, 1 << 8)?;
            self.write(0x06a4, 2, 0)?;
            self.update(0x0100, 1, TX_AND_RX, RX_ENABLE)?;
            let start = hardware::millis();
            while hardware::millis().wrapping_sub(start) < 130 {
                if let Some(len) = self.0.receive(&mut data)? {
                    for (frame, signal) in usb_frames(&data[..len]) {
                        if let Some(ap) = candidate(frame, channel, ssid, secured) {
                            if strongest.is_none_or(|(_, current)| signal > current) {
                                strongest = Some((ap, signal));
                            }
                        }
                    }
                }
            }
        }
        let (ap, _) = strongest.ok_or(if secured {
            "Compatible WPA2-Personal (CCMP) network not found"
        } else {
            "Open network not found. Enter the password for a secured network."
        })?;
        self.update(0x0100, 1, TX_AND_RX, 0)?;
        self.tune_20mhz(ap.channel, board)?;
        Ok(ap)
    }

    fn enable_station(&mut self, board: Board, ap: Candidate) -> Result {
        let group = match ap.channel {
            1..=2 => 0,
            3..=5 => 1,
            6..=8 => 2,
            _ => 3,
        };
        let factory_index = board.cck_power[group];
        let ofdm = board.ofdm_power[group];
        if factory_index > 0x3f || ofdm > 0x3f || board.ofdm_difference.is_none() {
            return Err("RTL8811CU factory transmit power data unavailable");
        }
        // Use conservative fixed legacy rates. Do not enable HT/VHT or power boosting.
        let cck_index = factory_index.saturating_sub(8).min(20);
        let ofdm_index =
            (i16::from(ofdm) + i16::from(board.ofdm_difference.unwrap()) - 8).clamp(0, 20) as u8;
        for (address, index) in [
            (0x1d00, cck_index),
            (0x1d04, ofdm_index),
            (0x1d08, ofdm_index),
        ] {
            self.write(address, 4, u32::from_le_bytes([index; 4]))?;
        }
        for (offset, byte) in board.station_address.iter().enumerate() {
            self.write(0x0610 + offset as u16, 1, u32::from(*byte))?;
        }
        for (offset, byte) in ap.bssid.iter().enumerate() {
            self.write(0x0618 + offset as u16, 1, u32::from(*byte))?;
        }
        self.update(0x0102, 1, 3, 2)?; // Port 0 infrastructure station mode.
        self.write(0x0440, 2, u32::from(ap.basic_rates))?;
        self.write(0x010d, 1, 0xf0)?; // Management and high-priority frames use the first OUT endpoint.
        self.write(0x0230, 2, 0x200)?;
        self.update(0x022c, 4, 1 << 31, 1 << 31)?;
        self.write(0x06a0, 2, 0x1d02)?; // Association reply, beacon, auth and disconnect frames.
        self.write(0x06a2, 2, 0)?;
        self.write(0x06a4, 2, 0xffff)?;
        self.write(0x0608, 4, (1 << 28) | (1 << 10) | 0xce)?; // Software CCMP; filter by BSSID.
        self.update(0x0550, 1, (1 << 3) | (1 << 4), 1 << 4)?;
        self.write(0x0522, 1, 0)?;
        self.update(0x0100, 1, TX_AND_RX, TX_AND_RX)
    }

    fn disable_station(&mut self) -> Result {
        self.write(0x0522, 1, 0xff)?;
        self.update(0x0100, 1, TX_AND_RX, 0)
    }

    fn send_packet(&mut self, frame: &[u8], queue: u8, rate: u8) -> Result {
        let pad = usize::from((TX_DESCRIPTOR + frame.len()) % 512 == 0);
        let length = frame.len() + pad;
        let mut packet = alloc::vec![0; TX_DESCRIPTOR + length];
        let flags = 1 << 31 | 1 << 26 | (TX_DESCRIPTOR as u32) << 16 | length as u32;
        packet[..4].copy_from_slice(&flags.to_le_bytes());
        packet[5] = queue;
        packet[13] = 5; // Use the selected legacy rate without fallback.
        packet[16] = rate;
        let seq = u16::from_le_bytes([frame[22], frame[23]]) >> 4;
        packet[36..40].copy_from_slice(&(u32::from(seq) << 12).to_le_bytes());
        let checksum = packet[..32].chunks_exact(2).fold(0u16, |acc, pair| {
            acc ^ u16::from_le_bytes([pair[0], pair[1]])
        });
        packet[28..30].copy_from_slice(&checksum.to_le_bytes());
        packet[TX_DESCRIPTOR..TX_DESCRIPTOR + frame.len()].copy_from_slice(frame);
        if !self.0.bulk_out(&packet) {
            return Err("RTL8811CU transmit failed");
        }
        Ok(())
    }

    fn management_exchange(
        &mut self,
        sta: &Station,
        request: &[u8],
        subtype: u8,
        status_offset: usize,
        matches_reply: impl Fn(&[u8]) -> bool,
    ) -> Result {
        let mut data = [0u8; RX_TRANSFER];
        let mut outbound = request.to_vec();
        for _ in 0..3 {
            self.send_packet(&outbound, 18, sta.rate)?;
            outbound[1] |= 8;
            let start = hardware::millis();
            while hardware::millis().wrapping_sub(start) < 1000 {
                let Some(len) = self.0.receive(&mut data)? else {
                    continue;
                };
                for (frame, _) in usb_frames(&data[..len]) {
                    if frame.len() < 24 + status_offset
                        || frame[0] != subtype
                        || frame[4..10] != sta.mac
                        || frame[10..16] != sta.ap
                        || frame[16..22] != sta.ap
                        || !matches_reply(&frame[24..])
                    {
                        continue;
                    }
                    if frame[24 + status_offset - 2..24 + status_offset] != [0, 0] {
                        return Err("Access point rejected native Wi-Fi connection");
                    }
                    return Ok(());
                }
            }
        }
        Err("Timed out waiting for access point")
    }

    fn finish_wpa2(&mut self, sta: &mut Station, handshake: &mut Handshake) -> Result {
        let mut data = [0u8; RX_TRANSFER];
        let start = hardware::millis();
        while hardware::millis().wrapping_sub(start) < 7500 {
            let Some(len) = self.0.receive(&mut data)? else {
                continue;
            };
            for (frame, _) in usb_frames(&data[..len]) {
                let Some((header, body)) = sta.inbound(frame) else {
                    continue;
                };
                if header[1] & 0x40 != 0
                    || header[4..10] != sta.mac
                    || !body.starts_with(LLC)
                    || body.get(6..8) != Some(&EAPOL)
                {
                    continue;
                }
                if let Some((reply, keys)) =
                    handshake.advance(&body[8..], None, hardware::millis())?
                {
                    let ap = sta.ap;
                    let packet = sta.data_packet(ap, EAPOL, &reply, false)?;
                    self.send_packet(&packet, 17, sta.rate)?;
                    if keys.is_some() {
                        sta.keys = keys;
                        handshake.complete();
                        return Ok(());
                    }
                }
            }
        }
        Err("WPA2 authentication timed out. Check the password.")
    }
}

impl Station {
    pub fn rate_mbps(&self) -> u16 {
        u16::from(RATES[usize::from(self.rate)]) / 2
    }
    fn header(&mut self, control: [u8; 2], destination: [u8; 6]) -> [u8; 24] {
        let mut frame = [0u8; 24];
        frame[..2].copy_from_slice(&control);
        frame[4..10].copy_from_slice(&self.ap);
        frame[10..16].copy_from_slice(&self.mac);
        frame[16..22].copy_from_slice(&destination);
        frame[22..24].copy_from_slice(&((self.seq & 0xfff) << 4).to_le_bytes());
        self.seq = self.seq.wrapping_add(1);
        frame
    }
    fn management(&mut self, subtype: u8, body: &[u8]) -> Vec<u8> {
        let ap = self.ap;
        let mut frame = self.header([subtype, 0], ap).to_vec();
        frame.extend_from_slice(body);
        frame
    }
    fn data_packet(
        &mut self,
        destination: [u8; 6],
        protocol: [u8; 2],
        payload: &[u8],
        protect: bool,
    ) -> Result<Vec<u8>> {
        let mut header = self.header([0x08, 1], destination);
        let mut body = LLC.to_vec();
        body.extend_from_slice(&protocol);
        body.extend_from_slice(payload);
        if protect {
            if let Some(keys) = &mut self.keys {
                body = keys.encrypt(&mut header, &body)?;
            }
        }
        let mut frame = header.to_vec();
        frame.extend_from_slice(&body);
        Ok(frame)
    }
    fn inbound<'a>(&self, frame: &'a [u8]) -> Option<(&'a [u8], &'a [u8])> {
        if frame.len() < 24
            || frame[0] & 0x0f != 8
            || frame[0] & 0x40 != 0
            || frame[1] & 0x87 != 0x02
            || frame[22] & 15 != 0
            || (frame[4..10] != self.mac && frame[4] & 1 == 0)
            || frame[10..16] != self.ap
            || (frame[0] & 0x80 != 0 && frame.get(24).is_none_or(|byte| byte & 0x80 != 0))
        {
            return None;
        }
        frame.split_at_checked(if frame[0] & 0x80 == 0 { 24 } else { 26 })
    }
    pub fn service(&mut self, usb: &mut impl Usb) -> Result {
        let mut bytes = [0; RX_TRANSFER];
        if let Some(length) = usb.receive(&mut bytes)? {
            for (frame, _) in usb_frames(&bytes[..length]) {
                if frame.len() >= 24 && frame[10..16] == self.ap {
                    if frame[0] == 0x80 && frame[16..22] == self.ap {
                        self.alive = hardware::millis();
                    } else if matches!(frame[0], 0xa0 | 0xc0)
                        && frame[4..10] == self.mac
                        && frame[16..22] == self.ap
                    {
                        return Err("Native Wi-Fi access point disconnected");
                    }
                }
                let Some((header, data)) = self.inbound(frame) else {
                    continue;
                };
                let decrypted;
                let body = if header[1] & 0x40 != 0 {
                    let Some(plain) = self
                        .keys
                        .as_mut()
                        .and_then(|keys| keys.decrypt(header, data))
                    else {
                        continue;
                    };
                    decrypted = plain;
                    &decrypted[..]
                } else {
                    data
                };
                if !body.starts_with(LLC) || body.len() < 8 {
                    continue;
                }
                if body[6..8] == EAPOL {
                    if header[4..10] == self.mac {
                        self.handle_eapol(usb, &body[8..], header[1] & 0x40 != 0)?;
                    }
                    continue;
                }
                if (self.keys.is_some() && header[1] & 0x40 == 0)
                    || body.len() > 1530
                    || self.received.len() >= 16
                {
                    continue;
                }
                let mut ethernet = Vec::with_capacity(body.len() + 6);
                ethernet.extend_from_slice(&header[4..10]);
                ethernet.extend_from_slice(&header[16..22]);
                ethernet.extend_from_slice(&body[6..]);
                self.received.push_back(ethernet);
                self.alive = hardware::millis();
            }
        }
        if hardware::millis().wrapping_sub(self.alive) > 7000 {
            return Err("Native Wi-Fi beacons lost");
        }
        Ok(())
    }
    fn handle_eapol(&mut self, usb: &mut impl Usb, eapol: &[u8], protected: bool) -> Result {
        let (Some(handshake), Some(keys)) = (&mut self.handshake, &mut self.keys) else {
            return Ok(());
        };
        let reply = if let Some(reply) = keys.replay_reply(eapol)? {
            Some((reply, None))
        } else {
            handshake.advance(eapol, Some(keys), hardware::millis())?
        };
        if let Some((reply, replacement)) = reply {
            let packet = self.data_packet(self.ap, EAPOL, &reply, protected)?;
            Registers(usb).send_packet(&packet, 17, self.rate)?;
            if let Some(keys) = replacement {
                // The access point is still using the old PTK until it receives message 4.
                self.keys = Some(keys);
                if let Some(handshake) = &mut self.handshake {
                    handshake.complete();
                }
            }
        }
        Ok(())
    }
    pub fn read_ethernet(&mut self, dst: &mut [u8]) -> Option<usize> {
        let frame = self.received.pop_front()?;
        dst.get_mut(..frame.len())?.copy_from_slice(&frame);
        Some(frame.len())
    }
    pub fn write_ethernet(&mut self, usb: &mut impl Usb, frame: &[u8]) -> Result {
        if !(14..=1536).contains(&frame.len()) || frame[6..12] != self.mac {
            return Err("Invalid native Wi-Fi Ethernet frame");
        }
        let packet = self.data_packet(
            frame[..6].try_into().unwrap(),
            frame[12..14].try_into().unwrap(),
            &frame[14..],
            true,
        )?;
        Registers(usb).send_packet(&packet, 17, self.rate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn station_wraps_ethernet_and_checks_infrastructure_frame_addresses() {
        let mac = [2, 1, 2, 3, 4, 5];
        let ap = [2, 6, 7, 8, 9, 10];
        let target = [2, 11, 12, 13, 14, 15];
        let mut station = Station {
            mac,
            ap,
            rate: 0,
            seq: 1,
            keys: None,
            handshake: None,
            received: VecDeque::new(),
            alive: 0,
        };
        let packet = station.data_packet(target, [8, 0], b"IPv4", true).unwrap();
        assert_eq!(&packet[..2], &[8, 1]);
        assert_eq!(&packet[4..10], &ap);
        assert_eq!(&packet[10..16], &mac);
        assert_eq!(&packet[16..22], &target);
        assert_eq!(&packet[24..32], &[0xaa, 0xaa, 3, 0, 0, 0, 8, 0]);
        assert!(station.inbound(&packet).is_none());
        let mut downlink = packet;
        downlink[1] = 2;
        downlink[4..10].copy_from_slice(&mac);
        downlink[10..16].copy_from_slice(&ap);
        assert_eq!(
            station.inbound(&downlink).unwrap().1,
            &[0xaa, 0xaa, 3, 0, 0, 0, 8, 0, b'I', b'P', b'v', b'4']
        );
        downlink[10] ^= 2;
        assert!(station.inbound(&downlink).is_none());
    }
}
