//! WPA2-Personal handshake and CCMP for the native Realtek station.
use super::Result;
use aes::Aes128;
use aes_kw::KekAes128;
use alloc::vec::Vec;
use ccm::{
    aead::{AeadInPlace, KeyInit},
    consts::{U13, U8},
    Ccm,
};
use hmac::{Hmac, Mac};
use sha1::Sha1;
use subtle::ConstantTimeEq;
use zeroize::{Zeroize, Zeroizing};

type HmacSha1 = Hmac<Sha1>;
type Ccmp = Ccm<Aes128, U8, U13>;
const HEADER: usize = 99;
const ACK: u16 = 1 << 7;
const MIC: u16 = 1 << 8;
const INSTALL: u16 = 1 << 6;
const SECURE: u16 = 1 << 9;
const ENCRYPTED: u16 = 1 << 12;
const PAIRWISE: u16 = 1 << 3;
const SUITE: [u8; 4] = [0, 15, 172, 4];
pub const RSN_PSK: [u8; 22] = [
    48, 20, 1, 0, 0, 15, 172, 4, 1, 0, 0, 15, 172, 4, 1, 0, 0, 15, 172, 2, 0, 0,
];

fn hmac(key: &[u8], bytes: &[u8]) -> [u8; 20] {
    let mut h = <HmacSha1 as Mac>::new_from_slice(key).unwrap();
    h.update(bytes);
    h.finalize().into_bytes().into()
}

pub fn accepts_rsn(mut data: &[u8]) -> bool {
    if data
        .get(..6)
        .is_none_or(|head| head[..2] != [1, 0] || head[2..] != SUITE)
    {
        return false;
    }
    data = &data[6..];
    let Some((ciphers, rest)) = suite_list(data) else {
        return false;
    };
    let Some((akms, caps)) = suite_list(rest) else {
        return false;
    };
    ciphers.chunks_exact(4).any(|entry| entry == SUITE)
        && akms.chunks_exact(4).any(|entry| entry == [0, 15, 172, 2])
        && caps.first().is_none_or(|first| first & 0x40 == 0)
}

fn suite_list(data: &[u8]) -> Option<(&[u8], &[u8])> {
    let count = u16::from_le_bytes(data.get(..2)?.try_into().ok()?) as usize;
    data.get(2..)?.split_at_checked(count * 4)
}

pub struct Handshake {
    pmk: Zeroizing<[u8; 32]>,
    sta: [u8; 6],
    ap: [u8; 6],
    challenge: Option<Challenge>,
}

struct Challenge {
    snonce: Zeroizing<[u8; 32]>,
    anonce: [u8; 32],
    replay: u64,
    ptk: Zeroizing<[u8; 48]>,
    started: u64,
}

struct KeyFrame<'a> {
    data: &'a [u8],
    flags: u16,
    replay: u64,
}

impl<'a> KeyFrame<'a> {
    fn parse(bytes: &'a [u8]) -> Option<Self> {
        if bytes.len() < HEADER || !matches!(bytes[0], 1..=3) || bytes[1] != 3 || bytes[4] != 2 {
            return None;
        }
        let length = u16::from_be_bytes(bytes[2..4].try_into().ok()?) as usize + 4;
        let key_len = u16::from_be_bytes(bytes[97..99].try_into().ok()?) as usize;
        if length != HEADER + key_len || length > 4096 {
            return None;
        }
        Some(Self {
            data: bytes.get(..length)?,
            flags: u16::from_be_bytes(bytes[5..7].try_into().ok()?),
            replay: u64::from_be_bytes(bytes[9..17].try_into().ok()?),
        })
    }

    fn reply(&self, flags: u16, nonce: Option<&[u8; 32]>, extra: &[u8], kck: &[u8]) -> Vec<u8> {
        let mut out = alloc::vec![0; HEADER + extra.len()];
        out[0] = self.data[0];
        out[1] = 3;
        out[2..4].copy_from_slice(&((HEADER + extra.len() - 4) as u16).to_be_bytes());
        out[4] = 2;
        out[5..7].copy_from_slice(&flags.to_be_bytes());
        out[9..17].copy_from_slice(&self.replay.to_be_bytes());
        if let Some(nonce) = nonce {
            out[17..49].copy_from_slice(nonce);
        }
        out[97..99].copy_from_slice(&(extra.len() as u16).to_be_bytes());
        out[HEADER..].copy_from_slice(extra);
        let mic = hmac(kck, &out);
        out[81..97].copy_from_slice(&mic[..16]);
        out
    }

