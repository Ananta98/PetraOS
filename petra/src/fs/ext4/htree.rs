// SPDX-License-Identifier: GPL-2.0

//! Ext4 HTree (Hash-indexed Tree) directory lookup implementation.
//!
//! Replaces $O(N)$ linear directory scanning with $O(\log N)$ tree lookups for
//! directories with `EXT4_INDEX_FL` (`0x00001000`). Supports legacy, Half-MD4,
//! TEA, and SipHash-2-4 directory hashing algorithms compliant with Linux ext4.

use alloc::vec;

use crate::{
    fs::ext4::{dir, fs::Ext4Fs, inode::RawInode},
    uapi::errno::Result,
};

/// Ext4 HTree hash versions (`dx_hash_info.hash_version`).
pub const DX_HASH_LEGACY: u8 = 0;
pub const DX_HASH_HALF_MD4: u8 = 1;
pub const DX_HASH_TEA: u8 = 2;
pub const DX_HASH_LEGACY_UNSIGNED: u8 = 3;
pub const DX_HASH_HALF_MD4_UNSIGNED: u8 = 4;
pub const DX_HASH_TEA_UNSIGNED: u8 = 5;
pub const DX_HASH_SIPHASH: u8 = 6;

// ---------------------------------------------------------------------------
// Hash computation helpers
// ---------------------------------------------------------------------------

#[inline(always)]
fn f(x: u32, y: u32, z: u32) -> u32 {
    (x & y) | (!x & z)
}

#[inline(always)]
fn g(x: u32, y: u32, z: u32) -> u32 {
    (x & y) | (x & z) | (y & z)
}

#[inline(always)]
fn h(x: u32, y: u32, z: u32) -> u32 {
    x ^ y ^ z
}

#[inline(always)]
fn round1(a: &mut u32, b: u32, c: u32, d: u32, k: u32, s: u32) {
    *a = a.wrapping_add(f(b, c, d)).wrapping_add(k);
    *a = a.rotate_left(s);
}

#[inline(always)]
fn round2(a: &mut u32, b: u32, c: u32, d: u32, k: u32, s: u32) {
    *a = a.wrapping_add(g(b, c, d)).wrapping_add(k).wrapping_add(0x5a827999);
    *a = a.rotate_left(s);
}

#[inline(always)]
fn round3(a: &mut u32, b: u32, c: u32, d: u32, k: u32, s: u32) {
    *a = a.wrapping_add(h(b, c, d)).wrapping_add(k).wrapping_add(0x6ed9eba1);
    *a = a.rotate_left(s);
}

/// 3-round Half-MD4 transform producing 4-word output state from 8-word input.
pub fn half_md4_transform(buf: &mut [u32; 4], input: &[u32; 8]) {
    let mut a = buf[0];
    let mut b = buf[1];
    let mut c = buf[2];
    let mut d = buf[3];

    // Round 1
    round1(&mut a, b, c, d, input[0], 3);
    round1(&mut d, a, b, c, input[1], 7);
    round1(&mut c, d, a, b, input[2], 11);
    round1(&mut b, c, d, a, input[3], 19);
    round1(&mut a, b, c, d, input[4], 3);
    round1(&mut d, a, b, c, input[5], 7);
    round1(&mut c, d, a, b, input[6], 11);
    round1(&mut b, c, d, a, input[7], 19);

    // Round 2
    round2(&mut a, b, c, d, input[1], 3);
    round2(&mut d, a, b, c, input[3], 5);
    round2(&mut c, d, a, b, input[5], 9);
    round2(&mut b, c, d, a, input[7], 13);
    round2(&mut a, b, c, d, input[0], 3);
    round2(&mut d, a, b, c, input[2], 5);
    round2(&mut c, d, a, b, input[4], 9);
    round2(&mut b, c, d, a, input[6], 13);

    // Round 3
    round3(&mut a, b, c, d, input[3], 3);
    round3(&mut d, a, b, c, input[7], 9);
    round3(&mut c, d, a, b, input[2], 11);
    round3(&mut b, c, d, a, input[6], 15);
    round3(&mut a, b, c, d, input[1], 3);
    round3(&mut d, a, b, c, input[5], 9);
    round3(&mut c, d, a, b, input[0], 11);
    round3(&mut b, c, d, a, input[4], 15);

    buf[0] = buf[0].wrapping_add(a);
    buf[1] = buf[1].wrapping_add(b);
    buf[2] = buf[2].wrapping_add(c);
    buf[3] = buf[3].wrapping_add(d);
}

