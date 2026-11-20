//! AX210 bootstrap DMA payloads: IML, init LMAC/UMAC sections and paging.
const PAGE: usize = 4096;
const MAX_SECTIONS: usize = 64;
pub const ARENA_SIZE: usize = 6 * 1024 * 1024;

#[derive(Clone, Copy)]
pub struct Region {
    pub address: u64,
    pub bytes: usize,
}
const EMPTY: Region = Region {
    address: 0,
    bytes: 0,
};

pub struct Bootstrap {
    pub iml: Region,
    pub lmac: [Region; MAX_SECTIONS],
    pub umac: [Region; MAX_SECTIONS],
    pub paging: [Region; MAX_SECTIONS],
    pub counts: [usize; 3],
}

pub static mut BOOTSTRAP: Option<Bootstrap> = None;

fn stage(arena: &mut [u8], base: u64, used: &mut usize, bytes: &[u8]) -> Option<Region> {
    let start = used.checked_add(PAGE - 1)? & !(PAGE - 1);
    let end = start.checked_add(bytes.len())?;
    let padded = end.checked_add(PAGE - 1)? & !(PAGE - 1);
    let target = arena.get_mut(start..padded)?;
    target.fill(0);
    target[..bytes.len()].copy_from_slice(bytes);
    *used = padded;
    Some(Region {
        address: base.checked_add(start as u64)?,
        bytes: bytes.len(),
    })
}

pub fn prepare(firmware: &[u8], arena: &mut [u8], base: u64) -> Option<Bootstrap> {
    if base == 0 || base & (PAGE as u64 - 1) != 0 || base.checked_add(arena.len() as u64)? > 1 << 32
    {
        return None;
    }
    let word = |bytes: &[u8]| Some(u32::from_le_bytes(bytes.get(..4)?.try_into().ok()?));
    if firmware.get(..8)? != b"\0\0\0\0IWL\n" {
        return None;
    }
    let mut rest = firmware.get(88..)?;
    let mut used = 0;
    let mut group = 0;
    let mut out = Bootstrap {
        iml: EMPTY,
        lmac: [EMPTY; MAX_SECTIONS],
        umac: [EMPTY; MAX_SECTIONS],
        paging: [EMPTY; MAX_SECTIONS],
        counts: [0; 3],
    };
    while !rest.is_empty() {
        let kind = word(rest)?;
        let size = word(rest.get(4..)?)? as usize;
        let bytes = rest.get(8..8usize.checked_add(size)?)?;
        let padded = size.checked_add(3)? & !3;
        rest = rest.get(8usize.checked_add(padded)?..)?;
        match kind {
            52 => {
                if out.iml.bytes != 0 || bytes.is_empty() {
                    return None;
                }
                out.iml = stage(arena, base, &mut used, bytes)?;
            }
            20 | 25 => {
                let offset = word(bytes)?;
                let payload = bytes.get(4..)?;
                match offset {
                    0xffff_cccc if group == 0 => group = 1,
                    0xaaaa_bbbb if group == 1 => group = 2,
                    0xffff_cccc | 0xaaaa_bbbb => return None,
                    _ => {
                        if payload.is_empty() {
                            return None;
                        }
                        let index = out.counts[group];
                        let table = match group {
                            0 => &mut out.lmac,
                            1 => &mut out.umac,
                            _ => &mut out.paging,
                        };
                        *table.get_mut(index)? = stage(arena, base, &mut used, payload)?;
                        out.counts[group] += 1;
                    }
                }
            }
            _ => {}
        }
    }
    (out.iml.bytes != 0 && out.counts[0] != 0 && out.counts[1] != 0).then_some(out)
}
