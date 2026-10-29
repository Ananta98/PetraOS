// SPDX-License-Identifier: GPL-2.0

//! Legacy ext2/3-style indirect block mapping.
//!
//! Files without the extent flag use the classic 15-pointer scheme
//! (12 direct + single + double + triple indirect). This module implements
//! read mapping plus allocate-on-write for every depth.

use ::alloc::vec;
use ::alloc::vec::Vec;

use crate::fs::ext4::inode::RawInode;
use crate::uapi::errno::{Errno, Result};

/// Block I/O context for indirect mapping.
pub struct IndirectIo<'a> {
    pub read_block: &'a dyn Fn(u64, &mut [u8]) -> Result<()>,
    pub write_block: &'a dyn Fn(u64, &[u8]) -> Result<()>,
    /// Allocates one new FS block; bitmap bookkeeping is the caller's job.
    pub alloc_block: &'a mut dyn FnMut() -> Result<u64>,
    pub block_size: usize,
}

impl IndirectIo<'_> {
    fn pointers_per_block(&self) -> u64 {
        (self.block_size / 4) as u64
    }

    fn load(&self, phys: u64) -> Result<Vec<u8>> {
        let mut buf = vec![0u8; self.block_size];
        (self.read_block)(phys, &mut buf)?;
        Ok(buf)
    }

    fn ptr_at(&self, buf: &[u8], idx: u64) -> u64 {
        let off = (idx as usize * 4).min(buf.len().saturating_sub(4));
        u32::from_le_bytes(buf[off..off + 4].try_into().unwrap_or_default()) as u64
    }

    fn set_ptr(buf: &mut [u8], idx: u64, val: u64) {
        let off = (idx as usize * 4).min(buf.len().saturating_sub(4));
        buf[off..off + 4].copy_from_slice(&(val as u32).to_le_bytes());
    }
}

/// Read-side mapping without allocation. Returns the physical block or 0
/// for a hole.
pub fn map_read(ino: &RawInode, lblock: u32, io: &IndirectIo<'_>) -> Result<u64> {
    let ppb = io.pointers_per_block();
    if lblock < 12 {
        return Ok(ino.block[lblock as usize] as u64);
    }
    let mut rel = lblock as u64 - 12;
    if rel < ppb {
        if ino.block[12] == 0 {
            return Ok(0);
        }
        let buf = io.load(ino.block[12] as u64)?;
        return Ok(io.ptr_at(&buf, rel));
    }
    rel -= ppb;
    if rel < ppb * ppb {
        if ino.block[13] == 0 {
            return Ok(0);
        }
        let outer = io.load(ino.block[13] as u64)?;
        let mid = io.ptr_at(&outer, rel / ppb);
        if mid == 0 {
            return Ok(0);
        }
        let inner = io.load(mid)?;
        return Ok(io.ptr_at(&inner, rel % ppb));
    }
    rel -= ppb * ppb;
    if rel < ppb * ppb * ppb {
        if ino.block[14] == 0 {
            return Ok(0);
        }
        let top = io.load(ino.block[14] as u64)?;
        let mid = io.ptr_at(&top, rel / (ppb * ppb));
        if mid == 0 {
            return Ok(0);
        }
        let inner = io.load(mid)?;
        let leaf = io.ptr_at(&inner, (rel % (ppb * ppb)) / ppb);
        if leaf == 0 {
            return Ok(0);
        }
        let leafbuf = io.load(leaf)?;
        return Ok(io.ptr_at(&leafbuf, rel % ppb));
    }
    Err(Errno::EOVERFLOW)
}

/// Ensures the pointer at `idx` inside pointer block `parent` is non-zero,
/// allocating a fresh block (and writing back the patched pointer block)
/// when needed. Returns the pointer value.
fn ensure_bptr(io: &mut IndirectIo<'_>, parent: u64, idx: u64) -> Result<u64> {
    let buf = io.load(parent)?;
    let cur = io.ptr_at(&buf, idx);
    if cur != 0 {
        return Ok(cur);
    }
    let fresh = (io.alloc_block)()?;
    let mut store = buf.clone();
    IndirectIo::set_ptr(&mut store, idx, fresh);
    (io.write_block)(parent, &store)?;
    Ok(fresh)
}

/// Write-side mapping: allocates missing chain blocks and the target block.
///
/// The caller persists the (mutated) inode afterwards.
pub fn map_write(ino: &mut RawInode, lblock: u32, io: &mut IndirectIo<'_>) -> Result<u64> {
    let ppb = io.pointers_per_block();

    if lblock < 12 {
        if ino.block[lblock as usize] == 0 {
            ino.block[lblock as usize] = (io.alloc_block)()? as u32;
        }
        return Ok(ino.block[lblock as usize] as u64);
    }

    let mut rel = lblock as u64 - 12;

    // Single indirect: inode block[12] -> leaf chains.
    if rel < ppb {
        if ino.block[12] == 0 {
            ino.block[12] = (io.alloc_block)()? as u32;
        }
        return ensure_bptr(io, ino.block[12] as u64, rel);
    }
    rel -= ppb;

    // Double indirect: inode block[13] -> outer, outer -> leaf.
    if rel < ppb * ppb {
        if ino.block[13] == 0 {
            ino.block[13] = (io.alloc_block)()? as u32;
        }
        let mid = ensure_bptr(io, ino.block[13] as u64, rel / ppb)?;
        return ensure_bptr(io, mid, rel % ppb);
    }
    rel -= ppb * ppb;

    // Triple indirect: inode block[14] -> top -> mid -> target.
    if rel < ppb * ppb * ppb {
        if ino.block[14] == 0 {
            ino.block[14] = (io.alloc_block)()? as u32;
        }
        let mid = ensure_bptr(io, ino.block[14] as u64, rel / (ppb * ppb))?;
        let leaf = ensure_bptr(io, mid, (rel % (ppb * ppb)) / ppb)?;
        return ensure_bptr(io, leaf, rel % ppb);
    }

    Err(Errno::EOVERFLOW)
}
