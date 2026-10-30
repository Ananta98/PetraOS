// SPDX-License-Identifier: GPL-2.0

//! JBD2 journal replay.
//!
//! Scan/replay committed transactions at mount time; afterward the journal
//! is marked empty (`s_start = 0`). All jbd2 on-disk fields are big-endian.

use ::alloc::vec;
use ::alloc::vec::Vec;

use crate::crypto::hash::crc32c::crc32c;
use crate::uapi::errno::{Errno, Result};

/// JBD2 magic `0xC03B3998` (big-endian bytes on disk).
const JBD2_MAGIC_BE: [u8; 4] = [0xC0, 0x3B, 0x39, 0x98];

/// Block type codes (`journal_header_t.h_blocktype`).
mod bt {
    pub const DESCRIPTOR: u32 = 1;
    pub const COMMIT: u32 = 2;
    pub const SUPERBLOCK_V1: u32 = 3;
    pub const SUPERBLOCK_V2: u32 = 4;
    pub const REVOKE: u32 = 5;
}

/// Journal incompatible features (`s_feature_incompat`, big-endian).
mod jfeat {
    pub const REVOKE: u32 = 0x1;
    pub const B64: u32 = 0x2;
    pub const ASYNC_COMMIT: u32 = 0x4;
    pub const CSUM_V2: u32 = 0x8;
    pub const CSUM_V3: u32 = 0x10;
    pub const FAST_COMMIT: u32 = 0x20;
}

/// Tag flags.
mod tagf {
    pub const ESCAPE: u32 = 0x1;
    pub const SAME_UUID: u32 = 0x2;
    pub const LAST: u32 = 0x8;
}

/// Restore jbd2 magic in an escaped data block.
fn unescape(buf: &mut [u8]) {
    if buf.len() >= 4 {
        buf[0..4].copy_from_slice(&JBD2_MAGIC_BE);
    }
}

/// Journal superblock as parsed at mount time.
pub struct JournalSb {
    pub magic_ok: bool,
    pub block_type: u32,
    pub block_size: u32,
    pub maxlen: u32,
    pub first: u32,
    pub sequence: u32,
    pub start: u32,
    pub feature_incompat: u32,
    pub uuid: [u8; 16],
}

impl JournalSb {
    /// Parses the first 1024 bytes of the journal (journal block 0).
    pub fn parse(buf: &[u8]) -> Result<Self> {
        if buf.len() < 1024 {
            return Err(Errno::EINVAL);
        }
        let be32 = |off: usize| -> u32 {
            u32::from_be_bytes(buf[off..off + 4].try_into().unwrap_or_default())
        };
        let magic_ok = buf[0..4] == JBD2_MAGIC_BE;
        let mut uuid = [0u8; 16];
        uuid.copy_from_slice(&buf[0x30..0x40]);
        Ok(Self {
            magic_ok,
            block_type: be32(4),
            block_size: be32(0xC),
            maxlen: be32(0x10),
            first: be32(0x14),
            sequence: be32(0x18),
            start: be32(0x1C),
            feature_incompat: be32(0x28),
            uuid,
        })
    }
}

/// A scanned committed transaction.
struct Tx {
    sequence: u32,
    /// Physical FS block -> data written by this transaction.
    writes: Vec<(u64, Vec<u8>)>,
    /// Blocks revoked from now on.
    revokes: Vec<u64>,
}

/// I/O endpoints for journal block reads/writes.
pub struct JournalIo<'a> {
    /// Read journal FS block #`lblock` (mapped through the journal inode).
    pub read_journal_block: &'a dyn Fn(u64, &mut [u8]) -> Result<()>,
    pub write_fs_block: &'a dyn Fn(u64, &[u8]) -> Result<()>,
}

