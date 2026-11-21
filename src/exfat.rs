//! Native exFAT data volumes. Discovery and geometry are verified before writes are enabled.
use crate::storage::{self, File};
use alloc::vec::Vec;

pub const DRAFT: &str = "KOSDRAFT.KDR";
pub const DRAFT_PREFIX: &str = "@draft-";
const ROOT_LIMIT: usize = 4 * 1024 * 1024;
const BITMAP_LIMIT: usize = 8 * 1024 * 1024;
const EOC: u32 = 0xffff_fff8;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Geometry {
    pub start: u64,
    pub length: u64,
    pub sector: usize,
    pub recovery: bool,
    spc: u32,
    fat: u32,
    heap: u32,
    clusters: u32,
    root: u32,
}

impl Geometry {
    pub fn probe(
        start: u64,
        available: u64,
        sector: usize,
        read: &mut impl FnMut(u64, &mut [u8]) -> bool,
    ) -> Option<Self> {
        if available < 24 || !matches!(sector, 512 | 4096) || start.checked_add(available).is_none()
        {
            return None;
        }
        let mut boot = alloc::vec![0; sector];
        let mut expected = None;
        let mut recovery = false;
        for copy in [start, start + 12] {
            if !read(copy, &mut boot)
                || &boot[3..11] != b"EXFAT   "
                || boot[11..64].iter().any(|&byte| byte != 0)
                || (qword(&boot, 64) != 0 && qword(&boot, 64) != start)
                || word(&boot, 104) != 0x100
                || (copy == start && word(&boot, 106) & 1 != 0)
                || boot[108] != sector.trailing_zeros() as u8
                || boot[109] > 25 - boot[108]
                || boot[110] != 1
                || boot[510..512] != [0x55, 0xaa]
            {
                return None;
            }
            if copy == start {
                recovery = word(&boot, 106) & 6 != 0;
            }
            let volume = Self {
                start,
                length: qword(&boot, 72),
                sector,
                recovery,
                spc: 1 << boot[109],
                fat: dword(&boot, 80),
                heap: dword(&boot, 88),
                clusters: dword(&boot, 92),
                root: dword(&boot, 96),
            };
            let fat_length = dword(&boot, 84);
            if volume.fat < 24
                || fat_length == 0
                || volume.fat.checked_add(fat_length)? > volume.heap
                || !(1..=0xffff_fff5).contains(&volume.clusters)
                || u64::from(fat_length) * (sector as u64) < (u64::from(volume.clusters) + 2) * 4
                || u64::from(volume.heap) + u64::from(volume.spc) * u64::from(volume.clusters)
                    > volume.length
                || volume.length > available
                || !volume.cluster_valid(volume.root)
                || expected.is_some_and(|first| first != (volume, fat_length))
            {
                return None;
            }
            let mut checksum = boot
                .iter()
                .enumerate()
                .filter(|(i, _)| ![106, 107, 112].contains(i))
                .fold(0u32, |sum, (_, &byte)| {
                    sum.rotate_right(1).wrapping_add(u32::from(byte))
                });
            for offset in 1..11 {
                if !read(copy + offset, &mut boot) {
                    return None;
                }
                checksum = boot.iter().fold(checksum, |sum, &byte| {
                    sum.rotate_right(1).wrapping_add(u32::from(byte))
                });
            }
            if !read(copy + 11, &mut boot)
                || !boot
                    .chunks_exact(4)
                    .all(|bytes| bytes == checksum.to_le_bytes())
            {
                return None;
            }
            if expected.is_some() {
                return Some(volume);
            }
            expected = Some((volume, fat_length));
        }
        None
    }

    fn cluster_valid(self, id: u32) -> bool {
        (2..self.clusters + 2).contains(&id)
    }

    fn cluster_bytes(self) -> usize {
        self.spc as usize * self.sector
    }

    fn lba(self, id: u32) -> u64 {
        self.start + u64::from(self.heap) + u64::from(self.spc) * u64::from(id - 2)
    }

    fn next(
        self,
        id: u32,
        slot: &mut [u8],
        read: &mut impl FnMut(u64, &mut [u8]) -> bool,
    ) -> Option<u32> {
        if !self.cluster_valid(id) {
            return None;
        }
        let offset = u64::from(id) * 4;
        read(
            self.start + u64::from(self.fat) + offset / self.sector as u64,
            slot,
        )
        .then(|| dword(slot, (offset % self.sector as u64) as usize))
    }