/// Packs byte string into 32-bit words with signed-char sign extension (Linux ext4 standard).
pub fn str2hashbuf_signed(msg: &[u8], buf: &mut [u32], num: usize) {
    let len = msg.len();
    let mut pad = (len as u32) | ((len as u32) << 8);
    pad |= pad << 16;
    let mut val = pad;
    let limit = len.min(num * 4);

    let mut buf_idx = 0;
    let mut remaining_num = num;

    for i in 0..limit {
        let sc = msg[i] as i8 as i32 as u32;
        val = sc.wrapping_add(val << 8);
        if (i % 4) == 3 {
            if buf_idx < buf.len() {
                buf[buf_idx] = val;
                buf_idx += 1;
            }
            val = pad;
            remaining_num = remaining_num.saturating_sub(1);
        }
    }
    if remaining_num > 0 {
        if buf_idx < buf.len() {
            buf[buf_idx] = val;
            buf_idx += 1;
        }
        remaining_num = remaining_num.saturating_sub(1);
    }
    while remaining_num > 0 && buf_idx < buf.len() {
        buf[buf_idx] = pad;
        buf_idx += 1;
        remaining_num = remaining_num.saturating_sub(1);
    }
}

/// Packs byte string into 32-bit words with unsigned-char byte packing.
pub fn str2hashbuf_unsigned(msg: &[u8], buf: &mut [u32], num: usize) {
    let len = msg.len();
    let mut pad = (len as u32) | ((len as u32) << 8);
    pad |= pad << 16;
    let mut val = pad;
    let limit = len.min(num * 4);

    let mut buf_idx = 0;
    let mut remaining_num = num;

    for i in 0..limit {
        let uc = msg[i] as u32;
        val = uc.wrapping_add(val << 8);
        if (i % 4) == 3 {
            if buf_idx < buf.len() {
                buf[buf_idx] = val;
                buf_idx += 1;
            }
            val = pad;
            remaining_num = remaining_num.saturating_sub(1);
        }
    }
    if remaining_num > 0 {
        if buf_idx < buf.len() {
            buf[buf_idx] = val;
            buf_idx += 1;
        }
        remaining_num = remaining_num.saturating_sub(1);
    }
    while remaining_num > 0 && buf_idx < buf.len() {
        buf[buf_idx] = pad;
        buf_idx += 1;
        remaining_num = remaining_num.saturating_sub(1);
    }
}

/// 16-round TEA (Tiny Encryption Algorithm) transform.
pub fn tea_transform(buf: &mut [u32; 4], input: &[u32; 4]) {
    let mut sum: u32 = 0;
    let mut b0 = buf[0];
    let mut b1 = buf[1];
    let a = input[0];
    let b = input[1];
    let c = input[2];
    let d = input[3];

    for _ in 0..16 {
        sum = sum.wrapping_add(0x9E3779B9);
        b0 = b0.wrapping_add(
            ((b1 << 4).wrapping_add(a)) ^ (b1.wrapping_add(sum)) ^ ((b1 >> 5).wrapping_add(b)),
        );
        b1 = b1.wrapping_add(
            ((b0 << 4).wrapping_add(c)) ^ (b0.wrapping_add(sum)) ^ ((b0 >> 5).wrapping_add(d)),
        );
    }

    buf[0] = buf[0].wrapping_add(b0);
    buf[1] = buf[1].wrapping_add(b1);
}