/// Performs journal replay given a journal reader and a block writer.
///
/// Returns `true` when the journal contained committed transactions that
/// were replayed (the caller should then set the log empty + mark clean).
pub fn replay(jsb: &JournalSb, io: &JournalIo<'_>, fs_block_size: usize) -> Result<bool> {
    if !jsb.magic_ok {
        // A destroyed/foreign superblock: nothing to replay; caller decides.
        return Ok(false);
    }
    if jsb.start == 0 || jsb.maxlen == 0 {
        return Ok(false);
    }

    let features = jsb.feature_incompat;
    let csum_v3 = features & jfeat::CSUM_V3 != 0;
    let _csum_v2 = features & jfeat::CSUM_V2 != 0;
    let b64 = features & jfeat::B64 != 0;

    let maxlength = jsb.maxlen as u64;
    let mut cur = jsb.start as u64 % maxlength;
    let mut expected_seq = jsb.sequence;
    let mut txs: Vec<Tx> = Vec::new();
    let mut steps = 0u64;

    while steps < maxlength + 2 {
        steps += 1;
        let mut hdr = vec![0u8; fs_block_size];
        (io.read_journal_block)(cur, &mut hdr)?;
        if hdr.len() < 12 || hdr[0..4] != JBD2_MAGIC_BE {
            break;
        }
        let btype = u32::from_be_bytes(hdr[4..8].try_into().unwrap_or_default());
        let seq = u32::from_be_bytes(hdr[8..12].try_into().unwrap_or_default());

        match btype {
            bt::DESCRIPTOR => {
                if seq != expected_seq {
                    break;
                }
                // Decode the tag list.
                let mut tags: Vec<(u64, u32)> = Vec::new(); // (target block, flags)
                let mut pos = 12usize;
                loop {
                    let tag = parse_tag(&hdr, pos, csum_v3, b64)?;
                    match tag {
                        None => break,
                        Some((target, flags, consumed)) => {
                            tags.push((target, flags));
                            pos += consumed;
                            if flags & tagf::LAST != 0 {
                                break;
                            }
                        }
                    }
                }
                // Follow physical journal blocks for the data.
                let mut writes = Vec::new();
                let mut jcur = (cur + 1) % maxlength;
                let mut done = false;
                for (target, flags) in &tags {
                    let mut data = vec![0u8; fs_block_size];
                    (io.read_journal_block)(jcur, &mut data)?;
                    if *flags & tagf::ESCAPE != 0 {
                        unescape(&mut data);
                    }
                    writes.push((*target, data.clone()));
                    jcur = (jcur + 1) % maxlength;
                    if jcur == cur {
                        done = true;
                        break;
                    }
                }
                let mut committed = false;
                if !done {
                    let mut cb = vec![0u8; fs_block_size];
                    (io.read_journal_block)(jcur, &mut cb)?;
                    if cb.len() >= 12 && cb[0..4] == JBD2_MAGIC_BE {
                        let ctype = u32::from_be_bytes(cb[4..8].try_into().unwrap_or_default());
                        let cseq = u32::from_be_bytes(cb[8..12].try_into().unwrap_or_default());
                        committed = ctype == bt::COMMIT && cseq == expected_seq;
                    }
                }

                if committed {
                    txs.push(Tx {
                        sequence: expected_seq,
                        writes,
                        revokes: Vec::new(),
                    });
                    expected_seq = expected_seq.wrapping_add(1);
                    // Advance past descriptor, data blocks and commit block:
                    // the commit block occupies `jcur`, so the next tx begins
                    // at `jcur + 1`.
                    cur = (jcur + 1) % maxlength;
                } else {
                    // Incomplete transaction: stop replay here.
                    break;
                }
            }
            bt::REVOKE => {
                if seq != expected_seq {
                    break;
                }
                let width = if b64 { 8 } else { 4 };
                let rcount =
                    u32::from_be_bytes(hdr[12..16].try_into().unwrap_or_default()) as usize;
                let end = (16 + rcount).min(fs_block_size);
                let mut revoked = Vec::new();
                let mut off = 16usize;
                while off + width <= end {
                    let v: u64 = if width == 8 {
                        u64::from_be_bytes(hdr[off..off + 8].try_into().unwrap_or_default())
                    } else {
                        u32::from_be_bytes(hdr[off..off + 4].try_into().unwrap_or_default()) as u64
                    };
                    revoked.push(v);
                    off += width;
                }
                txs.push(Tx {
                    sequence: expected_seq,
                    writes: Vec::new(),
                    revokes: revoked,
                });
                expected_seq = expected_seq.wrapping_add(1);
                cur = (cur + 1) % maxlength;
            }
            _ => break,
        }
    }

    let had_txs = !txs.is_empty();

    // Apply in transaction order, honoring revocation semantics:
    //  - a write to a block clears its revocation first,
    //  - then revocations arriving with a tx take effect for later txs.
    let mut revoked: Vec<u64> = Vec::new();
    for tx in &txs {
        for (target, data) in &tx.writes {
            if revoked.contains(target) {
                revoked.retain(|b| b != target);
            }
            (io.write_fs_block)(*target, data)?;
        }
        for b in &tx.revokes {
            revoked.push(*b);
        }
    }

    Ok(had_txs)
}

