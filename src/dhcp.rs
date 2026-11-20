//! DHCP client packets and lease parsing (RFC 2131 and RFC 2132).
pub type Address = [u8; 4];

#[derive(Clone, Copy)]
pub struct Config {
    pub ip: Address,
    pub mask: Address,
    pub router: Address,
    pub dns: Address,
}

impl Config {
    pub const QEMU: Self = Self {
        ip: [10, 0, 2, 15],
        mask: [255, 255, 255, 0],
        router: [10, 0, 2, 2],
        dns: [10, 0, 2, 3],
    };
}

#[derive(Clone, Copy)]
pub struct Offer {
    pub config: Config,
    pub server: Address,
    pub seconds: u32,
}

pub fn address(text: &str) -> Option<Address> {
    let mut parts = text.split('.');
    let mut result = [0; 4];
    for octet in &mut result {
        *octet = parts.next()?.parse().ok()?;
    }
    parts.next().is_none().then_some(result)
}

pub fn request(xid: u32, mac: [u8; 6], offer: Option<Offer>) -> [u8; 300] {
    let mut packet = [0; 300];
    packet[..3].copy_from_slice(&[1, 1, 6]);
    packet[4..8].copy_from_slice(&xid.to_be_bytes());
    packet[10] = 0x80;
    packet[28..34].copy_from_slice(&mac);
    packet[236..240].copy_from_slice(&[99, 130, 83, 99]);
    let mut position = 240;
    for option in [
        &[53, 1, if offer.is_some() { 3 } else { 1 }][..],
        &[55, 4, 1, 3, 6, 51],
    ] {
        packet[position..position + option.len()].copy_from_slice(option);
        position += option.len();
    }
    if let Some(offer) = offer {
        packet[position..position + 2].copy_from_slice(&[50, 4]);
        packet[position + 2..position + 6].copy_from_slice(&offer.config.ip);
        packet[position + 6..position + 8].copy_from_slice(&[54, 4]);
        packet[position + 8..position + 12].copy_from_slice(&offer.server);
        position += 12;
    }
    packet[position] = 255;
    packet
}

pub fn reply(packet: &[u8], xid: u32, mac: [u8; 6]) -> Option<(u8, Offer)> {
    if packet.get(..3)? != [2, 1, 6]
        || packet.get(4..8)? != xid.to_be_bytes()
        || packet.get(28..34)? != mac
        || packet.get(236..240)? != [99, 130, 83, 99]
    {
        return None;
    }
    let mut offer = Offer {
        config: Config {
            ip: packet[16..20].try_into().ok()?,
            mask: [0; 4],
            router: [0; 4],
            dns: [0; 4],
        },
        server: [0; 4],
        seconds: 0,
    };
    let mut message = 0;
    let mut options = &packet[240..];
    while let Some((&kind, rest)) = options.split_first() {
        options = rest;
        match kind {
            0 => continue,
            255 => break,
            _ => {}
        }
        let (&len, rest) = options.split_first()?;
        let value = rest.get(..len as usize)?;
        options = &rest[len as usize..];
        match (kind, value) {
            (53, [value]) => message = *value,
            (1, [a, b, c, d]) => offer.config.mask = [*a, *b, *c, *d],
            (3, [a, b, c, d, ..]) => offer.config.router = [*a, *b, *c, *d],
            (6, [a, b, c, d, ..]) => offer.config.dns = [*a, *b, *c, *d],
            (54, [a, b, c, d]) => offer.server = [*a, *b, *c, *d],
            (51, [a, b, c, d]) => offer.seconds = u32::from_be_bytes([*a, *b, *c, *d]),
            _ => {}
        }
    }
    (message != 0).then_some((message, offer))
}
