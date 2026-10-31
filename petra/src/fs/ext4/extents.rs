// SPDX-License-Identifier: GPL-2.0

//! Ext4 extent tree support.
//!
//! The extent tree rooted in an inode's block area is flattened into a sorted
//! list of [`Extent`] segments for lookups, and rebuilt bottom-up with
//! [`TreePlan`] whenever the structure changes (holes in the middle, creating
//! or truncating extents).

use ::alloc::vec;
use ::alloc::vec::Vec;

use crate::crypto::hash::crc32c::crc32c;
use crate::uapi::errno::{Errno, Result};

/// Maximum logical length of a single extent (`0x7FFF` blocks; the value
/// `0x8000` is reserved on-disk for uninitialized extents, never created here).
pub const MAX_EXTENT_LEN: u32 = 0x7FFF;

/// The extent-tree on-disk magic (`ext4_extent_header.eh_magic`).
const EH_MAGIC: u16 = 0xF30A;

/// One sorted logical/physical mapping segment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Extent {
    /// First logical (file) block.
    pub log: u32,
    /// Number of logical blocks covered (1..=MAX_EXTENT_LEN).
    pub len: u32,
    /// First physical (FS) block.
    pub phys: u64,
}

impl Extent {
    /// Last logical block covered (inclusive).
    pub fn last(&self) -> u32 {
        self.log + self.len - 1
    }
}

/// Parameters needed to parse and rebuild a tree on disk.
///
/// `root_capacity` follows the kernel formula `(inode_size - 64) / 12` for
/// 256-byte inodes (16), or 4 for the legacy 60-byte inline area.
pub struct ExtentCtx<'a> {
    pub read_block: &'a dyn Fn(u64, &mut [u8]) -> Result<()>,
    pub block_size: usize,
    pub has_64bit: bool,
    pub ino: u32,
    pub inode_gen: u32,
    pub csum_on: bool,
    pub csum_seed: u32,
    pub root_capacity: usize,
}