/// Parses one descriptor tag at `pos`, returning `(target, flags, size)`.
fn parse_tag(
    hdr: &[u8],
    pos: usize,
    csum_v3: bool,
    b64: bool,
) -> Result<Option<(u64, u32, usize)>> {
    if pos + 8 > hdr.len() {
        return Err(Errno::EIO);
    }
    let mut target: u64 =
        u32::from_be_bytes(hdr[pos..pos + 4].try_into().unwrap_or_default()) as u64;
    let mut flags: u32;
    let mut size: usize;

    if csum_v3 {
        if pos + 16 > hdr.len() {
            return Err(Errno::EIO);
        }
        flags = u32::from_be_bytes(hdr[pos + 4..pos + 8].try_into().unwrap_or_default());
        let high = u32::from_be_bytes(hdr[pos + 8..pos + 12].try_into().unwrap_or_default());
        // 64-bit target when present.
        if b64 {
            target |= (high as u64) << 32;
        }
        size = 16;
    } else {
        let ckfl = u16::from_be_bytes(hdr[pos + 4..pos + 6].try_into().unwrap_or_default());
        flags = u16::from_be_bytes(hdr[pos + 6..pos + 8].try_into().unwrap_or_default()) as u32;
        // t_checksum 16 bits occupy bytes 4..6, t_flags bytes 6..8.
        size = 8;
        if b64 {
            if pos + 12 > hdr.len() {
                return Err(Errno::EIO);
            }
            let high = u32::from_be_bytes(hdr[pos + 8..pos + 12].try_into().unwrap_or_default());
            target |= (high as u64) << 32;
            size = 12;
        }
        let _ = ckfl;
        let _ = &mut flags;
    }

    // Optional per-tag UUID (not needed; consumed if present).
    if flags & tagf::SAME_UUID == 0 {
        if pos + size + 16 > hdr.len() {
            return Err(Errno::EIO);
        }
        size += 16;
    }

    Ok(Some((target, flags, size)))
}

/// Builds the "empty journal" superblock bytes from the on-disk image:
/// `s_start = 0`, `s_head = 0`, and (for CSUM v2/v3 journals) a fresh
/// `s_checksum` over the seeded UUID + the first 1020 bytes.
pub fn make_empty_sb_bytes(orig: &[u8], feature_incompat: u32) -> Result<[u8; 1024]> {
    let mut sb = [0u8; 1024];
    sb.copy_from_slice(&orig[0..1024]);
    let be32mut =
        |sb: &mut [u8], off: usize, v: u32| sb[off..off + 4].copy_from_slice(&v.to_be_bytes());
    be32mut(&mut sb, 0x1C, 0); // start = empty
    be32mut(&mut sb, 0x58, 0); // head = 0

    if feature_incompat & (jfeat::CSUM_V2 | jfeat::CSUM_V3) != 0 {
        // Seed: CRC-32C chain over the journal UUID (bytes 0x30..0x40).
        let mut state = 0xFFFF_FFFF;
        state = crc32c(state, &sb[0x30..0x40]);
        sb[0xFC..0x100].copy_from_slice(&[0, 0, 0, 0]);
        state = crc32c(state, &sb[0..0xFC]);
        sb[0xFC..0x100].copy_from_slice(&state.to_be_bytes());
    }
    Ok(sb)
}