/// Legacy ext2 polynomial directory hash (signed).
fn dx_hack_hash_signed(name: &[u8]) -> (u32, u32) {
    let mut hash0: u32 = 0x12a3fe2d;
    let mut hash1: u32 = 0x37abe8f9;
    for &b in name {
        let sc = b as i8 as i32;
        let mut hash = hash1.wrapping_add(hash0 ^ (sc.wrapping_mul(7152373) as u32));
        if hash & 0x80000000 != 0 {
            hash = hash.wrapping_sub(0x7fffffff);
        }
        hash1 = hash0;
        hash0 = hash;
    }
    (hash0 << 1, hash1 << 1)
}

/// Legacy ext2 polynomial directory hash (unsigned).
fn dx_hack_hash_unsigned(name: &[u8]) -> (u32, u32) {
    let mut hash0: u32 = 0x12a3fe2d;
    let mut hash1: u32 = 0x37abe8f9;
    for &b in name {
        let uc = b as u32;
        let mut hash = hash1.wrapping_add(hash0 ^ (uc.wrapping_mul(7152373)));
        if hash & 0x80000000 != 0 {
            hash = hash.wrapping_sub(0x7fffffff);
        }
        hash1 = hash0;
        hash0 = hash;
    }
    (hash0 << 1, hash1 << 1)
}

/// Modern SipHash-2-4 directory hash.
fn dx_siphash(name: &[u8], seed: &[u32; 4]) -> (u32, u32) {
    let k0 = (seed[0] as u64) | ((seed[1] as u64) << 32);
    let k1 = (seed[2] as u64) | ((seed[3] as u64) << 32);
    let h = crate::crypto::SipHash24::hash(k0, k1, name);
    ((h as u32) & !1, (h >> 32) as u32)
}

/// Computes the primary and minor HTree directory hash for `name`.
///
/// Bit 0 of the primary hash is cleared (`hash & !1`) per ext4 specification
/// because bit 0 is reserved as a collision flag in directory index entries.
pub fn dx_hash(version: u8, name: &[u8], seed: &[u32; 4]) -> (u32, u32) {
    let (h, minor) = match version {
        DX_HASH_LEGACY => dx_hack_hash_signed(name),
        DX_HASH_LEGACY_UNSIGNED => dx_hack_hash_unsigned(name),
        DX_HASH_HALF_MD4 => {
            let mut buf: [u32; 4] = if seed.iter().any(|&s| s != 0) {
                *seed
            } else {
                [0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476]
            };
            let mut pos = 0;
            let mut rem = name.len();
            while rem > 0 {
                let mut input = [0u32; 8];
                str2hashbuf_signed(&name[pos..], &mut input, 8);
                half_md4_transform(&mut buf, &input);
                if rem > 32 {
                    rem -= 32;
                    pos += 32;
                } else {
                    break;
                }
            }
            (buf[1], buf[2])
        }
        DX_HASH_HALF_MD4_UNSIGNED => {
            let mut buf: [u32; 4] = if seed.iter().any(|&s| s != 0) {
                *seed
            } else {
                [0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476]
            };
            let mut pos = 0;
            let mut rem = name.len();
            while rem > 0 {
                let mut input = [0u32; 8];
                str2hashbuf_unsigned(&name[pos..], &mut input, 8);
                half_md4_transform(&mut buf, &input);
                if rem > 32 {
                    rem -= 32;
                    pos += 32;
                } else {
                    break;
                }
            }
            (buf[1], buf[2])
        }
        DX_HASH_TEA => {
            let mut buf: [u32; 4] = if seed.iter().any(|&s| s != 0) {
                *seed
            } else {
                [0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476]
            };
            let mut pos = 0;
            let mut rem = name.len();
            while rem > 0 {
                let mut input = [0u32; 4];
                str2hashbuf_signed(&name[pos..], &mut input, 4);
                tea_transform(&mut buf, &input);
                if rem > 16 {
                    rem -= 16;
                    pos += 16;
                } else {
                    break;
                }
            }
            (buf[0], buf[1])
        }
        DX_HASH_TEA_UNSIGNED => {
            let mut buf: [u32; 4] = if seed.iter().any(|&s| s != 0) {
                *seed
            } else {
                [0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476]
            };
            let mut pos = 0;
            let mut rem = name.len();
            while rem > 0 {
                let mut input = [0u32; 4];
                str2hashbuf_unsigned(&name[pos..], &mut input, 4);
                tea_transform(&mut buf, &input);
                if rem > 16 {
                    rem -= 16;
                    pos += 16;
                } else {
                    break;
                }
            }
            (buf[0], buf[1])
        }
        DX_HASH_SIPHASH => dx_siphash(name, seed),
        _ => dx_hack_hash_unsigned(name),
    };
    (h & !1, minor)
}