    fn chain(
        self,
        first: u32,
        max: usize,
        read: &mut impl FnMut(u64, &mut [u8]) -> bool,
    ) -> Option<Vec<u32>> {
        let mut ids = Vec::new();
        let mut slot = alloc::vec![0; self.sector];
        let mut next = first;
        while self.cluster_valid(next) && ids.len() < max && !ids.contains(&next) {
            ids.push(next);
            next = self.next(next, &mut slot, read)?;
            if next >= EOC {
                return Some(ids);
            }
        }
        None
    }

    pub fn opted_in(self, read: &mut impl FnMut(u64, &mut [u8]) -> bool) -> bool {
        let Some(chain) = self.chain(self.root, ROOT_LIMIT / self.cluster_bytes(), read) else {
            return false;
        };
        let mut sector = alloc::vec![0; self.sector];
        for cluster in chain {
            for offset in 0..u64::from(self.spc) {
                if !read(self.lba(cluster) + offset, &mut sector) {
                    return false;
                }
                for row in sector.chunks_exact(32) {
                    if row[0] == 0 {
                        return false;
                    }
                    if row[0] == 0x83 {
                        return row[1] == 6
                            && row[2..14]
                                .chunks_exact(2)
                                .zip(b"KOSDAT")
                                .all(|(pair, letter)| {
                                    pair[1] == 0 && pair[0].eq_ignore_ascii_case(letter)
                                });
                    }
                }
            }
        }
        false
    }

    fn mapped(self, chain: &[u32], block: usize) -> u64 {
        self.lba(chain[block / self.spc as usize]) + (block % self.spc as usize) as u64
    }
}

#[derive(Clone)]
struct Entry {
    name: crate::text::Text<{ storage::DISK_NAME_CAP }>,
    offset: usize,
    cluster: u32,
    len: usize,
    contiguous: bool,
    managed: bool,
    directory: bool,
}

struct Parent {
    chain: Vec<u32>,
    offset: usize,
    contiguous: bool,
}

pub struct Volume {
    geometry: Geometry,
    root: Vec<u8>,
    root_chain: Vec<u32>,
    bitmap: Vec<u8>,
    bitmap_chain: Vec<u32>,
    metadata: Vec<u32>,
    tracked: Vec<crate::text::Text<{ storage::PATH_CAP }>>,
    folders: Vec<crate::text::Text<{ storage::PATH_CAP }>>,
    parent: Option<Parent>,
    healthy: bool,
}

fn word(b: &[u8], index: usize) -> u16 {
    u16::from_le_bytes(b[index..index + 2].try_into().unwrap())
}
fn dword(b: &[u8], index: usize) -> u32 {
    u32::from_le_bytes(b[index..index + 4].try_into().unwrap())
}
fn qword(b: &[u8], index: usize) -> u64 {
    u64::from_le_bytes(b[index..index + 8].try_into().unwrap())
}
fn sum(data: &[u8], skip: &[usize]) -> u16 {
    data.iter()
        .enumerate()
        .filter(|(i, _)| !skip.contains(i))
        .fold(0u16, |sum, (_, &b)| {
            sum.rotate_right(1).wrapping_add(u16::from(b))
        })
}
fn hash(name: &str) -> u16 {
    name.bytes()
        .flat_map(|byte| [byte.to_ascii_uppercase(), 0])
        .fold(0u16, |sum, b| {
            sum.rotate_right(1).wrapping_add(u16::from(b))
        })
}

pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= storage::DISK_NAME_CAP
        && !name.ends_with(['.', ' '])
        && name
            .bytes()
            .all(|b| (32..127).contains(&b) && !b"\"*/:<>?\\|".contains(&b))
}

pub fn draft_name(name: &str) -> bool {
    name.eq_ignore_ascii_case(DRAFT)
        || name
            .get(..DRAFT_PREFIX.len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(DRAFT_PREFIX))
}

impl Volume {
    pub fn needs_recovery(&self) -> bool {
        self.geometry.recovery || !self.healthy
    }

    pub fn label(&self) -> crate::text::Text<11> {
        let mut label = crate::text::Text::new();
        if let Some(row) = self
            .root
            .chunks_exact(32)
            .take_while(|row| row[0] != 0)
            .find(|row| row[0] == 0x83)
        {
            for code in row[2..].chunks_exact(2).take(usize::from(row[1].min(11))) {
                label.push(if code[1] == 0 && (32..127).contains(&code[0]) {
                    code[0]
                } else {
                    b'?'
                });
            }
        }
        label
    }