    fn validate_mic(&self, kck: &[u8]) -> bool {
        let mut data = self.data.to_vec();
        data[81..97].fill(0);
        bool::from(hmac(kck, &data)[..16].ct_eq(&self.data[81..97]))
    }
}

fn derive_ptk(
    pmk: &[u8; 32],
    sta: [u8; 6],
    ap: [u8; 6],
    snonce: &[u8; 32],
    anonce: &[u8; 32],
) -> Zeroizing<[u8; 48]> {
    let label = b"Pairwise key expansion";
    let mut input = [0u8; 100];
    input[..label.len()].copy_from_slice(label);
    let (low, high) = if sta < ap { (sta, ap) } else { (ap, sta) };
    input[23..29].copy_from_slice(&low);
    input[29..35].copy_from_slice(&high);
    let (low, high) = if snonce < anonce {
        (snonce, anonce)
    } else {
        (anonce, snonce)
    };
    input[35..67].copy_from_slice(low);
    input[67..99].copy_from_slice(high);
    let mut out = Zeroizing::new([0; 48]);
    for (counter, chunk) in out.chunks_mut(20).enumerate() {
        input[99] = counter as u8;
        chunk.copy_from_slice(&hmac(pmk, &input)[..chunk.len()]);
    }
    input.zeroize();
    out
}

impl Handshake {
    pub fn new(ssid: &[u8], pass: &[u8], sta: [u8; 6], ap: [u8; 6]) -> Result<Self> {
        let mut pmk = Zeroizing::new([0u8; 32]);
        if pass.len() == 64 {
            for (dst, pair) in pmk.iter_mut().zip(pass.chunks_exact(2)) {
                let high = (pair[0] as char)
                    .to_digit(16)
                    .ok_or("Expected a hexadecimal WPA2 key")?;
                let low = (pair[1] as char)
                    .to_digit(16)
                    .ok_or("Expected a hexadecimal WPA2 key")?;
                *dst = ((high << 4) | low) as u8;
            }
        } else if (8..=63).contains(&pass.len()) {
            pbkdf2::pbkdf2_hmac::<Sha1>(pass, ssid, 4096, &mut pmk[..]);
        } else {
            return Err("WPA2 requires an 8-63 character password or a 64-digit key");
        }
        Ok(Self {
            pmk,
            sta,
            ap,
            challenge: None,
        })
    }

    pub fn complete(&mut self) {
        self.challenge = None;
    }

