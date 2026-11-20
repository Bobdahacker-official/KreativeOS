//! AX210 bootstrap DMA payloads: IML, init LMAC/UMAC sections and paging.
const PAGE: usize = 4096;
const RX_ENTRIES: usize = 512;
const RX_BUFFER_SIZE: usize = 4096;
const COMMAND_ENTRIES: usize = 128;
const COMMAND_DESCRIPTOR_SIZE: usize = 256;
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
    pub context: Region,
    pub scratch: Region,
}

pub static mut BOOTSTRAP: Option<Bootstrap> = None;

fn reserve(arena: &mut [u8], base: u64, used: &mut usize, bytes: usize) -> Option<Region> {
    let start = used.checked_add(PAGE - 1)? & !(PAGE - 1);
    let end = start.checked_add(bytes)?;
    let padded = end.checked_add(PAGE - 1)? & !(PAGE - 1);
    arena.get_mut(start..padded)?.fill(0);
    *used = padded;
    Some(Region {
        address: base.checked_add(start as u64)?,
        bytes,
    })
}

fn stage(arena: &mut [u8], base: u64, used: &mut usize, bytes: &[u8]) -> Option<Region> {
    let region = reserve(arena, base, used, bytes.len())?;
    let start = (region.address - base) as usize;
    arena[start..start + bytes.len()].copy_from_slice(bytes);
    Some(region)
}

fn write_field<const N: usize>(record: &mut [u8], offset: usize, value: [u8; N]) {
    record[offset..offset + N].copy_from_slice(&value);
}

// Intel Typhoon Peak context ABI from FreeBSD iwlwifi, pcie/iwl-context-info-v2.h.
// Model the packed records as little-endian bytes, including unaligned addresses.
fn prepare_context(
    arena: &mut [u8],
    base: u64,
    used: &mut usize,
    out: &mut Bootstrap,
) -> Option<()> {
    const CONTEXT_SIZE: usize = 104;
    const DRAM_MAP: usize = 124;
    const DRAM_TABLE_BYTES: usize = MAX_SECTIONS * 8;
    const SCRATCH_COMMON_SIZE: usize = DRAM_MAP + 3 * DRAM_TABLE_BYTES;
    const SCRATCH_SIZE: usize = SCRATCH_COMMON_SIZE + 8 * 8;
    const SCRATCH_FLAGS: u32 = (1 << 1) | (1 << 16) | (1 << 17) | (3 << 18);

    out.context = reserve(arena, base, used, CONTEXT_SIZE)?;
    out.scratch = reserve(arena, base, used, SCRATCH_SIZE)?;
    let info = reserve(arena, base, used, PAGE)?;
    let rx_status = reserve(arena, base, used, 12)?;
    let rx_free = reserve(arena, base, used, RX_ENTRIES * 16)?;
    let rx_used = reserve(arena, base, used, RX_ENTRIES * 32)?;
    let command = reserve(arena, base, used, COMMAND_ENTRIES * COMMAND_DESCRIPTOR_SIZE)?;
    let packets = reserve(arena, base, used, (RX_ENTRIES - 1) * RX_BUFFER_SIZE)?;

    let start = (rx_free.address - base) as usize;
    let descriptors = &mut arena[start..start + rx_free.bytes];
    for (index, descriptor) in descriptors
        .chunks_exact_mut(16)
        .take(RX_ENTRIES - 1)
        .enumerate()
    {
        write_field(descriptor, 0, ((index + 1) as u16).to_le_bytes());
        let address = packets.address + (index * RX_BUFFER_SIZE) as u64;
        write_field(descriptor, 8, address.to_le_bytes());
    }

    let start = (out.scratch.address - base) as usize;
    let scratch = &mut arena[start..start + SCRATCH_SIZE];
    write_field(scratch, 4, ((SCRATCH_SIZE / 4) as u16).to_le_bytes());
    write_field(scratch, 8, SCRATCH_FLAGS.to_le_bytes());
    write_field(scratch, 48, rx_free.address.to_le_bytes());
    for (index, table) in [&out.umac, &out.lmac, &out.paging].into_iter().enumerate() {
        let start = DRAM_MAP + index * DRAM_TABLE_BYTES;
        for (entry, region) in table.iter().enumerate() {
            write_field(scratch, start + entry * 8, region.address.to_le_bytes());
        }
    }

    let start = (out.context.address - base) as usize;
    let context = &mut arena[start..start + CONTEXT_SIZE];
    write_field(context, 8, info.address.to_le_bytes());
    write_field(context, 16, rx_status.address.to_le_bytes());
    write_field(
        context,
        24,
        (info.address + (PAGE / 2) as u64).to_le_bytes(),
    );
    write_field(
        context,
        32,
        (info.address + (3 * PAGE / 4) as u64).to_le_bytes(),
    );
    write_field(context, 52, command.address.to_le_bytes());
    write_field(context, 60, rx_used.address.to_le_bytes());
    write_field(
        context,
        68,
        (COMMAND_ENTRIES.ilog2() as u16 - 3).to_le_bytes(),
    );
    write_field(context, 70, (RX_ENTRIES.ilog2() as u16).to_le_bytes());
    write_field(context, 88, out.scratch.address.to_le_bytes());
    write_field(context, 96, (SCRATCH_COMMON_SIZE as u32).to_le_bytes());
    Some(())
}

impl Bootstrap {
    /// The scratch memory remains owned by the host until firmware boot.
    pub unsafe fn set_mac_revision(&self, revision: u16) {
        (self.scratch.address as *mut u16).write_volatile(revision.to_le());
    }
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
        context: EMPTY,
        scratch: EMPTY,
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
    if out.iml.bytes == 0 || out.counts[0] == 0 || out.counts[1] == 0 {
        return None;
    }
    prepare_context(arena, base, &mut used, &mut out)?;
    Some(out)
}
