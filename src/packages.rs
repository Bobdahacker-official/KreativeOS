use crate::{storage, text::Text};
use ed25519_dalek::{Signature, VerifyingKey};

const PUBLISHER: &str = "6f7abf9c3992958403518047e44cc6cabdb5fbc5f7cc568f71e2e4de8db73a31";

pub struct Package {
    pub name: &'static str,
    pub source: &'static str,
    pub installed: &'static str,
    bytes: &'static [u8],
    signature: &'static str,
}

pub const CATALOG: [Package; 2] = [
    Package {
        name: "Kreative Guide",
        source: "Guide.kapp",
        installed: "Guide.app",
        bytes: include_bytes!("../packages/Guide.kapp"),
        signature: "ca1cf2b249a42c0d0cccdba4e06a1fff08675df6e3fcc052a310a200f311e925f0e9a93c6ab080ca83d5ed1b6e268e4cb32f9b78ff2d53d050374e2bb4dffb0f",
    },
    Package {
        name: "Kreative SDK",
        source: "SDK.kapp",
        installed: "SDK.app",
        bytes: include_bytes!("../packages/SDK.kapp"),
        signature: "ab5b28b17cf8cd15b4ed022d0e11e0ab1f12fd38c0990ea67477f41de4017dd1d7f9d4d6caf55616878670c9344addfae51da7b17c59795eb549ac51d42c8b09",
    },
];

fn hex<const N: usize>(text: &str) -> Option<[u8; N]> {
    if text.len() != 2 * N {
        return None;
    }
    let mut result = [0; N];
    for (index, byte) in result.iter_mut().enumerate() {
        *byte = u8::from_str_radix(text.get(index * 2..index * 2 + 2)?, 16).ok()?;
    }
    Some(result)
}

impl Package {
    fn verified<'a>(&self, bytes: &'a [u8]) -> Option<&'a str> {
        let key = VerifyingKey::from_bytes(&hex(PUBLISHER)?).ok()?;
        key.verify_strict(bytes, &Signature::from_bytes(&hex(self.signature)?))
            .ok()?;
        let (manifest, body) = core::str::from_utf8(bytes).ok()?.split_once("\n---\n")?;
        let mut fields = manifest.lines();
        if fields.next()? != "KAPP1"
            || fields.next()?.strip_prefix("name=")? != self.name
            || fields.next()? != "version=1"
            || fields.next().is_some()
        {
            return None;
        }
        Some(body)
    }
    pub fn open(&self) -> Option<Text<2048>> {
        let file = unsafe { storage::FILES[storage::find(self.installed)?].data };
        let body = self.verified(&file.bytes[..file.len])?;
        let mut document = Text::new();
        document.set(body);
        Some(document)
    }
    pub fn install(&self) -> Result<bool, &'static str> {
        let local = storage::find(self.source).map(|index| unsafe { storage::FILES[index].data });
        let bytes = local
            .as_ref()
            .map_or(self.bytes, |data| &data.bytes[..data.len]);
        self.verified(bytes)
            .ok_or("Package signature or manifest rejected")?;
        storage::put(self.installed, bytes)?;
        Ok(storage::sync())
    }
}