    pub fn advance(
        &mut self,
        bytes: &[u8],
        active: Option<&Keys>,
        now: u64,
    ) -> Result<Option<(Vec<u8>, Option<Keys>)>> {
        let Some(frame) = KeyFrame::parse(bytes) else {
            return Ok(None);
        };
        if self
            .challenge
            .as_ref()
            .is_some_and(|c| now.saturating_sub(c.started) >= 15_000)
        {
            self.complete();
        }
        if active.is_some_and(|keys| frame.replay <= keys.replay) {
            return Ok(None);
        }
        let first = 2 | ACK | PAIRWISE;
        if frame.flags == first || (active.is_some() && frame.flags == first | SECURE) {
            let anonce: [u8; 32] = frame.data[17..49].try_into().unwrap();
            if anonce == [0; 32]
                || active.is_some_and(|keys| keys.anonce == anonce)
                || self
                    .challenge
                    .as_ref()
                    .is_some_and(|c| c.anonce == anonce && frame.replay < c.replay)
            {
                return Ok(None);
            }
            if self.challenge.as_ref().is_none_or(|c| c.anonce != anonce) {
                let mut snonce = Zeroizing::new([0u8; 32]);
                getrandom::getrandom(&mut snonce[..]).map_err(|_| "No entropy for WPA2")?;
                let ptk = derive_ptk(&self.pmk, self.sta, self.ap, &snonce, &anonce);
                self.challenge = Some(Challenge {
                    snonce,
                    anonce,
                    replay: frame.replay,
                    ptk,
                    started: now,
                });
            }
            let challenge = self.challenge.as_mut().unwrap();
            challenge.replay = frame.replay;
            return Ok(Some((
                frame.reply(
                    2 | PAIRWISE | MIC | (frame.flags & SECURE),
                    Some(&challenge.snonce),
                    &RSN_PSK,
                    &challenge.ptk[..16],
                ),
                None,
            )));
        }
        let Some(challenge) = &self.challenge else {
            return Ok(None);
        };
        if frame.flags != 2 | ACK | PAIRWISE | MIC | INSTALL | SECURE | ENCRYPTED
            || frame.replay <= challenge.replay
            || frame.data[17..49] != challenge.anonce
            || !frame.validate_mic(&challenge.ptk[..16])
        {
            return Ok(None);
        }
        let offered_group = unwrap_group(&frame, &challenge.ptk[16..32], true)?;
        let (group, group_id) = offered_group
            .or_else(|| active.map(|keys| (keys.group, keys.group_id)))
            .ok_or("No CCMP group key received")?;
        let pair: [u8; 16] = challenge.ptk[32..48].try_into().unwrap();
        if active.is_some_and(|keys| bool::from(pair.ct_eq(&keys.pair))) {
            return Ok(None);
        }
        let response = frame.reply(2 | PAIRWISE | MIC | SECURE, None, &[], &challenge.ptk[..16]);
        let group_floor = offered_group.map_or(0, |_| group_rx_counter(&frame));
        let rx_group = active
            .filter(|keys| bool::from(group.ct_eq(&keys.group)))
            .map_or([group_floor; 16], |keys| {
                keys.rx_group.map(|last| last.max(group_floor))
            });
        let keys = Keys {
            pair,
            group,
            group_id,
            tx_pn: 0,
            rx_pair: [0; 16],
            rx_group,
            kck: challenge.ptk[..16].try_into().unwrap(),
            kek: challenge.ptk[16..32].try_into().unwrap(),
            anonce: challenge.anonce,
            replay: frame.replay,
        };
        Ok(Some((response, Some(keys))))
    }
}

fn group_rx_counter(frame: &KeyFrame<'_>) -> u64 {
    let mut raw = [0u8; 8];
    raw[..6].copy_from_slice(&frame.data[65..71]);
    u64::from_le_bytes(raw)
}

fn unwrap_group(
    frame: &KeyFrame<'_>,
    kek: &[u8],
    require_rsn: bool,
) -> Result<Option<([u8; 16], u8)>> {
    let wrapped = &frame.data[HEADER..];
    if !(24..=1032).contains(&wrapped.len()) || wrapped.len() % 8 != 0 {
        return Err("Invalid WPA2 wrapped key length");
    }
    let mut plain = Zeroizing::new([0u8; 1024]);
    let length = wrapped.len() - 8;
    let kek = KekAes128::try_from(kek).map_err(|_| "Invalid WPA2 key encryption key")?;
    kek.unwrap(wrapped, &mut plain[..length])
        .map_err(|_| "WPA2 key integrity check failed")?;
    let mut remaining = &plain[..length];
    let mut rsn_ok = !require_rsn;
    let mut group = None;
    while let Some((&id, rest)) = remaining.split_first() {
        if id == 0xdd && rest.iter().all(|&b| b == 0) {
            break;
        }
        let (&len, rest) = rest.split_first().ok_or("Invalid WPA2 key data")?;
        let (entry, tail) = rest
            .split_at_checked(usize::from(len))
            .ok_or("Truncated WPA2 key data")?;
        if id == 48 {
            rsn_ok |= accepts_rsn(entry);
        }
        if id == 221 && entry.len() == 22 && entry[..4] == [0, 15, 172, 1] {
            if group.is_some() {
                return Err("Duplicate WPA2 group key");
            }
            group = Some((entry[6..22].try_into().unwrap(), entry[4] & 3));
        }
        remaining = tail;
    }
    if !rsn_ok {
        return Err("Access point changed its WPA2 parameters");
    }
    Ok(group)
}

pub struct Keys {
    pair: [u8; 16],
    group: [u8; 16],
    group_id: u8,
    tx_pn: u64,
    rx_pair: [u64; 16],
    rx_group: [u64; 16],
    kck: [u8; 16],
    kek: [u8; 16],
    anonce: [u8; 32],
    replay: u64,
}