    pub fn mount(geometry: Geometry) -> Option<Self> {
        let mut read = |lba, data: &mut [u8]| storage::blocks(lba, data, false);
        let cluster_bytes = geometry.cluster_bytes();
        let root_chain = geometry.chain(geometry.root, ROOT_LIMIT / cluster_bytes, &mut read)?;
        let mut root = alloc::vec![0; root_chain.len() * cluster_bytes];
        for (&id, bytes) in root_chain.iter().zip(root.chunks_exact_mut(cluster_bytes)) {
            if !read(geometry.lba(id), bytes) {
                return None;
            }
        }
        let directory = root.chunks_exact(32).take_while(|row| row[0] != 0);
        let mut bitmap_id = None;
        let mut upcase_id = None;
        for row in directory {
            match row[0] {
                0x81 if bitmap_id.is_none() && row[1] == 0 => {
                    bitmap_id = Some((dword(row, 20), qword(row, 24)));
                }
                0x82 if upcase_id.is_none() => upcase_id = Some((dword(row, 20), qword(row, 24))),
                0x81 | 0x82 => return None,
                _ => {}
            }
        }
        let (bitmap_id, bitmap_length) = bitmap_id?;
        let required = (geometry.clusters as usize).div_ceil(8);
        if bitmap_length != required as u64 || required > BITMAP_LIMIT {
            return None;
        }
        let bitmap_clusters = required.div_ceil(cluster_bytes);
        let bitmap_chain = geometry.chain(bitmap_id, bitmap_clusters, &mut read)?;
        let (upcase_id, upcase_length) = upcase_id?;
        if !(2..=131072).contains(&upcase_length) || upcase_length % 2 != 0 {
            return None;
        }
        let upcase_clusters = (upcase_length as usize).div_ceil(cluster_bytes);
        let upcase_chain = geometry.chain(upcase_id, upcase_clusters, &mut read)?;
        if bitmap_chain.len() != bitmap_clusters || upcase_chain.len() != upcase_clusters {
            return None;
        }
        let mut metadata = root_chain.clone();
        metadata.extend(&bitmap_chain);
        metadata.extend(upcase_chain);
        metadata.sort_unstable();
        if metadata.windows(2).any(|pair| pair[0] == pair[1]) {
            return None;
        }
        let padded = required.div_ceil(geometry.sector) * geometry.sector;
        let mut bitmap = alloc::vec![0; padded];
        for (&id, data) in bitmap_chain.iter().zip(bitmap.chunks_mut(cluster_bytes)) {
            if !read(geometry.lba(id), data) {
                return None;
            }
        }
        let volume = Self {
            geometry,
            root,
            root_chain,
            bitmap,
            bitmap_chain,
            metadata,
            tracked: Vec::new(),
            folders: Vec::new(),
            parent: None,
            healthy: true,
        };
        if !volume.metadata.iter().all(|&id| volume.allocated(id)) {
            return None;
        }
        volume.entries()?;
        Some(volume)
    }

    fn allocated(&self, id: u32) -> bool {
        let index = (id - 2) as usize;
        self.bitmap[index / 8] & (1 << (index % 8)) != 0
    }

    fn file_chain(&self, id: u32, length: usize, contiguous: bool) -> Option<Vec<u32>> {
        let count = length.div_ceil(self.geometry.cluster_bytes());
        if count == 0 {
            return (id == 0).then(Vec::new);
        }
        if !self.geometry.cluster_valid(id) {
            return None;
        }
        let chain = if contiguous {
            (id..id.checked_add(count as u32)?).collect()
        } else {
            self.geometry.chain(id, count, &mut |lba, data| {
                storage::blocks(lba, data, false)
            })?
        };
        (chain.len() == count
            && chain.iter().all(|&id| {
                self.geometry.cluster_valid(id)
                    && self.allocated(id)
                    && self.metadata.binary_search(&id).is_err()
            }))
        .then_some(chain)
    }