/// Entry capacity of one disk block (header 12 bytes + optional 4-byte tail).
pub fn block_capacity(block_size: usize, csum_on: bool) -> usize {
    (block_size - 12 - if csum_on { 4 } else { 0 }) / 12
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

/// Decodes an extent (depth-0) entry: block, len, start_hi, start_lo.
fn parse_extent_entry(b: &[u8], has_64bit: bool) -> Extent {
    let log = u32::from_le_bytes(b[0..4].try_into().expect("4 bytes"));
    let mut len = u16::from_le_bytes(b[4..6].try_into().expect("2 bytes")) as u32;
    if len > 32768 {
        len -= 32768;
    }
    let start_hi = if has_64bit {
        u16::from_le_bytes(b[6..8].try_into().expect("2 bytes")) as u64
    } else {
        0
    };
    let start_lo = u32::from_le_bytes(b[8..12].try_into().expect("4 bytes")) as u64;
    Extent {
        log,
        len,
        phys: start_lo | (start_hi << 32),
    }
}

/// Decodes an index entry: block, leaf_lo, leaf_hi, pad.
fn parse_index_entry(b: &[u8], has_64bit: bool) -> (u32, u64) {
    let log = u32::from_le_bytes(b[0..4].try_into().unwrap_or_default());
    let leaf_lo = u32::from_le_bytes(b[4..8].try_into().unwrap_or_default()) as u64;
    let leaf_hi = if has_64bit {
        u16::from_le_bytes(b[8..10].try_into().unwrap_or_default()) as u64
    } else {
        0
    };
    (log, leaf_lo | (leaf_hi << 32))
}

/// Verifies an extent header and returns `(entry_count, depth)`.
fn parse_header(block: &[u8], max_entries: usize) -> Result<(usize, u16)> {
    if block.len() < 12 {
        return Err(Errno::EIO);
    }
    if u16::from_le_bytes([block[0], block[1]]) != EH_MAGIC {
        return Err(Errno::EIO);
    }
    let n = u16::from_le_bytes([block[2], block[3]]) as usize;
    let max = u16::from_le_bytes([block[4], block[5]]) as usize;
    let depth = u16::from_le_bytes([block[6], block[7]]);
    if n > max_entries || n > max {
        return Err(Errno::EIO);
    }
    Ok((n, depth))
}

/// Flattens the extent tree rooted in an inode's inline area.
pub fn scan(raw_inode: &[u8], ctx: &ExtentCtx<'_>) -> Result<Vec<Extent>> {
    if raw_inode.len() < 100 {
        return Err(Errno::EIO);
    }
    let root_area = &raw_inode[40..100];
    let (n, depth) = parse_header(root_area, ctx.root_capacity)?;
    let mut entries: Vec<[u8; 12]> = Vec::with_capacity(n);
    for i in 0..n {
        let off = 12 + i * 12;
        entries.push(root_area[off..off + 12].try_into().unwrap_or_default());
    }

    if depth == 0 {
        return Ok(entries.iter().map(|e| parse_extent_entry(e, ctx.has_64bit)).collect());
    }

    // Depth >= 1: walk the index levels top-down, collecting leaves.
    let capacity = block_capacity(ctx.block_size, ctx.csum_on);
    let mut level: u16 = depth;
    let mut current: Vec<(u32, u64)> = entries.iter().map(|e| parse_index_entry(e, ctx.has_64bit)).collect();
    let mut extents = Vec::new();

    loop {
        level -= 1;
        let mut child_list: Vec<(u32, u64)> = Vec::new();
        for (_log, ptr) in &current {
            let mut blk = vec![0u8; ctx.block_size];
            (ctx.read_block)(*ptr, &mut blk)?;
            let (count, d) = parse_header(&blk, capacity)?;
            if d != level {
                return Err(Errno::EIO);
            }
            for i in 0..count {
                let e = &blk[12 + i * 12..12 + (i + 1) * 12];
                if level == 0 {
                    extents.push(parse_extent_entry(e, ctx.has_64bit));
                } else {
                    child_list.push(parse_index_entry(e, ctx.has_64bit));
                }
            }
        }
        if level == 0 {
            break;
        }
        current = child_list;
    }

    extents.sort_by_key(|e| e.log);
    Ok(extents)
}

// ---------------------------------------------------------------------------
// Lookup / mutation of the in-memory (flat) mapping
// ---------------------------------------------------------------------------

/// Finds the extent covering logical block `lblock`, as `(phys, len)`.
pub fn lookup(extents: &[Extent], lblock: u32) -> Option<(u64, u32)> {
    let idx = extents.partition_point(|e| e.log <= lblock);
    if idx == 0 {
        return None;
    }
    let e = &extents[idx - 1];
    if lblock < e.log + e.len {
        Some((e.phys + (lblock - e.log) as u64, e.len - (lblock - e.log)))
    } else {
        None
    }
}

/// Physical block of `lblock`, or 0 for a hole.
pub fn map(extents: &[Extent], lblock: u32) -> u64 {
    lookup(extents, lblock).map_or(0, |(p, _)| p)
}

/// Frees the physical ranges of extents covering logical blocks >= `from`,
/// adjusting an extent that straddles the boundary.
pub fn truncate(extents: &mut Vec<Extent>, from: u32) -> Vec<(u64, u32)> {
    let mut freed = Vec::new();
    let mut k = 0;
    while k < extents.len() {
        let e = extents[k];
        if e.log >= from {
            freed.push((e.phys, e.len));
            extents.remove(k);
        } else if e.log + e.len > from {
            let keep = from - e.log;
            freed.push((e.phys + keep as u64, e.len - keep));
            extents[k].len = keep;
            k += 1;
        } else {
            k += 1;
        }
    }
    freed
}

/// Inserts `e` into the sorted flat mapping, merging with adjacent segments.
///
/// Callers only insert into regions currently unmapped (holes or past EOF),
/// so no overlap handling is required.
pub fn insert(extents: &mut Vec<Extent>, e: Extent) {
    let idx = extents.partition_point(|x| x.log < e.log);
    extents.insert(idx, e);
    merge_neighbors(extents, idx);
}

/// Merges segments around `idx` when log/phys contiguous and len allows.
fn merge_neighbors(extents: &mut Vec<Extent>, idx: usize) {
    // Forward merges.
    while idx + 1 < extents.len() {
        let a = extents[idx];
        let b = extents[idx + 1];
        if b.log == a.log + a.len
            && a.phys + a.len as u64 == b.phys
            && a.len + b.len <= MAX_EXTENT_LEN
        {
            extents[idx] = Extent {
                log: a.log,
                len: a.len + b.len,
                phys: a.phys,
            };
            extents.remove(idx + 1);
        } else {
            break;
        }
    }
    // Backward merge.
    if idx > 0 {
        let a = extents[idx - 1];
        let b = extents[idx];
        if b.log == a.log + a.len
            && a.phys + a.len as u64 == b.phys
            && a.len + b.len <= MAX_EXTENT_LEN
        {
            extents[idx - 1] = Extent {
                log: a.log,
                len: a.len + b.len,
                phys: a.phys,
            };
            extents.remove(idx);
        }
    }
}

// ---------------------------------------------------------------------------
// Rebuilding (serialization) of the on-disk tree
// ---------------------------------------------------------------------------

/// A planned tree shape: block count per level, leaves first.
pub struct TreePlan {
    /// Block count per level; the root lives inline (not counted).
    pub levels: Vec<usize>,
}

impl TreePlan {
    /// Plans the tree shape for `n` entries.
    pub fn plan(n: usize, block_size: usize, csum_on: bool, root_capacity: usize) -> Self {
        let cap = block_capacity(block_size, csum_on);
        if n == 0 || n <= root_capacity {
            return Self { levels: Vec::new() };
        }

        let mut levels = Vec::new();
        let mut upper = n.div_ceil(cap);
        levels.push(upper);
        while upper > root_capacity {
            upper = upper.div_ceil(cap);
            levels.push(upper);
        }
        // The last push counts the top-level blocks; everything above them
        // lives inline in the root. We keep one more level than the number of
        // *index* levels so the build loop treats the final chunking of
        // `current` as root entries — see `build_tree`.
        Self { levels }
    }

    /// Total number of disk blocks this tree requires.
    pub fn total_blocks(&self) -> usize {
        self.levels.iter().sum()
    }
}

/// Serializes one non-root tree block, including its trailing checksum.
fn build_leaf_block(
    extents: &[Extent],
    block_size: usize,
    csum_on: bool,
    seed: u32,
    ino: u32,
    inode_gen: u32,
) -> Vec<u8> {
    let cap = block_capacity(block_size, csum_on);
    let mut block = vec![0u8; block_size];
    block[0..2].copy_from_slice(&EH_MAGIC.to_le_bytes());
    let n = extents.len().min(cap) as u16;
    block[2..4].copy_from_slice(&n.to_le_bytes());
    block[4..6].copy_from_slice(&(cap as u16).to_le_bytes());
    block[6..8].copy_from_slice(&0u16.to_le_bytes());
    block[8..12].copy_from_slice(&0u32.to_le_bytes());
    for (i, e) in extents.iter().enumerate() {
        block[12 + i * 12..12 + (i + 1) * 12].copy_from_slice(&pack_extent_entry(e));
    }
    if csum_on {
        let mut state = seed;
        state = crc32c(state, &ino.to_le_bytes());
        state = crc32c(state, &inode_gen.to_le_bytes());
        state = crc32c(state, &block[0..block_size - 4]);
        block[block_size - 4..block_size].copy_from_slice(&state.to_le_bytes());
    }
    block
}

fn build_index_block(
    entries: &[(u32, u64)],
    depth: u16,
    block_size: usize,
    csum_on: bool,
    seed: u32,
    ino: u32,
    inode_gen: u32,
) -> Vec<u8> {
    let cap = block_capacity(block_size, csum_on);
    let mut block = vec![0u8; block_size];
    block[0..2].copy_from_slice(&EH_MAGIC.to_le_bytes());
    let n = entries.len().min(cap) as u16;
    block[2..4].copy_from_slice(&n.to_le_bytes());
    block[4..6].copy_from_slice(&(cap as u16).to_le_bytes());
    block[6..8].copy_from_slice(&depth.to_le_bytes());
    block[8..12].copy_from_slice(&0u32.to_le_bytes());
    for (i, (log, ptr)) in entries.iter().enumerate() {
        block[12 + i * 12..12 + (i + 1) * 12].copy_from_slice(&pack_index_entry(*log, *ptr));
    }
    if csum_on {
        let mut state = seed;
        state = crc32c(state, &ino.to_le_bytes());
        state = crc32c(state, &inode_gen.to_le_bytes());
        state = crc32c(state, &block[0..block_size - 4]);
        block[block_size - 4..block_size].copy_from_slice(&state.to_le_bytes());
    }
    block
}

/// Packs an extent entry (depth 0, `ext4_extent`).
fn pack_extent_entry(e: &Extent) -> [u8; 12] {
    let mut out = [0u8; 12];
    out[0..4].copy_from_slice(&e.log.to_le_bytes());
    out[4..6].copy_from_slice(&(e.len as u16).to_le_bytes());
    out[6..8].copy_from_slice(&((e.phys >> 32) as u16).to_le_bytes());
    out[8..12].copy_from_slice(&(e.phys as u32).to_le_bytes());
    out
}

/// Packs an index pointer entry (depth > 0, `ext4_extent_idx`).
fn pack_index_entry(log: u32, ptr: u64) -> [u8; 12] {
    let mut out = [0u8; 12];
    out[0..4].copy_from_slice(&log.to_le_bytes());
    out[4..8].copy_from_slice(&(ptr as u32).to_le_bytes());
    out[8..10].copy_from_slice(&((ptr >> 32) as u16).to_le_bytes());
    out
}

/// Result of a tree rebuild: writes plus the root's inline `eh` area.
pub struct BuiltTree {
    /// Physical block -> bytes to write, in the order blocks were allocated.
    pub writes: Vec<(u64, Vec<u8>)>,
    /// Inline extent header bytes for the inode (60-byte image).
    pub root: [u8; 60],
    pub depth: u16,
}

/// Builds the whole tree from a flat extent list and preallocated blocks.
///
/// `allocated` must contain exactly `plan.total_blocks()` physical block
/// numbers, assigned level by level, each level in order.
pub fn build_tree(
    extents: &[Extent],
    plan: &TreePlan,
    allocated: &[u64],
    ctx: &ExtentCtx<'_>,
) -> Result<BuiltTree> {
    if plan.levels.is_empty() {
        // Everything fits inline in the root.
        let mut root = [0u8; 60];
        let n = extents.len();
        root[0..2].copy_from_slice(&EH_MAGIC.to_le_bytes());
        root[2..4].copy_from_slice(&(n as u16).to_le_bytes());
        root[4..6].copy_from_slice(&(ctx.root_capacity as u16).to_le_bytes());
        root[6..8].copy_from_slice(&0u16.to_le_bytes());
        root[8..12].copy_from_slice(&0u32.to_le_bytes());
        for (i, e) in extents.iter().enumerate() {
            // Inline root entries are always extents (depth 0).
            root[12 + i * 12..12 + (i + 1) * 12].copy_from_slice(&pack_extent_entry(e));
        }
        return Ok(BuiltTree {
            writes: Vec::new(),
            root,
            depth: 0,
        });
    }

    let cap = block_capacity(ctx.block_size, ctx.csum_on);
    let mut writes: Vec<(u64, Vec<u8>)> = Vec::new();
    let mut cursor = 0usize;

    // Current level's child-pointer list `(first_log, child_phys)`.
    // Level 0 chunks the flat extent list into leaf blocks.
    let mut current: Vec<(u32, u64)> = Vec::new();
    for i in 0..plan.levels[0] {
        let phys = *allocated.get(cursor).ok_or(Errno::EINVAL)?;
        cursor += 1;
        let start = i * cap;
        let end = ((i + 1) * cap).min(extents.len());
        let chunk = &extents[start..end];
        let image = build_leaf_block(
            chunk,
            ctx.block_size,
            ctx.csum_on,
            ctx.csum_seed,
            ctx.ino,
            ctx.inode_gen,
        );
        writes.push((phys, image));
        current.push((chunk.first().map_or(0, |e| e.log), phys));
    }

    // Index levels: level `li` blocks hold child pointers from level `li-1`.
    // Each block at level `li` stores `eh_depth == li`. After the last level,
    // `current` (<= root_capacity entries) is embedded in the root.
    for li in 1..plan.levels.len() {
        let count = plan.levels[li];
        let mut next: Vec<(u32, u64)> = Vec::new();
        for i in 0..count {
            let phys = *allocated.get(cursor).ok_or(Errno::EINVAL)?;
            cursor += 1;
            let start = i * cap;
            let end = ((i + 1) * cap).min(current.len());
            let entries = &current[start..end];
            let image = build_index_block(
                entries,
                li as u16,
                ctx.block_size,
                ctx.csum_on,
                ctx.csum_seed,
                ctx.ino,
                ctx.inode_gen,
            );
            writes.push((phys, image));
            next.push((entries.first().map_or(0, |(l, _)| *l), phys));
        }
        current = next;
    }

    // Pack the root inline area with `current` as index entries. The root
    // depth equals the number of block levels below it.
    let root_depth = plan.levels.len() as u16;
    let mut root = [0u8; 60];
    root[0..2].copy_from_slice(&EH_MAGIC.to_le_bytes());
    root[2..4].copy_from_slice(&(current.len() as u16).to_le_bytes());
    root[4..6].copy_from_slice(&(ctx.root_capacity as u16).to_le_bytes());
    root[6..8].copy_from_slice(&root_depth.to_le_bytes());
    root[8..12].copy_from_slice(&0u32.to_le_bytes());
    for (i, (log, ptr)) in current.iter().enumerate() {
        root[12 + i * 12..12 + (i + 1) * 12].copy_from_slice(&pack_index_entry(*log, *ptr));
    }

    Ok(BuiltTree {
        writes,
        root,
        depth: root_depth,
    })
}