impl Drop for Keys {
    fn drop(&mut self) {
        self.pair.zeroize();
        self.group.zeroize();
        self.kck.zeroize();
        self.kek.zeroize();
    }
}

impl Keys {
    pub fn replay_reply(&mut self, eapol: &[u8]) -> Result<Option<Vec<u8>>> {
        let Some(frame) = KeyFrame::parse(eapol) else {
            return Ok(None);
        };
        if frame.replay < self.replay || !frame.validate_mic(&self.kck) {
            return Ok(None);
        }
        if frame.flags == 2 | ACK | PAIRWISE | MIC | INSTALL | SECURE | ENCRYPTED
            && frame.data[17..49] == self.anonce
        {
            // A retransmitted message 3 can carry a fresh replay counter.
            // Acknowledge it without reinstalling keys or resetting CCMP counters.
            self.replay = frame.replay;
            return Ok(Some(frame.reply(
                2 | PAIRWISE | MIC | SECURE,
                None,
                &[],
                &self.kck,
            )));
        }
        if frame.flags == 2 | ACK | MIC | SECURE | ENCRYPTED {
            let (group, id) =
                unwrap_group(&frame, &self.kek, false)?.ok_or("No CCMP group key received")?;
            let pn = group_rx_counter(&frame);
            if !bool::from(group.ct_eq(&self.group)) {
                if frame.replay <= self.replay {
                    return Ok(None);
                }
                self.group.zeroize();
                self.group = group;
                self.rx_group.fill(pn);
            } else if frame.replay > self.replay {
                self.rx_group = self.rx_group.map(|last| last.max(pn));
            }
            if frame.replay > self.replay {
                self.group_id = id;
            }
            self.replay = frame.replay;
            return Ok(Some(frame.reply(2 | MIC | SECURE, None, &[], &self.kck)));
        }
        Ok(None)
    }

    pub fn encrypt(&mut self, header: &mut [u8; 24], plain: &[u8]) -> Result<Vec<u8>> {
        self.tx_pn += 1;
        if self.tx_pn >= 1u64 << 48 {
            return Err("WPA2 packet counter exhausted");
        }
        let pn = self.tx_pn.to_le_bytes();
        header[1] |= 0x40;
        let (nonce, aad) = ccmp_parts(header, &pn, 0);
        let cipher = Ccmp::new((&self.pair).into());
        let mut out = Vec::with_capacity(8 + plain.len() + 8);
        out.extend_from_slice(&[pn[0], pn[1], 0, 0x20, pn[2], pn[3], pn[4], pn[5]]);
        out.extend_from_slice(plain);
        let tag = cipher
            .encrypt_in_place_detached((&nonce).into(), &aad[..22], &mut out[8..])
            .map_err(|_| "CCMP encryption failed")?;
        out.extend_from_slice(&tag);
        Ok(out)
    }

    pub fn decrypt(&mut self, header: &[u8], body: &[u8]) -> Option<Vec<u8>> {
        let group = header.get(4)? & 1 != 0;
        let keyid = *body.get(3)?;
        if body.get(2) != Some(&0)
            || keyid & 0x3f != 0x20
            || keyid >> 6 != if group { self.group_id } else { 0 }
        {
            return None;
        }
        let pn = [
            body[0],
            body[1],
            *body.get(4)?,
            *body.get(5)?,
            *body.get(6)?,
            *body.get(7)?,
            0,
            0,
        ];
        let tid = if header.len() == 26 {
            header[24] & 15
        } else {
            0
        };
        let (key, last) = if group {
            (&self.group, &mut self.rx_group[usize::from(tid)])
        } else {
            (&self.pair, &mut self.rx_pair[usize::from(tid)])
        };
        let counter = u64::from_le_bytes(pn);
        if counter <= *last || body.len() < 16 {
            return None;
        }
        let (nonce, aad) = ccmp_parts(header, &pn, tid);
        let len = body.len() - 8;
        let tag: &[u8; 8] = body[len..].try_into().ok()?;
        let mut out = body[8..len].to_vec();
        Ccmp::new(key.into())
            .decrypt_in_place_detached(
                (&nonce).into(),
                &aad[..if header.len() == 26 { 24 } else { 22 }],
                &mut out,
                tag.into(),
            )
            .ok()?;
        *last = counter;
        Some(out)
    }
}