    fn entries(&self) -> Option<Vec<Entry>> {
        let mut out: Vec<Entry> = Vec::new();
        let mut offset = 0;
        while let Some(entry) = self.root.get(offset..offset + 32) {
            if entry[0] == 0 {
                break;
            }
            if entry[0] < 128 || matches!(entry[0], 0x81..=0x83) {
                offset += 32;
                continue;
            }
            if entry[0] != 0x85 {
                if entry[0] & 0x60 == 0x20 {
                    offset += 32;
                    continue;
                }
                return None;
            }
            let count = usize::from(entry[1]) + 1;
            let end = offset + count * 32;
            let set = self.root.get(offset..end)?;
            if !(3..=19).contains(&count) || sum(set, &[2, 3]) != word(set, 2) || set[32] != 0xc0 {
                return None;
            }
            let name_len = usize::from(set[35]);
            let size = qword(set, 56);
            let cluster = dword(set, 52);
            let contiguous = set[33] & 2 != 0;
            if name_len == 0 || name_len.div_ceil(15) + 2 != count {
                return None;
            }
            let mut name = crate::text::Text::new();
            let mut scanned = 0;
            let mut ascii = name_len <= storage::DISK_NAME_CAP;
            for row in set[64..].chunks_exact(32) {
                if row[0] != 0xc1 {
                    return None;
                }
                for value in row[2..].chunks_exact(2).take(name_len - scanned) {
                    scanned += 1;
                    if value[1] != 0 || !(32..127).contains(&value[0]) {
                        ascii = false;
                    }
                    name.push(value[0]);
                }
            }
            if !ascii {
                offset = end;
                continue;
            }
            if !valid_name(name.str())
                || word(set, 36) != hash(name.str())
                || out.iter().any(|old| {
                    old.name.str().eq_ignore_ascii_case(name.str())
                        || (cluster != 0 && old.cluster == cluster)
                })
            {
                return None;
            }
            let max = if draft_name(name.str()) {
                storage::DRAFT_CAP
            } else {
                storage::CAP
            };
            let directory = word(entry, 4) & 0x10 != 0;
            let managed = word(entry, 4) & 0x11 == 0
                && size <= max as u64
                && qword(set, 40) == size
                && set[33] & !3 == 0
                && (size == 0 || set[33] & 1 != 0)
                && self
                    .file_chain(cluster, size as usize, contiguous)
                    .is_some();
            out.push(Entry {
                name,
                offset,
                cluster,
                len: size.min(if directory { 512 * 1024 } else { max } as u64 + 1) as usize,
                contiguous,
                managed,
                directory,
            });
            offset = end;
        }
        Some(out)
    }

    fn read_entry(&self, entry: &Entry, data: &mut [u8]) -> bool {
        let Some(chain) = self.file_chain(entry.cluster, entry.len, entry.contiguous) else {
            return false;
        };
        if entry.len == 0 {
            return true;
        }
        let cluster = self.geometry.cluster_bytes();
        let mut tail = alloc::vec![0; self.geometry.sector];
        for (&id, target) in chain.iter().zip(data[..entry.len].chunks_mut(cluster)) {
            let aligned = target.len() / self.geometry.sector * self.geometry.sector;
            let start = self.geometry.lba(id);
            if aligned != 0 && !storage::blocks(start, &mut target[..aligned], false) {
                return false;
            }
            if aligned < target.len() {
                if !storage::blocks(
                    start + (aligned / self.geometry.sector) as u64,
                    &mut tail,
                    false,
                ) {
                    return false;
                }
                let remainder = &mut target[aligned..];
                remainder.copy_from_slice(&tail[..remainder.len()]);
            }
        }
        true
    }

    fn in_child<R>(&mut self, entry: &Entry, operation: impl FnOnce(&mut Self) -> R) -> Option<R> {
        if !entry.directory
            || entry.len == 0
            || entry.len > 512 * 1024
            || entry.len % self.geometry.cluster_bytes() != 0
        {
            return None;
        }
        let chain = self.file_chain(entry.cluster, entry.len, entry.contiguous)?;
        if chain.iter().any(|id| self.root_chain.contains(id)) {
            return None;
        }
        let mut bytes = alloc::vec![0; entry.len];
        let size = self.geometry.cluster_bytes();
        for (&id, data) in chain.iter().zip(bytes.chunks_exact_mut(size)) {
            if !storage::blocks(self.geometry.lba(id), data, false) {
                return None;
            }
        }
        let mut outer = core::mem::replace(&mut self.root, bytes);
        let outer_chain = core::mem::replace(&mut self.root_chain, chain);
        let previous = self.parent.replace(Parent {
            chain: outer_chain,
            offset: entry.offset,
            contiguous: entry.contiguous,
        });
        let result = operation(self);
        if self.root.len() != entry.len {
            Self::resize_entry(&mut outer, entry.offset, self.root.len());
        }
        self.root = outer;
        self.root_chain = self.parent.take().unwrap().chain;
        self.parent = previous;
        Some(result)
    }