// ---------------------------------------------------------------------------
// On-disk HTree Index Structures
// ---------------------------------------------------------------------------

/// Reads the 32-bit hash value from a `dx_entry` array slice at `idx`.
#[inline]
pub fn read_entry_hash(entries_data: &[u8], idx: usize) -> u32 {
    if idx == 0 {
        0
    } else {
        let off = idx * 8;
        u32::from_le_bytes(entries_data[off..off + 4].try_into().unwrap_or([0; 4]))
    }
}

/// Reads the 32-bit block number from a `dx_entry` array slice at `idx`.
#[inline]
pub fn read_entry_block(entries_data: &[u8], idx: usize) -> u32 {
    let off = idx * 8 + 4;
    u32::from_le_bytes(entries_data[off..off + 4].try_into().unwrap_or([0; 4]))
}

/// Binary searches a `dx_entry` list for the largest entry whose hash <= `search_hash`.
///
/// Returns `(target_block, entry_index)`.
pub fn binary_search_entries(entries_data: &[u8], count: u16, search_hash: u32) -> (u32, usize) {
    if count <= 1 {
        return (read_entry_block(entries_data, 0), 0);
    }

    let mut left = 0usize;
    let mut right = (count as usize) - 1;
    let mut found = 0usize;

    while left <= right {
        let mid = left + (right - left) / 2;
        let entry_hash = read_entry_hash(entries_data, mid);
        if entry_hash <= search_hash {
            found = mid;
            left = mid + 1;
        } else {
            if mid == 0 {
                break;
            }
            right = mid - 1;
        }
    }

    (read_entry_block(entries_data, found), found)
}

/// Result of an HTree directory lookup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HtreeResult {
    /// Entry was found in the indexed directory: `(inode_number, file_type)`.
    Found(u32, u8),
    /// Entry definitely does not exist in the indexed directory.
    NotFound,
    /// HTree structure is invalid, unsupported, or corrupted; caller should fall back to linear scan.
    Fallback,
}