fn ccmp_parts(header: &[u8], pn: &[u8; 8], tid: u8) -> ([u8; 13], [u8; 24]) {
    let mut nonce = [0; 13];
    nonce[0] = tid;
    nonce[1..7].copy_from_slice(&header[10..16]);
    for (dst, src) in nonce[7..].iter_mut().zip(pn[..6].iter().rev()) {
        *dst = *src;
    }
    let mut aad = [0; 24];
    aad[0] = header[0] & 0x8f;
    aad[1] = header[1] & 0xc7;
    aad[2..20].copy_from_slice(&header[4..22]);
    aad[20] = header[22] & 15;
    aad[22] = tid;
    (nonce, aad)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(
        flags: u16,
        replay: u64,
        nonce: &[u8; 32],
        extra: &[u8],
        key: Option<&[u8]>,
    ) -> Vec<u8> {
        let length = HEADER + extra.len();
        let mut out = alloc::vec![0; length];
        out[..2].copy_from_slice(&[2, 3]);
        out[2..4].copy_from_slice(&((length - 4) as u16).to_be_bytes());
        out[4] = 2;
        out[5..7].copy_from_slice(&flags.to_be_bytes());
        out[9..17].copy_from_slice(&replay.to_be_bytes());
        out[17..49].copy_from_slice(nonce);
        out[97..99].copy_from_slice(&(extra.len() as u16).to_be_bytes());
        out[HEADER..].copy_from_slice(extra);
        if let Some(key) = key {
            let mic = hmac(key, &out);
            out[81..97].copy_from_slice(&mic[..16]);
        }
        out
    }

    #[test]
    fn validates_wpa2_mic_installs_ccmp_once_and_rejects_replayed_data() {
        let sta = [2, 1, 2, 3, 4, 5];
        let ap = [2, 6, 7, 8, 9, 10];
        let mut handshake = Handshake::new(b"IEEE", b"password", sta, ap).unwrap();
        assert_eq!(
            &handshake.pmk[..],
            &[
                0xf4, 0x2c, 0x6f, 0xc5, 0x2d, 0xf0, 0xeb, 0xef, 0x9e, 0xbb, 0x4b, 0x90, 0xb3, 0x8a,
                0x5f, 0x90, 0x2e, 0x83, 0xfe, 0x1b, 0x13, 0x5a, 0x70, 0xe2, 0x3a, 0xed, 0x76, 0x2e,
                0x97, 0x10, 0xa1, 0x2e,
            ]
        );
        let anonce = [11; 32];
        let (reply, keys) = handshake
            .advance(&message(2 | ACK | PAIRWISE, 3, &anonce, &[], None), None, 1)
            .unwrap()
            .unwrap();
        assert!(keys.is_none());
        let ptk = derive_ptk(
            &handshake.pmk,
            sta,
            ap,
            reply[17..49].try_into().unwrap(),
            &anonce,
        );
        assert!(KeyFrame::parse(&reply).unwrap().validate_mic(&ptk[..16]));
        let mut payload = [0; 48];
        payload[..22].copy_from_slice(&RSN_PSK);
        payload[22..30].copy_from_slice(&[0xdd, 22, 0, 15, 172, 1, 1, 0]);
        payload[30..46].fill(0x39);
        payload[46] = 0xdd;
        let kek = KekAes128::try_from(&ptk[16..32]).unwrap();
        let mut wrapped = [0; 56];
        kek.wrap(&payload, &mut wrapped).unwrap();
        let flags = 2 | ACK | PAIRWISE | MIC | INSTALL | SECURE | ENCRYPTED;
        let mut bad = message(flags, 4, &anonce, &wrapped, Some(&ptk[..16]));
        bad[81] ^= 1;
        assert!(handshake.advance(&bad, None, 2).unwrap().is_none());
        let (reply, keys) = handshake
            .advance(
                &message(flags, 4, &anonce, &wrapped, Some(&ptk[..16])),
                None,
                2,
            )
            .unwrap()
            .unwrap();
        assert!(KeyFrame::parse(&reply).unwrap().validate_mic(&ptk[..16]));
        let mut keys = keys.unwrap();
        assert_eq!(keys.group, [0x39; 16]);
        let mut header = [0u8; 24];
        header[..2].copy_from_slice(&[8, 2]);
        header[4..10].copy_from_slice(&sta);
        header[10..16].copy_from_slice(&ap);
        let protected = keys.encrypt(&mut header, b"DHCP payload").unwrap();
        assert_eq!(keys.decrypt(&header, &protected).unwrap(), b"DHCP payload");
        assert!(keys
            .replay_reply(&message(flags, 5, &anonce, &wrapped, Some(&ptk[..16])))
            .unwrap()
            .is_some());
        assert_eq!(keys.tx_pn, 1);
        assert!(keys.decrypt(&header, &protected).is_none());
        let mut tampered = keys.encrypt(&mut header, b"new packet").unwrap();
        tampered[9] ^= 1;
        assert!(keys.decrypt(&header, &tampered).is_none());
        assert_eq!(keys.rx_pair[0], 1);
    }

    #[test]
    fn pairwise_rekey_requires_a_fresh_nonce_and_keeps_unchanged_group_counters() {
        let sta = [2, 1, 2, 3, 4, 5];
        let ap = [2, 6, 7, 8, 9, 10];
        let mut handshake = Handshake::new(b"IEEE", b"password", sta, ap).unwrap();
        let mut active = Keys {
            pair: [0x41; 16],
            group: [0x39; 16],
            group_id: 1,
            tx_pn: 55,
            rx_pair: [32; 16],
            rx_group: [127; 16],
            kck: [0x81; 16],
            kek: [0x43; 16],
            anonce: [0x11; 32],
            replay: 9,
        };
        active.rx_group[2] = 200;
        assert!(handshake
            .advance(
                &message(2 | ACK | PAIRWISE, 10, &active.anonce, &[], None),
                Some(&active),
                1,
            )
            .unwrap()
            .is_none());
        let anonce = [0x55; 32];
        assert!(handshake
            .advance(
                &message(2 | ACK | PAIRWISE, 8, &anonce, &[], None),
                Some(&active),
                1,
            )
            .unwrap()
            .is_none());
        let first = message(2 | ACK | PAIRWISE | SECURE, 11, &anonce, &[], None);
        let (second, keys) = handshake
            .advance(&first, Some(&active), 50)
            .unwrap()
            .unwrap();
        assert!(keys.is_none());
        assert_eq!(
            u16::from_be_bytes(second[5..7].try_into().unwrap()),
            2 | PAIRWISE | MIC | SECURE
        );
        let ptk = derive_ptk(
            &handshake.pmk,
            sta,
            ap,
            second[17..49].try_into().unwrap(),
            &anonce,
        );
        let mut payload = [0; 48];
        payload[..22].copy_from_slice(&RSN_PSK);
        payload[22..30].copy_from_slice(&[0xdd, 22, 0, 15, 172, 1, 2, 0]);
        payload[30..46].copy_from_slice(&active.group);
        payload[46] = 0xdd;
        let mut wrapped = [0; 56];
        KekAes128::try_from(&ptk[16..32])
            .unwrap()
            .wrap(&payload, &mut wrapped)
            .unwrap();
        let third = message(
            2 | ACK | PAIRWISE | MIC | INSTALL | SECURE | ENCRYPTED,
            12,
            &anonce,
            &wrapped,
            Some(&ptk[..16]),
        );
        let (fourth, replacement) = handshake
            .advance(&third, Some(&active), 100)
            .unwrap()
            .unwrap();
        assert!(KeyFrame::parse(&fourth).unwrap().validate_mic(&ptk[..16]));
        let replacement = replacement.unwrap();
        assert_ne!(replacement.pair, active.pair);
        assert_eq!(replacement.tx_pn, 0);
        assert_eq!(replacement.rx_pair, [0; 16]);
        assert_eq!(replacement.rx_group, active.rx_group);
        assert_eq!(replacement.group_id, 2);
        assert_eq!(active.tx_pn, 55);
        assert!(handshake
            .advance(&third, Some(&active), 15_050)
            .unwrap()
            .is_none());
    }
}