    fn resize_entry(directory: &mut [u8], offset: usize, len: usize) {
        let count = usize::from(directory[offset + 1]) + 1;
        let set = &mut directory[offset..offset + 32 * count];
        set[33] &= !2;
        set[40..48].copy_from_slice(&(len as u64).to_le_bytes());
        set[56..64].copy_from_slice(&(len as u64).to_le_bytes());
        let check = sum(set, &[2, 3]);
        set[2..4].copy_from_slice(&check.to_le_bytes());
    }

    fn in_directory<R>(&mut self, path: &str, operation: impl FnOnce(&mut Self) -> R) -> Option<R> {
        if path.matches('/').count() >= 8 {
            return None;
        }
        self.walk(path, &mut Some(operation))
    }

    fn walk<R>(
        &mut self,
        path: &str,
        operation: &mut Option<impl FnOnce(&mut Self) -> R>,
    ) -> Option<R> {
        if path.is_empty() {
            return Some(operation.take()?(self));
        }
        let (head, tail) = path.split_once('/').unwrap_or((path, ""));
        let entry = self
            .entries()?
            .into_iter()
            .find(|entry| entry.directory && entry.name.str().eq_ignore_ascii_case(head))?;
        self.in_child(&entry, |volume| volume.walk(tail, operation))?
    }

    pub fn accepts_file(&mut self, path: &str) -> bool {
        let (parent, name) = path.rsplit_once('/').unwrap_or(("", path));
        if parent.matches('/').count() >= 8 {
            return false;
        }
        self.in_directory(parent, |volume| {
            volume.entries().is_some_and(|entries| {
                !entries
                    .iter()
                    .any(|entry| entry.name.str().eq_ignore_ascii_case(name) && !entry.managed)
            })
        })
        .unwrap_or(false)
    }

    pub fn folder_paths(&self) -> impl Iterator<Item = &str> {
        self.folders.iter().map(|name| name.str())
    }

    pub fn mkdir(&mut self, path: &str) -> bool {
        let (parent, name) = path.rsplit_once('/').unwrap_or(("", path));
        if parent.matches('/').count() >= 8 || !valid_name(name) {
            return false;
        }
        if self.in_directory(parent, |volume| volume.update_entry(name, Some(&[]), true))
            != Some(true)
        {
            return false;
        }
        let mut label = crate::text::Text::new();
        label.set(path);
        self.folders.push(label);
        true
    }

    pub fn rmdir(&mut self, path: &str) -> bool {
        let (parent, name) = path.rsplit_once('/').unwrap_or(("", path));
        let removed = self.in_directory(parent, |volume| {
            let Some(entry) = volume.entries().and_then(|entries| {
                entries
                    .into_iter()
                    .find(|entry| entry.directory && entry.name.str().eq_ignore_ascii_case(name))
            }) else {
                return false;
            };
            if volume.in_child(&entry, |child| {
                child
                    .root
                    .chunks_exact(32)
                    .take_while(|row| row[0] != 0)
                    .all(|row| row[0] < 128)
            }) != Some(true)
            {
                return false;
            }
            volume.update_entry(name, None, true)
        });
        if removed == Some(true) {
            self.folders
                .retain(|folder| !folder.str().eq_ignore_ascii_case(path));
            true
        } else {
            false
        }
    }

    pub fn load(&mut self, files: &mut Vec<File>) -> bool {
        self.load_tree(files, "", 0, &mut 0).is_some()
    }

    fn load_tree(
        &mut self,
        files: &mut Vec<File>,
        prefix: &str,
        depth: u8,
        used: &mut usize,
    ) -> Option<()> {
        let mut entries = self.entries()?;
        entries.sort_by_key(|entry| !crate::users::is_database(entry.name.str()));
        for entry in &entries {
            let name = entry.name.str();
            if prefix.len() + name.len() + 1 > storage::PATH_CAP {
                continue;
            }
            let mut path = crate::text::Text::new();
            if !prefix.is_empty() {
                path.append(prefix);
                path.push(b'/');
            }
            path.append(name);
            if entry.directory && depth < 8 {
                if self.folders.len() < storage::COUNT {
                    self.folders.push(path);
                    let _ = self.in_child(entry, |volume| {
                        volume.load_tree(files, path.str(), depth + 1, used)
                    });
                }
            } else if entry.managed
                && !draft_name(name)
                && files.len() < storage::COUNT
                && *used + entry.len <= storage::CACHE_BYTES
            {
                let mut bytes = alloc::vec![0; entry.len];
                if !self.read_entry(entry, &mut bytes) {
                    return None;
                }
                *used += entry.len;
                self.tracked.push(path);
                files.push(File {
                    name: path,
                    data: storage::Data { bytes },
                    used: true,
                    dirty: false,
                });
            }
        }
        Some(())
    }