/// Performs an HTree index lookup for `name` inside `dir_ino`.
///
/// Traverses root and intermediate index blocks to locate the candidate leaf block,
/// then searches the leaf block directly. If hash collisions span into the next block,
/// the adjacent leaf block is also checked.
pub fn htree_lookup(
    fs: &Ext4Fs,
    dir_ino: u32,
    raw: &RawInode,
    name: &str,
) -> Result<HtreeResult> {
    let bs = fs.block_size;

    // Fast-path for "." and "..": stored in block 0
    if name == "." {
        let mut root_buf = vec![0u8; bs];
        fs.read_inode_block(dir_ino, raw, 0, &mut root_buf)?;
        if root_buf.len() >= 12 && root_buf[6] == 1 && &root_buf[8..9] == b"." {
            let ino = u32::from_le_bytes(root_buf[0..4].try_into().unwrap_or([0; 4]));
            let ftype = root_buf[7];
            return Ok(HtreeResult::Found(ino, ftype));
        }
        return Ok(HtreeResult::Fallback);
    }
    if name == ".." {
        let mut root_buf = vec![0u8; bs];
        fs.read_inode_block(dir_ino, raw, 0, &mut root_buf)?;
        if root_buf.len() >= 24 && root_buf[18] == 2 && &root_buf[20..22] == b".." {
            let ino = u32::from_le_bytes(root_buf[12..16].try_into().unwrap_or([0; 4]));
            let ftype = root_buf[19];
            return Ok(HtreeResult::Found(ino, ftype));
        }
        return Ok(HtreeResult::Fallback);
    }

    // Read logical block 0 (dx_root)
    let mut root_buf = vec![0u8; bs];
    fs.read_inode_block(dir_ino, raw, 0, &mut root_buf)?;

    // Validate root header
    if root_buf.len() < 40 {
        return Ok(HtreeResult::Fallback);
    }
    // Verify dot and dotdot presence
    if root_buf[6] != 1 || &root_buf[8..9] != b"." || root_buf[18] != 2 || &root_buf[20..22] != b".." {
        return Ok(HtreeResult::Fallback);
    }

    // Parse dx_root_info at offset 24..32
    let reserved_zero = u32::from_le_bytes(root_buf[24..28].try_into().unwrap_or([0; 4]));
    let hash_version = root_buf[28];
    let info_length = root_buf[29];
    let indirect_levels = root_buf[30];

    if reserved_zero != 0 || info_length != 8 || indirect_levels > 2 {
        return Ok(HtreeResult::Fallback);
    }
    if hash_version > DX_HASH_SIPHASH {
        return Ok(HtreeResult::Fallback);
    }

    // Parse dx_countlimit in first entry (offset 32..36)
    let limit = u16::from_le_bytes(root_buf[32..34].try_into().unwrap_or([0; 2]));
    let count = u16::from_le_bytes(root_buf[34..36].try_into().unwrap_or([0; 2]));

    if count == 0 || count > limit || (count as usize) * 8 > bs - 32 {
        return Ok(HtreeResult::Fallback);
    }

    let seed = fs.sb.lock().hash_seed();
    let (hash, _) = dx_hash(hash_version, name.as_bytes(), &seed);
    let search_hash = hash & !1;

    // Search root index entries starting at offset 32
    let (mut target_block, mut entry_idx) =
        binary_search_entries(&root_buf[32..], count, search_hash);

    // Descend through indirect index levels (if any)
    for _ in 0..indirect_levels {
        let mut node_buf = vec![0u8; bs];
        fs.read_inode_block(dir_ino, raw, target_block as u64, &mut node_buf)?;

        // Verify fake dirent at 0..8
        let rec_len = u16::from_le_bytes(node_buf[4..6].try_into().unwrap_or([0; 2])) as usize;
        if rec_len != bs {
            return Ok(HtreeResult::Fallback);
        }

        let node_limit = u16::from_le_bytes(node_buf[8..10].try_into().unwrap_or([0; 2]));
        let node_count = u16::from_le_bytes(node_buf[10..12].try_into().unwrap_or([0; 2]));

        if node_count == 0 || node_count > node_limit || (node_count as usize) * 8 > bs - 8 {
            return Ok(HtreeResult::Fallback);
        }

        let (next_block, next_idx) =
            binary_search_entries(&node_buf[8..], node_count, search_hash);
        target_block = next_block;
        entry_idx = next_idx;
    }

    // Read target leaf block
    let mut leaf_buf = vec![0u8; bs];
    fs.read_inode_block(dir_ino, raw, target_block as u64, &mut leaf_buf)?;

    if let Some(res) = dir::find_in_block(&leaf_buf, name) {
        return Ok(HtreeResult::Found(res.0, res.1));
    }

    // Collision check: if the next entry in the index has a matching hash, check it too
    if indirect_levels > 0 {
        return Ok(HtreeResult::Fallback);
    }
    if entry_idx + 1 < count as usize {
        let next_hash = read_entry_hash(&root_buf[32..], entry_idx + 1);
        if (next_hash & !1) == search_hash {
            let next_block = read_entry_block(&root_buf[32..], entry_idx + 1);
            fs.read_inode_block(dir_ino, raw, next_block as u64, &mut leaf_buf)?;
            if let Some(res) = dir::find_in_block(&leaf_buf, name) {
                return Ok(HtreeResult::Found(res.0, res.1));
            }
        }
    }

    Ok(HtreeResult::NotFound)
}