    pub fn read_draft(&self, name: &str) -> Option<Vec<u8>> {
        let entry = self
            .entries()?
            .into_iter()
            .find(|entry| entry.managed && entry.name.str().eq_ignore_ascii_case(name))?;
        let mut bytes = alloc::vec![0; entry.len];
        self.read_entry(&entry, &mut bytes).then_some(bytes)
    }

    pub fn has_draft(&self, name: &str) -> bool {
        self.entries().is_none_or(|entries| {
            entries
                .iter()
                .any(|entry| entry.name.str().eq_ignore_ascii_case(name))
        })
    }

    fn dirty(&self, dirty: bool) -> bool {
        let mut boot = alloc::vec![0; self.geometry.sector];
        for lba in [self.geometry.start, self.geometry.start + 12] {
            if !storage::blocks(lba, &mut boot, false) {
                return false;
            }
            boot[106] = (boot[106] & !2) | (u8::from(dirty) << 1);
            if !storage::blocks(lba, &mut boot, true) {
                return false;
            }
        }
        true
    }

    fn allocate(&self, len: usize) -> Option<Vec<u32>> {
        let mut run = Vec::new();
        for id in 2..self.geometry.clusters + 2 {
            if self.allocated(id) {
                run.clear();
            } else {
                run.push(id);
                if run.len() == len {
                    return Some(run);
                }
            }
        }
        None
    }

    fn set_allocation(&mut self, chain: &[u32], allocated: bool) -> bool {
        let sector = self.geometry.sector;
        let mut changed = alloc::collections::BTreeSet::new();
        for &id in chain {
            let bit = (id - 2) as usize;
            if allocated {
                self.bitmap[bit / 8] |= 1 << (bit % 8);
            } else {
                self.bitmap[bit / 8] &= !(1 << (bit % 8));
            }
            changed.insert(bit / (8 * sector));
        }
        changed.into_iter().all(|index| {
            storage::blocks(
                self.geometry.mapped(&self.bitmap_chain, index),
                &mut self.bitmap[index * sector..(index + 1) * sector],
                true,
            )
        })
    }

    fn put_fat(&self, id: u32, next: u32) -> bool {
        let sector = self.geometry.sector;
        let offset = u64::from(id) * 4;
        let lba = self.geometry.start + u64::from(self.geometry.fat) + offset / sector as u64;
        let mut block = alloc::vec![0; sector];
        if !storage::blocks(lba, &mut block, false) {
            return false;
        }
        let slot = (offset % sector as u64) as usize;
        block[slot..slot + 4].copy_from_slice(&next.to_le_bytes());
        storage::blocks(lba, &mut block, true)
    }

    fn grow_root(&mut self) -> bool {
        let len = self.geometry.cluster_bytes();
        let limit = if self.parent.is_some() {
            512 * 1024
        } else {
            ROOT_LIMIT
        };
        if self.root.len() + len > limit {
            return false;
        }
        let Some(ids) = self.allocate(1) else {
            return false;
        };
        let id = ids[0];
        if !self.zero_clusters(&ids)
            || !self.set_allocation(&ids, true)
            || !self.put_fat(id, u32::MAX)
        {
            return false;
        }
        if self.parent.as_ref().is_some_and(|parent| parent.contiguous)
            && !self
                .root_chain
                .windows(2)
                .all(|pair| self.put_fat(pair[0], pair[1]))
        {
            return false;
        }
        if !self.put_fat(*self.root_chain.last().unwrap(), id) {
            return false;
        }
        if let Some(parent) = &self.parent {
            let sector = self.geometry.sector;
            let first = parent.offset / sector;
            let offset = parent.offset % sector;
            let mut buffer = alloc::vec![0; sector * 2];
            if !storage::blocks(
                self.geometry.mapped(&parent.chain, first),
                &mut buffer[..sector],
                false,
            ) {
                return false;
            }
            let count = usize::from(buffer[offset + 1]) + 1;
            let crosses = offset + count * 32 > sector;
            if crosses
                && !storage::blocks(
                    self.geometry.mapped(&parent.chain, first + 1),
                    &mut buffer[sector..],
                    false,
                )
            {
                return false;
            }
            Self::resize_entry(&mut buffer, offset, self.root.len() + len);
            if (crosses
                && !storage::blocks(
                    self.geometry.mapped(&parent.chain, first + 1),
                    &mut buffer[sector..],
                    true,
                ))
                || !storage::blocks(
                    self.geometry.mapped(&parent.chain, first),
                    &mut buffer[..sector],
                    true,
                )
            {
                return false;
            }
        } else {
            self.metadata.push(id);
            self.metadata.sort_unstable();
        }
        self.root.resize(self.root.len() + len, 0);
        self.root_chain.push(id);
        true
    }

    pub(crate) fn update(&mut self, path: &str, data: Option<&[u8]>) -> bool {
        let (parent, name) = path.rsplit_once('/').unwrap_or(("", path));
        if self.in_directory(parent, |volume| volume.update_entry(name, data, false)) != Some(true)
        {
            return false;
        }
        if !draft_name(path) {
            self.tracked
                .retain(|entry| !entry.str().eq_ignore_ascii_case(path));
            if data.is_some() {
                let mut label = crate::text::Text::new();
                label.set(path);
                self.tracked.push(label);
            }
        }
        true
    }

    fn update_entry(&mut self, name: &str, data: Option<&[u8]>, directory: bool) -> bool {
        if !self.healthy || !valid_name(name) {
            return false;
        }
        let Some(entries) = self.entries() else {
            return false;
        };
        let old = entries
            .iter()
            .find(|entry| entry.name.str().eq_ignore_ascii_case(name));
        if old.is_some_and(|entry| {
            if directory {
                data.is_some() || !entry.directory
            } else {
                !entry.managed
            }
        }) {
            return false;
        }
        if let Some((entry, value)) = old.zip(data) {
            let mut prior = alloc::vec![0; entry.len];
            if entry.len == value.len()
                && self.read_entry(entry, &mut prior)
                && &prior[..entry.len] == value
                && entry.name.str() == name
            {
                return true;
            }
        }
        if data.is_none() && old.is_none() {
            return true;
        }
        let maximum = if draft_name(name) {
            storage::DRAFT_CAP
        } else {
            storage::CAP
        };
        if data.is_some_and(|value| value.len() > maximum) {
            return false;
        }
        let old_chain = old.map_or_else(
            || Some(Vec::new()),
            |entry| self.file_chain(entry.cluster, entry.len, entry.contiguous),
        );
        let Some(old_chain) = old_chain else {
            return false;
        };
        let count = 2 + name.len().div_ceil(15);
        let bytes = count * 32;
        let end = self
            .root
            .chunks_exact(32)
            .position(|row| row[0] == 0)
            .map_or(self.root.len(), |index| index * 32);
        let target = old.map_or_else(
            || {
                (0..end)
                    .step_by(32)
                    .find(|&offset| {
                        offset + bytes <= end
                            && self.root[offset..offset + bytes]
                                .chunks_exact(32)
                                .all(|row| row[0] < 128)
                    })
                    .unwrap_or(end)
            },
            |entry| entry.offset,
        );
        if target + bytes > ROOT_LIMIT {
            return false;
        }
        if !self.dirty(true) {
            self.healthy = false;
            return false;
        }
        self.healthy = false;
        if (target + bytes > self.root.len()) && !self.grow_root() {
            return false;
        }
        let mut changed_end = target + bytes;
        if let Some(value) = data {
            let size = if directory {
                self.geometry.cluster_bytes()
            } else {
                value.len()
            };
            let (cluster, contiguous) = if size == 0 {
                (0, false)
            } else {
                let cluster_bytes = self.geometry.cluster_bytes();
                let count = size.div_ceil(cluster_bytes);
                let (chain, contiguous) = if let Some(chain) = self.allocate(count) {
                    (chain, !directory)
                } else {
                    let scattered: Vec<_> = (2..self.geometry.clusters + 2)
                        .filter(|&id| !self.allocated(id))
                        .take(count)
                        .collect();
                    (scattered, false)
                };
                if chain.len() != count {
                    self.healthy = self.dirty(false);
                    return false;
                }
                if !contiguous
                    && !chain.iter().enumerate().all(|(index, &id)| {
                        self.put_fat(id, chain.get(index + 1).copied().unwrap_or(u32::MAX))
                    })
                {
                    return false;
                }
                if directory && !self.zero_clusters(&chain) {
                    return false;
                }
                let sector = self.geometry.sector;
                let mut buffer = Vec::new();
                for (&id, chunk) in chain.iter().zip(value.chunks(cluster_bytes)) {
                    buffer.clear();
                    buffer.extend_from_slice(chunk);
                    buffer.resize(chunk.len().div_ceil(sector) * sector, 0);
                    if !storage::blocks(self.geometry.lba(id), &mut buffer, true) {
                        return false;
                    }
                }
                if !self.set_allocation(&chain, true) {
                    return false;
                }
                (chain[0], contiguous)
            };
            let set = &mut self.root[target..target + bytes];
            set.fill(0);
            set[0] = 0x85;
            set[1] = count as u8 - 1;
            set[4] = if directory {
                0x10
            } else if draft_name(name) {
                2
            } else {
                0x20
            };
            for field in [8, 12, 16] {
                set[field + 2] = 0x21;
            }
            set[32..36].copy_from_slice(&[
                0xc0,
                if contiguous { 3 } else { 1 },
                0,
                name.len() as u8,
            ]);
            set[36..38].copy_from_slice(&hash(name).to_le_bytes());
            set[40..48].copy_from_slice(&(size as u64).to_le_bytes());
            set[52..56].copy_from_slice(&cluster.to_le_bytes());
            set[56..64].copy_from_slice(&(size as u64).to_le_bytes());
            for (row, part) in set[64..]
                .chunks_exact_mut(32)
                .zip(name.as_bytes().chunks(15))
            {
                row[0] = 0xc1;
                for (i, &byte) in part.iter().enumerate() {
                    row[2 + i * 2] = byte;
                }
            }
            let checksum = sum(set, &[2, 3]);
            set[2..4].copy_from_slice(&checksum.to_le_bytes());
            if target == end && changed_end < self.root.len() {
                self.root[changed_end] = 0;
                changed_end += 1;
            }
        } else {
            for row in self.root[target..target + bytes].chunks_exact_mut(32) {
                row[0] &= 0x7f;
            }
        }
        // Write secondary entries before the sector containing the primary entry.
        let sector = self.geometry.sector;
        for index in (target / sector..changed_end.div_ceil(sector)).rev() {
            if !storage::blocks(
                self.geometry.mapped(&self.root_chain, index),
                &mut self.root[index * sector..(index + 1) * sector],
                true,
            ) {
                return false;
            }
        }
        // Publish the new directory entry before destroying the old contents.
        // Keep the old clusters allocated if any zeroing write fails.
        if !self.zero_clusters(&old_chain) || !self.set_allocation(&old_chain, false) {
            return false;
        }
        self.healthy = self.dirty(false);
        self.healthy
    }

    pub fn save(&mut self, files: &mut [File]) -> bool {
        for old in self.tracked.clone() {
            if !files
                .iter()
                .any(|file| file.used && file.name.str().eq_ignore_ascii_case(old.str()))
                && !self.update(old.str(), None)
            {
                return false;
            }
        }
        files
            .iter_mut()
            .filter(|file| file.used && file.dirty)
            .all(|file| {
                if self.update(file.name.str(), Some(&file.data.bytes)) {
                    file.dirty = false;
                    true
                } else {
                    false
                }
            })
    }

    fn zero_clusters(&self, chain: &[u32]) -> bool {
        if chain.is_empty() {
            return true;
        }
        let sectors = self.geometry.spc as usize;
        let chunk = sectors.min(16);
        let mut zeros = alloc::vec![0; chunk * self.geometry.sector];
        chain.iter().all(|&cluster| {
            (0..sectors).step_by(chunk).all(|offset| {
                let bytes = (sectors - offset).min(chunk) * self.geometry.sector;
                storage::blocks(
                    self.geometry.lba(cluster) + offset as u64,
                    &mut zeros[..bytes],
                    true,
                )
            })
        })
    }

    pub(crate) fn scrub_free(&self, cursor: &mut u32, budget: u8) -> Result<bool, &'static str> {
        if !self.healthy {
            return Err("The data volume needs a filesystem check");
        }
        let end = (self.geometry.clusters + 2).min(cursor.saturating_add(32));
        let mut wiped = 0;
        while *cursor < end && wiped < budget.clamp(1, 8) {
            if !self.allocated(*cursor) {
                if !self.zero_clusters(&[*cursor]) {
                    return Err("A free-cluster write failed");
                }
                wiped += 1;
            }
            *cursor += 1;
        }
        Ok(*cursor == self.geometry.clusters + 2)
    }
}
