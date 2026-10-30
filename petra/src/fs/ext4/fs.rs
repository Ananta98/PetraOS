// SPDX-License-Identifier: GPL-2.0

//! Ext4 filesystem instance, VFS integration, and high-level operations.
//!
//! Serialization: one coarse `ops_lock` serializes each logical operation;
//! the superblock and block-group table carry their own mutexes for the
//! allocator. Inodes are re-read from disk for every operation (no cache).

use ::alloc::boxed::Box;
use ::alloc::string::String;
use ::alloc::sync::Arc;
use ::alloc::sync::Weak;
use ::alloc::vec;
use ::alloc::vec::Vec;
use spin::Mutex;

use crate::uapi::errno::{Errno, Result};
use crate::drivers::block::BlockDevice;
use crate::fs::ext4::dir::ftype;
use crate::fs::vfs::{
    FileOps, FileSystem, INode, INodeAttr, Mode, NodeOps, OpenFlags, SeekAnchor, StatFs,
};
use crate::crypto::hash::crc32c::crc32c;

use super::alloc::AllocIo;
use super::blockgroup::{BgTable, DescSum};
use super::dir::{self, DIRTAIL_TYPE};
use super::extents::{self, Extent, ExtentCtx, MAX_EXTENT_LEN, TreePlan};
use super::indirect::{self, IndirectIo};
use super::inode::{ROOT_INO, RawInode, iflags, nowsecs32, recompute_inode_csum};
use super::journal::{self, JournalIo, JournalSb};
use super::superblock::{EXT4_VALID_FS, SuperBlock, rocompat};

// ---------------------------------------------------------------------------
// Filesystem instance
// ---------------------------------------------------------------------------

/// The ext4 filesystem mount instance.
pub struct Ext4Fs {
    pub dev: Arc<dyn BlockDevice>,
    /// Superblock state; `raw` is re-serialized on sync.
    pub sb: Mutex<SuperBlock>,
    pub bgt: Mutex<BgTable>,
    pub block_size: usize,
    pub dev_block_size: usize,
    /// Identity tag for same-filesystem checks in link/rename.
    pub fsid: u64,
    /// Coarse per-filesystem operation lock.
    pub ops_lock: Mutex<()>,
    /// Weak self-reference so nodes can recover the owning `Arc<Ext4Fs>`.
    self_ref: Mutex<Weak<Ext4Fs>>,
}

impl Ext4Fs {
    pub fn new(dev: Arc<dyn BlockDevice>) -> Result<Arc<Self>> {
        if dev.block_size() == 0 || dev.capacity() < 2048 {
            return Err(Errno::EINVAL);
        }

        // Probe at least bytes 1024..2048 for the superblock; block-size
        // specifics are resolved by the byte-accurate reader below.
        let probe = dev.block_size().max(2048).max(4096);
        let mut prolog = vec![0u8; probe];
        dev_read_bytes(&*dev, 0, &mut prolog)?;
        let sb = SuperBlock::parse(&prolog[1024..2048])?;
        let block_size = sb.block_size();

        let groups = sb.block_groups_count().max(1) as usize;
        let csum_mode = if sb.has_metadata_csum() {
            DescSum::Crc32c
        } else if sb.feature_ro_compat & rocompat::GDT_CSUM != 0 {
            DescSum::Crc16
        } else {
            DescSum::None
        };
        let first_desc_block: u64 = if block_size == 1024 { 2 } else { 1 };
        let dev_block_size = dev.block_size();
        let probe_read = |blk: u64, buf: &mut [u8]| -> Result<()> {
            read_fs_blocks(&*dev, block_size, dev_block_size, blk, buf)
        };
        let mut bgt = BgTable::load(
            first_desc_block,
            sb.desc_size as usize,
            groups,
            block_size,
            &probe_read,
            csum_mode,
        )?;
        bgt.set_csum_ingredients(sb.uuid, sb.csum_seed());

        let fs = Arc::new(Self {
            fsid: Arc::as_ptr(&dev) as *const () as u64,
            dev,
            sb: Mutex::new(sb),
            bgt: Mutex::new(bgt),
            block_size,
            dev_block_size,
            ops_lock: Mutex::new(()),
            self_ref: Mutex::new(Weak::new()),
        });
        *fs.self_ref.lock() = Arc::downgrade(&fs);
        fs.mount_recovery()?;
        Ok(fs)
    }

    /// Recovers the owning `Arc<Ext4Fs>` (fails only if fs is being freed).
    pub fn as_arc(&self) -> Result<Arc<Self>> {
        self.self_ref.lock().upgrade().ok_or(Errno::EIO)
    }

    // -------------------------------------------------------------------
    // FS block / inode-table I/O
    // -------------------------------------------------------------------

    pub fn read_fs_block(&self, blk: u64, buf: &mut [u8]) -> Result<()> {
        read_fs_blocks(&*self.dev, self.block_size, self.dev_block_size, blk, buf)
    }

    pub fn write_fs_block(&self, blk: u64, buf: &[u8]) -> Result<()> {
        write_fs_blocks(&*self.dev, self.block_size, self.dev_block_size, blk, buf)
    }

    pub fn read_inode(&self, ino: u32) -> Result<RawInode> {
        let (total, ipg, inode_size) = {
            let sb = self.sb.lock();
            (
                sb.inodes_count,
                sb.inodes_per_group.max(1),
                sb.inode_size as usize,
            )
        };
        if ino == 0 || ino > total {
            return Err(Errno::EINVAL);
        }
        if (ino as u64 - 1) / ipg as u64 >= self.bgt.lock().count as u64 {
            return Err(Errno::EINVAL);
        }
        let g = ((ino - 1) / ipg) as usize;
        let index = ((ino - 1) % ipg) as usize;
        let table = self.bgt.lock().desc(g)?.inode_table;
        let rel = index * inode_size;
        let mut blk = vec![0u8; self.block_size];
        self.read_fs_block(table + (rel / self.block_size) as u64, &mut blk)?;
        let off = rel % self.block_size;
        RawInode::parse(&blk[off..off + inode_size], inode_size)
    }

    /// Writes back a raw inode, recomputing its checksum when enabled.
    pub fn write_inode(&self, ino: u32, raw: &mut RawInode) -> Result<()> {
        let (ipg, inode_size, metadata_csum, seed) = {
            let sb = self.sb.lock();
            (
                sb.inodes_per_group.max(1),
                sb.inode_size as usize,
                sb.has_metadata_csum(),
                sb.csum_seed(),
            )
        };
        if metadata_csum && inode_size >= 256 {
            recompute_inode_csum(&mut raw.raw, seed, ino, raw.generation);
        }
        let g = ((ino - 1) / ipg) as usize;
        let index = ((ino - 1) % ipg) as usize;
        let table = self.bgt.lock().desc(g)?.inode_table;
        let rel = index * inode_size;
        let mut blk = vec![0u8; self.block_size];
        self.read_fs_block(table + (rel / self.block_size) as u64, &mut blk)?;
        let off = rel % self.block_size;
        blk[off..off + inode_size].copy_from_slice(&raw.raw);
        self.write_fs_block(table + (rel / self.block_size) as u64, &blk)
    }

    // -------------------------------------------------------------------
    // Allocator glue
    // -------------------------------------------------------------------

    fn with_alloc<T>(&self, f: impl FnOnce(&mut AllocIo<'_>) -> Result<T>) -> Result<T> {
        let mut sb = self.sb.lock();
        let mut bgt = self.bgt.lock();
        let total_blocks = sb.blocks_count();
        let read = |blk: u64, buf: &mut [u8]| self.read_fs_block(blk, buf);
        let write = |blk: u64, buf: &[u8]| self.write_fs_block(blk, buf);
        let mut io = AllocIo {
            read_block: &read,
            write_block: &write,
            sb: &mut sb,
            bgt: &mut bgt,
            block_size: self.block_size,
            total_blocks,
        };
        f(&mut io)
    }

    pub fn alloc_block(&self, start_group: usize) -> Result<u64> {
        self.with_alloc(|io| io.alloc_block(start_group))
    }

    pub fn free_blocks(&self, phys: u64, len: u32) -> Result<()> {
        if len == 0 {
            return Ok(());
        }
        self.with_alloc(|io| io.free_blocks(phys, len))
    }

    /// Allocates one inode; returns `(ino, group)`.
    pub fn alloc_inode(&self, start_group: usize) -> Result<(u32, usize)> {
        let total = self.sb.lock().inodes_count;
        self.with_alloc(|io| io.alloc_inode(start_group, total))
    }

    pub fn free_inode(&self, ino: u32) -> Result<()> {
        self.with_alloc(|io| io.free_inode(ino))
    }

    // -------------------------------------------------------------------
    // Block mapping: extents and indirect trees
    // -------------------------------------------------------------------

    /// Runs `f` with an `ExtentCtx` bound to this filesystem.
    fn with_extent_ctx<T>(
        &self,
        ino: u32,
        inode_gen: u32,
        f: impl FnOnce(&ExtentCtx<'_>) -> Result<T>,
    ) -> Result<T> {
        let read = |blk: u64, buf: &mut [u8]| self.read_fs_block(blk, buf);
        let (bs, has64, csum_on, seed, rc) = {
            let sb = self.sb.lock();
            (
                self.block_size,
                sb.has_64bit(),
                sb.has_metadata_csum(),
                sb.csum_seed(),
                if sb.inode_size >= 256 {
                    (sb.inode_size as usize - 64) / 12
                } else {
                    4
                },
            )
        };
        let ctx = ExtentCtx {
            read_block: &read,
            block_size: bs,
            has_64bit: has64,
            ino,
            inode_gen,
            csum_on,
            csum_seed: seed,
            root_capacity: rc,
        };
        f(&ctx)
    }

    /// Read-only mapping of one logical block (no allocation, no writes).
    ///
    /// Returns `Ok(None)` for a hole.
    pub fn map_read(&self, ino: u32, raw: &RawInode, lblock: u64) -> Result<Option<u64>> {
        if raw.flags & iflags::EXTENTS != 0 {
            self.with_extent_ctx(ino, raw.generation, |ctx| {
                let list = extents::scan(&raw.raw, ctx)?;
                Ok(extents::lookup(&list, lblock as u32).map(|(p, _)| p))
            })
        } else {
            self.indirect_read(raw, lblock as u32)
                .map(|p| (p != 0).then_some(p))
        }
    }

    fn indirect_read(&self, raw: &RawInode, lblock: u32) -> Result<u64> {
        let read = |blk: u64, buf: &mut [u8]| self.read_fs_block(blk, buf);
        let write: Box<dyn Fn(u64, &[u8]) -> Result<()> + '_> =
            Box::new(|_blk: u64, _buf: &[u8]| Ok(()));
        let mut dummy_alloc = || -> Result<u64> { Err(Errno::EINVAL) };
        let io = IndirectIo {
            read_block: &read,
            write_block: write.as_ref(),
            alloc_block: &mut dummy_alloc,
            block_size: self.block_size,
        };
        indirect::map_read(raw, lblock, &io)
    }

    /// Reads one logical filesystem block of an inode into `buf`.
    pub fn read_inode_block(
        &self,
        ino: u32,
        raw: &RawInode,
        lblock: u64,
        buf: &mut [u8],
    ) -> Result<()> {
        match self.map_read(ino, raw, lblock)? {
            Some(phys) => self.read_fs_block(phys, buf),
            None => {
                buf.fill(0);
                Ok(())
            }
        }
    }

    /// Performs an HTree indexed directory lookup.
    pub fn htree_lookup(
        &self,
        ino: u32,
        raw: &RawInode,
        name: &str,
    ) -> Result<super::htree::HtreeResult> {
        super::htree::htree_lookup(self, ino, raw, name)
    }

    /// Maps one logical block, allocating when requested (write path).
    /// Mutates `raw` and updates persistent pointers / tree state.
    pub fn map_write(&self, ino: u32, raw: &mut RawInode, lblock: u64) -> Result<u64> {
        if lblock > u32::MAX as u64 {
            return Err(Errno::EFBIG);
        }
        let lb = lblock as u32;
        if raw.flags & iflags::EXTENTS != 0 {
            self.with_extent_ctx(ino, raw.generation, |ctx| {
                let mut list = extents::scan(&raw.raw, ctx)?;
                if let Some((phys, _)) = extents::lookup(&list, lb) {
                    return Ok(phys);
                }
                let groups = self.bgt.lock().count.max(1);
                let phys = self.alloc_block((ino as usize * 7) % groups)?;
                // Fast path: extend the trailing extent in place.
                let extends_last = list.last().is_some_and(|e| {
                    e.log + e.len == lb && e.phys + e.len as u64 == phys && e.len < MAX_EXTENT_LEN
                });
                if extends_last {
                    list.last_mut().unwrap().len += 1;
                } else {
                    extents::insert(
                        &mut list,
                        Extent {
                            log: lb,
                            len: 1,
                            phys,
                        },
                    );
                }
                self.rebuild_extents(ino, raw, &list, ctx)?;
                Ok(phys)
            })
        } else {
            // Legacy indirect tree: allocate-on-write at every depth.
            let groups = self.bgt.lock().count.max(1);
            let hint = (ino as usize * 7) % groups;
            let mut alc = || self.alloc_block(hint);
            let read = |blk: u64, buf: &mut [u8]| self.read_fs_block(blk, buf);
            let write = |blk: u64, buf: &[u8]| self.write_fs_block(blk, buf);
            let mut io = IndirectIo {
                read_block: &read,
                write_block: &write,
                alloc_block: &mut alc,
                block_size: self.block_size,
            };
            let phys = indirect::map_write(raw, lb, &mut io)?;
            let b512 = (self.block_size / 512) as u64;
            raw.set_blocks(self.recount_indirect_blocks(raw)? * b512);
            Ok(phys)
        }
    }

    /// Rebuilds the on-disk extent tree for the flat mapping `list`,
    /// freeing previously referenced tree blocks first.
    fn rebuild_extents(
        &self,
        _ino: u32,
        raw: &mut RawInode,
        list: &[Extent],
        ctx: &ExtentCtx<'_>,
    ) -> Result<()> {
        for phys in self.collect_meta_blocks(raw, ctx)? {
            self.free_blocks(phys, 1)?;
        }

        let plan = TreePlan::plan(list.len(), ctx.block_size, ctx.csum_on, ctx.root_capacity);
        let need = plan.total_blocks();
        let mut allocated = Vec::with_capacity(need);
        for _ in 0..need {
            let phys = self.alloc_block(0)?;
            let zero = vec![0u8; ctx.block_size];
            self.write_fs_block(phys, &zero)?;
            allocated.push(phys);
        }
        let built = extents::build_tree(list, &plan, &allocated, ctx)?;
        for (phys, image) in &built.writes {
            self.write_fs_block(*phys, image)?;
        }
        raw.raw[40..100].copy_from_slice(&built.root);
        // i_blocks: data extents + metadata blocks, in 512-byte units.
        let data_blocks: u64 = list.iter().map(|e| e.len as u64).sum();
        let units = (data_blocks + need as u64) * (self.block_size / 512) as u64;
        raw.set_blocks(units);
        Ok(())
    }

    /// Walks the tree and returns every referenced non-root block
    /// (leaves plus intermediate index blocks). Root-length invariants are
    /// trusted here; `collect_child` validates each level's header.
    fn collect_meta_blocks(&self, raw: &RawInode, _ctx: &ExtentCtx<'_>) -> Result<Vec<u64>> {
        let mut out = Vec::new();
        if raw.flags & iflags::EXTENTS == 0 || raw.raw.len() < 100 {
            return Ok(out);
        }
        let magic = u16::from_le_bytes([raw.raw[40], raw.raw[41]]);
        let depth = u16::from_le_bytes([raw.raw[44], raw.raw[45]]);
        if magic != 0xF3_0A || depth == 0 {
            return Ok(out);
        }
        let n = u16::from_le_bytes([raw.raw[42], raw.raw[43]]) as usize;
        for i in 0..n {
            let e = &raw.raw[52 + i * 12..64 + i * 12];
            let ptr = extent_entry_ptr(e);
            if ptr == 0 {
                continue;
            }
            // Child blocks at (depth - 1) levels; leaves count as metadata.
            out.push(ptr);
            if depth > 1 {
                self.collect_child(ptr, depth as u32 - 1, &mut out)?;
            }
        }
        Ok(out)
    }

    /// Recurses into an index block, collecting pointed-to blocks.
    fn collect_child(&self, ptr: u64, depth: u32, out: &mut Vec<u64>) -> Result<()> {
        if depth == 0 || out.len() > 1 << 20 {
            return Ok(());
        }
        let mut blk = vec![0u8; self.block_size];
        self.read_fs_block(ptr, &mut blk)?;
        let magic = u16::from_le_bytes([blk[0], blk[1]]);
        if magic != 0xF3_0A {
            return Err(Errno::EIO);
        }
        let n = u16::from_le_bytes([blk[2], blk[3]]) as usize;
        let d = u16::from_le_bytes([blk[4], blk[5]]) as u32;
        if d != depth {
            return Err(Errno::EIO);
        }
        for i in 0..n {
            let e = &blk[12 + i * 12..24 + i * 12];
            let cptr = extent_entry_ptr(e);
            if cptr == 0 {
                continue;
            }
            out.push(cptr);
            if d > 1 {
                self.collect_child(cptr, d - 1, out)?;
            }
        }
        Ok(())
    }

    /// Recounts allocated blocks (data + pointer blocks) for an indirect
    /// file; returns the raw count of FS blocks.
    fn recount_indirect_blocks(&self, raw: &RawInode) -> Result<u64> {
        let ppb = (self.block_size / 4) as u64;
        fn count(fs: &Ext4Fs, ptr: u64, depth: u32, ppb: u64) -> Result<u64> {
            if ptr == 0 {
                return Ok(0);
            }
            if depth == 0 {
                return Ok(1);
            }
            let mut buf = vec![0u8; fs.block_size];
            fs.read_fs_block(ptr, &mut buf)?;
            let mut total = 1;
            for i in 0..ppb as usize {
                let v = u32::from_le_bytes(buf[i * 4..i * 4 + 4].try_into().unwrap_or_default());
                if v != 0 {
                    total += count(fs, v as u64, depth - 1, ppb)?;
                }
            }
            Ok(total)
        }
        let mut total = 0u64;
        for (i, b) in raw.block.iter().enumerate() {
            let depth = match i {
                0..=11 => 0,
                12 => 1,
                13 => 2,
                _ => 3,
            };
            if *b == 0 {
                continue;
            }
            if depth == 0 {
                total += 1;
            } else {
                total += count(self, *b as u64, depth, ppb)?;
            }
        }
        Ok(total)
    }

    // -------------------------------------------------------------------
    // File data I/O
    // -------------------------------------------------------------------

    /// Reads file content; holes are zero-filled and the result clamps at EOF.
    pub fn file_read(
        &self,
        ino: u32,
        raw: &RawInode,
        offset: u64,
        buf: &mut [u8],
    ) -> Result<usize> {
        let size = raw.size();
        if offset >= size || buf.is_empty() {
            return Ok(0);
        }
        let end = (offset + buf.len() as u64).min(size);
        let bs = self.block_size as u64;
        let mut out = Vec::with_capacity((end - offset) as usize);
        let mut pos = offset;
        while pos < end {
            let lblock = pos / bs;
            let rel = pos % bs;
            let take = (bs - rel).min(end - pos) as usize;
            match self.map_read(ino, raw, lblock)? {
                Some(phys) => {
                    let mut blk = vec![0u8; self.block_size];
                    self.read_fs_block(phys, &mut blk)?;
                    out.extend_from_slice(&blk[rel as usize..rel as usize + take]);
                }
                None => out.resize(out.len() + take, 0),
            }
            pos += take as u64;
            if out.len() == (end - offset) as usize {
                break;
            }
        }
        buf[..out.len()].copy_from_slice(&out);
        Ok(out.len())
    }

    /// Writes file content, allocating blocks and extending the size.
    /// Returns bytes written.
    pub fn file_write(
        &self,
        ino: u32,
        raw: &mut RawInode,
        offset: u64,
        buf: &[u8],
    ) -> Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let bs = self.block_size as u64;
        let mut pos = offset;
        let mut written = 0usize;
        while written < buf.len() {
            let lblock = pos / bs;
            let rel = pos % bs;
            let take = (bs - rel).min((buf.len() - written) as u64) as usize;
            let phys = self.map_write(ino, raw, lblock)?;
            let mut blk = vec![0u8; self.block_size];
            if rel != 0 || take != self.block_size {
                self.read_fs_block(phys, &mut blk)?;
            }
            blk[rel as usize..rel as usize + take].copy_from_slice(&buf[written..written + take]);
            self.write_fs_block(phys, &blk)?;
            written += take;
            pos += take as u64;
        }
        let new_end = offset + buf.len() as u64;
        if new_end > raw.size() {
            raw.set_size(new_end);
        }
        let now = nowsecs32();
        raw.mtime = now;
        raw.ctime = now;
        self.write_inode(ino, raw)?;
        Ok(written)
    }

    /// Truncates (sparse when growing) to `new_size`.
    pub fn file_truncate(&self, ino: u32, raw: &mut RawInode, new_size: u64) -> Result<()> {
        let old = raw.size();
        if new_size > old {
            raw.set_size(new_size);
            raw.ctime = nowsecs32();
            self.write_inode(ino, raw)?;
            return Ok(());
        }
        if new_size == old {
            return Ok(());
        }

        let bs = self.block_size as u64;
        let boundary = new_size.div_ceil(bs) as u32; // first logical block kept
        if raw.flags & iflags::EXTENTS != 0 {
            self.with_extent_ctx(ino, raw.generation, |ctx| {
                let mut list = extents::scan(&raw.raw, ctx)?;
                let freed = extents::truncate(&mut list, boundary);
                let mut freed_blocks = 0u64;
                for (phys, len) in &freed {
                    self.free_blocks(*phys, *len)?;
                    freed_blocks += *len as u64;
                }
                self.rebuild_extents(ino, raw, &list, ctx)?;
                Ok(freed_blocks)
            })?;
        } else {
            let freed = self.free_indirect_tail(raw, boundary)?;
            let b512 = (self.block_size / 512) as u64;
            raw.set_blocks(self.recount_indirect_blocks(raw)? * b512);
            let _ = freed;
        }
        raw.set_size(new_size);
        let now = nowsecs32();
        raw.mtime = now;
        raw.ctime = now;
        self.write_inode(ino, raw)?;
        Ok(())
    }

    /// Frees all indirect-tree blocks covering logical blocks
    /// `[boundary, ∞)`, including pointer blocks that become empty.
    fn free_indirect_tail(&self, raw: &mut RawInode, boundary: u32) -> Result<u64> {
        let b = (self.block_size / 4) as u32;
        let mut freed = 0u64;

        // Direct blocks: logical 0..12.
        for i in boundary.min(12)..12 {
            let phys = raw.block[i as usize] as u64;
            if phys != 0 {
                self.free_blocks(phys, 1)?;
                freed += 1;
                raw.block[i as usize] = 0;
            }
        }
        // Single / double / triple subtrees.
        let mut regions: [(usize, u64); 3] = [
            (12, 12),
            (13, 12 + b as u64),
            (14, 12 + b as u64 + b as u64 * b as u64),
        ];
        for (slot, start) in regions {
            let ptr = raw.block[slot] as u64;
            if ptr == 0 {
                continue;
            }
            let depth = slot - 11; // 12→1, 13→2, 14→3 pointer levels
            let start = start as u64;
            let region_len = match slot {
                12 => b as u64,
                13 => b as u64 * b as u64,
                _ => u64::MAX / 2,
            };
            let boundary_u = boundary as u64;
            if boundary_u <= start {
                // Entire subtree freed.
                freed += self.free_subtree(ptr, depth as u32)?;
                raw.block[slot] = 0;
                raw.raw[40 + slot * 4..44 + slot * 4].copy_from_slice(&0u32.to_le_bytes());
            } else if boundary_u < start + region_len {
                // Partial: free data blocks from the boundary on; collapse
                // empty pointer blocks along the way.
                let rel = (boundary_u - start) as u32;
                freed += self.free_range(ptr, depth as u32, rel, region_len as u32)?;
                if self.block_all_zero(ptr)? {
                    self.free_blocks(ptr, 1)?;
                    freed += 1;
                    raw.block[slot] = 0;
                    raw.raw[40 + slot * 4..44 + slot * 4].copy_from_slice(&0u32.to_le_bytes());
                }
            }
        }
        // Persist the patched pointer array.
        for (i, v) in raw.block.iter().enumerate() {
            raw.raw[40 + i * 4..44 + i * 4].copy_from_slice(&v.to_le_bytes());
        }
        let _ = &mut regions;
        Ok(freed)
    }

    /// Frees an entire pointer subtree (`depth` levels); returns blocks freed.
    fn free_subtree(&self, ptr: u64, depth: u32) -> Result<u64> {
        if depth == 0 {
            self.free_blocks(ptr, 1)?;
            return Ok(1);
        }
        let mut freed = self.free_blocks(ptr, 1).map(|_| 1u64)?;
        let mut buf = vec![0u8; self.block_size];
        self.read_fs_block(ptr, &mut buf)?;
        for i in 0..self.block_size / 4 {
            let v = u32::from_le_bytes(buf[i * 4..i * 4 + 4].try_into().unwrap_or_default());
            if v == 0 {
                continue;
            }
            if depth == 1 {
                freed += 1;
                self.free_blocks(v as u64, 1)?;
            } else {
                freed += self.free_subtree(v as u64, depth - 1)?;
            }
        }
        Ok(freed)
    }

    /// Frees logical range `[rel_lo, rel_hi)` inside pointer block `ptr`
    /// whose entries cover `depth` pointer levels. Pointer blocks that empty
    /// out are freed too.
    fn free_range(&self, ptr: u64, depth: u32, rel_lo: u32, rel_hi: u32) -> Result<u64> {
        let b = (self.block_size / 4) as u32;
        if ptr == 0 || rel_lo >= rel_hi {
            return Ok(0);
        }
        let mut freed = 0u64;
        let mut buf = vec![0u8; self.block_size];
        self.read_fs_block(ptr, &mut buf)?;
        let child_span = b.pow(depth - 1);
        let first_child = rel_lo / child_span;
        let last_child = (rel_hi.div_ceil(child_span)).min(b);
        for k in first_child..last_child {
            let v = u32::from_le_bytes(
                buf[(k * 4) as usize..(k * 4 + 4) as usize]
                    .try_into()
                    .unwrap_or_default(),
            );
            if v == 0 {
                continue;
            }
            let child_lo = k * child_span;
            let child_hi = child_lo + child_span;
            let lo = rel_lo.max(child_lo) - child_lo;
            let hi = rel_hi.min(child_hi) - child_lo;
            if depth == 1 {
                freed += 1;
                self.free_blocks(v as u64, 1)?;
                buf[(k * 4) as usize..(k * 4 + 4) as usize].copy_from_slice(&0u32.to_le_bytes());
            } else {
                freed += self.free_range(v as u64, depth - 1, lo, hi)?;
                if self.block_all_zero(v as u64)? {
                    freed += 1;
                    self.free_blocks(v as u64, 1)?;
                    buf[(k * 4) as usize..(k * 4 + 4) as usize]
                        .copy_from_slice(&0u32.to_le_bytes());
                }
            }
        }
        self.write_fs_block(ptr, &buf)?;
        Ok(freed)
    }

    fn block_all_zero(&self, ptr: u64) -> Result<bool> {
        if ptr == 0 {
            return Ok(true);
        }
        let mut buf = vec![0u8; self.block_size];
        self.read_fs_block(ptr, &mut buf)?;
        Ok(buf.iter().all(|b| *b == 0))
    }

    // -------------------------------------------------------------------
    // Directory data
    // -------------------------------------------------------------------

    pub fn read_dir_data(&self, ino: u32, raw: &RawInode) -> Result<Vec<u8>> {
        let size = raw.size().min(64 * 1024 * 1024) as usize;
        let mut buf = vec![0u8; size];
        let n = self.file_read(ino, raw, 0, &mut buf)?;
        buf.truncate(n);
        Ok(buf)
    }

    /// Recomputes directory-block checksum tails (metadata_csum).
    fn fix_dir_tails(&self, data: &mut [u8], ino: u32, inode_gen: u32) {
        let (csum_on, seed) = {
            let sb = self.sb.lock();
            (sb.has_metadata_csum(), sb.csum_seed())
        };
        if !csum_on {
            return;
        }
        let bs = self.block_size;
        let mut off = 0usize;
        while off + bs <= data.len() {
            let blk = &mut data[off..off + bs];
            let tail = bs - 12;
            if blk[tail + 7] == DIRTAIL_TYPE && blk[tail..tail + 4] == [0, 0, 0, 0] {
                let mut state = crc32c(seed, &ino.to_le_bytes());
                state = crc32c(state, &inode_gen.to_le_bytes());
                state = crc32c(state, &blk[0..tail]);
                blk[tail + 8..tail + 12].copy_from_slice(&state.to_le_bytes());
            }
            off += bs;
        }
    }

    pub fn write_dir_data(&self, ino: u32, raw: &mut RawInode, data: &mut Vec<u8>) -> Result<()> {
        while data.len() % self.block_size != 0 {
            data.push(0);
        }
        let inode_gen = raw.generation;
        self.fix_dir_tails(data, ino, inode_gen);
        for off in (0..data.len()).step_by(self.block_size) {
            let lblock = (off / self.block_size) as u64;
            let phys = self.map_write(ino, raw, lblock)?;
            self.write_fs_block(phys, &data[off..off + self.block_size])?;
        }
        raw.set_size(data.len() as u64);
        let now = nowsecs32();
        raw.mtime = now;
        raw.ctime = now;
        self.write_inode(ino, raw)?;
        Ok(())
    }

    // -------------------------------------------------------------------
    // Mount recovery, statfs, sync
    // -------------------------------------------------------------------

    fn write_sb_bytes(&self, sb_raw: &[u8]) -> Result<()> {
        let blk = (1024 / self.block_size) as u64;
        let off = 1024 % self.block_size;
        let mut b = vec![0u8; self.block_size];
        self.read_fs_block(blk, &mut b)?;
        b[off..off + 1024].copy_from_slice(&sb_raw[0..1024]);
        self.write_fs_block(blk, &b)
    }

    /// Replays the journal when the fs was not cleanly unmounted, then
    /// stamps the superblock as a valid clean mount.
    fn mount_recovery(self: &Arc<Self>) -> Result<()> {
        let (jino, needs_journal) = {
            let sb = self.sb.lock();
            (
                sb.journal_inum,
                sb.state != EXT4_VALID_FS && sb.has_journal(),
            )
        };
        if needs_journal && jino != 0 && jino <= self.sb.lock().inodes_count {
            let jraw = self.read_inode(jino)?;
            let inode_gen = jraw.generation;
            let use_extents = jraw.flags & iflags::EXTENTS != 0;
            let map = |lb: u64| -> Result<u64> {
                if use_extents {
                    self.with_extent_ctx(jino, inode_gen, |ctx| {
                        let list = extents::scan(&jraw.raw, ctx)?;
                        Ok(extents::map(&list, lb as u32))
                    })
                } else {
                    self.indirect_read(&jraw, lb as u32)
                }
            };
            let read_jb = |lb: u64, buf: &mut [u8]| -> Result<()> {
                let phys = map(lb)?;
                if phys == 0 {
                    return Err(Errno::EIO);
                }
                self.read_fs_block(phys, buf)
            };
            let write_jb = |lb: u64, buf: &[u8]| -> Result<()> {
                let phys = map(lb)?;
                if phys == 0 {
                    return Err(Errno::EIO);
                }
                self.write_fs_block(phys, buf)
            };

            let mut jsb_buf = vec![0u8; self.block_size];
            read_jb(0, &mut jsb_buf)?;
            let jsb = JournalSb::parse(&jsb_buf)?;
            let io = JournalIo {
                read_journal_block: &read_jb,
                write_fs_block: &|blk: u64, buf: &[u8]| self.write_fs_block(blk, buf),
            };
            journal::replay(&jsb, &io, self.block_size)?;
            // Mark the log empty (`s_start = 0`) so nothing replays twice.
            let empty = journal::make_empty_sb_bytes(&jsb_buf, jsb.feature_incompat)?;
            write_jb(0, &empty)?;
        }
        {
            let mut sb = self.sb.lock();
            sb.set_state(EXT4_VALID_FS);
            sb.record_mount();
            sb.recompute_sb_checksum();
            self.write_sb_bytes(&sb.raw)?;
        }
        Ok(())
    }

    pub fn statfs_snapshot(&self) -> StatFs {
        let sb = self.sb.lock();
        let total = sb.blocks_count();
        let free = sb.free_blocks_count();
        let reserved = sb.r_blocks_count();
        StatFs {
            bsize: self.block_size as u64,
            blocks: total,
            bfree: free,
            bavail: free.saturating_sub(reserved),
            files: sb.inodes_count as u64,
            ffree: sb.free_inodes_count as u64,
            namelen: 255,
        }
    }

    pub fn sync_all(&self) -> Result<()> {
        let write = |blk: u64, buf: &[u8]| self.write_fs_block(blk, buf);
        self.bgt.lock().flush(&write)?;
        {
            let mut sb = self.sb.lock();
            sb.recompute_sb_checksum();
            self.write_sb_bytes(&sb.raw)?;
        }
        self.dev.flush()?;
        Ok(())
    }
}

/// Extracts the 48-bit tree pointer from a 12-byte extent/index entry
/// (both encodings place `hi` at bytes 8..10 and `lo` at bytes 4..8).
fn extent_entry_ptr(e: &[u8]) -> u64 {
    let hi = u16::from_le_bytes([e[8], e[9]]) as u64;
    let lo = u32::from_le_bytes([e[4], e[5], e[6], e[7]]) as u64;
    (hi << 32) | lo
}

// ---------------------------------------------------------------------------
// Generic device byte/block readers
// ---------------------------------------------------------------------------

/// Reads `buf.len()` bytes starting at device-byte `byte_offset`, honoring
/// arbitrary FS/device block-size ratios.
pub fn dev_read_bytes(dev: &dyn BlockDevice, byte_offset: u64, buf: &mut [u8]) -> Result<()> {
    let dbs = dev.block_size() as u64;
    if dbs == 0 {
        return Err(Errno::EINVAL);
    }
    let mut off = 0usize;
    let mut sector = byte_offset / dbs;
    let in_sector = (byte_offset % dbs) as usize;
    if in_sector > 0 {
        let mut tmp = vec![0u8; dbs as usize];
        dev.read_blocks(sector, &mut tmp)?;
        if (buf.len()).min(dbs as usize - in_sector) > 0 {
            let take = buf.len().min(dbs as usize - in_sector);
            buf[off..off + take].copy_from_slice(&tmp[in_sector..in_sector + take]);
            off += take;
        }
        sector += 1;
    }
    while off < buf.len() {
        let take = (buf.len() - off).min(dbs as usize);
        let mut tmp = vec![0u8; dbs as usize];
        dev.read_blocks(sector, &mut tmp)?;
        buf[off..off + take].copy_from_slice(&tmp[0..take]);
        off += take;
        sector += 1;
    }
    Ok(())
}

/// Reads FS block `blk` into `buf` (length = FS block size).
pub fn read_fs_blocks(
    dev: &dyn BlockDevice,
    fs_bs: usize,
    dev_bs: usize,
    blk: u64,
    buf: &mut [u8],
) -> Result<()> {
    if fs_bs % dev_bs == 0 {
        let spb = (fs_bs / dev_bs) as u64;
        let mut pad = vec![0u8; fs_bs];
        let got = dev.read_blocks(blk * spb, &mut pad)?;
        if got == 0 {
            return Err(Errno::EIO);
        }
        buf.copy_from_slice(&pad);
        return Ok(());
    }
    dev_read_bytes(dev, blk * fs_bs as u64, buf)
}

/// Writes FS block `blk` from `buf`.
pub fn write_fs_blocks(
    dev: &dyn BlockDevice,
    fs_bs: usize,
    dev_bs: usize,
    blk: u64,
    buf: &[u8],
) -> Result<()> {
    if fs_bs % dev_bs == 0 {
        let spb = (fs_bs / dev_bs) as u64;
        dev.write_blocks(blk * spb, buf)?;
        return Ok(());
    }
    // FS block smaller than (or unaligned with) the device sector:
    // read-modify-write each covered sector.
    let dbs = dev_bs as u64;
    let byte_start = blk * fs_bs as u64;
    let byte_end = byte_start + fs_bs as u64;
    let first = byte_start / dbs;
    let last = (byte_end - 1) / dbs;
    let mut tmp = vec![0u8; dev_bs];
    for s in first..=last {
        dev.read_blocks(s, &mut tmp)?;
        let sec_lo = s * dbs;
        let src_lo = byte_start.max(sec_lo) - sec_lo;
        let src_hi = byte_end.min(sec_lo + dbs) - sec_lo;
        let dst = (byte_start.max(sec_lo) - byte_start) as usize;
        tmp[src_lo as usize..src_hi as usize]
            .copy_from_slice(&buf[dst..dst + (src_hi - src_lo) as usize]);
        dev.write_blocks(s, &tmp)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Inode construction helpers
// ---------------------------------------------------------------------------

/// Serializes a fresh inode image from its initial fields.
fn encode_new_inode(
    inode_size: usize,
    mode: u16,
    flags: u32,
    links: u16,
    uid: u32,
    gid: u32,
    rdev: Option<u32>,
    empty_extent_root: Option<usize>,
) -> RawInode {
    let now = nowsecs32();
    let mut raw = vec![0u8; inode_size];
    raw[0..2].copy_from_slice(&mode.to_le_bytes());
    raw[2..4].copy_from_slice(&(uid as u16).to_le_bytes());
    raw[8..12].copy_from_slice(&now.to_le_bytes());
    raw[12..16].copy_from_slice(&now.to_le_bytes());
    raw[16..20].copy_from_slice(&now.to_le_bytes());
    raw[24..26].copy_from_slice(&(gid as u16).to_le_bytes());
    raw[26..28].copy_from_slice(&links.to_le_bytes());
    raw[32..36].copy_from_slice(&flags.to_le_bytes());
    if let Some(dev) = rdev {
        raw[40..44].copy_from_slice(&dev.to_le_bytes());
    }
    if let Some(root_capacity) = empty_extent_root {
        // Empty extent header inline at 40: magic, entries=0, max, depth=0.
        raw[40..42].copy_from_slice(&0xF3_0Au16.to_le_bytes());
        raw[42..44].copy_from_slice(&0u16.to_le_bytes());
        raw[44..46].copy_from_slice(&(root_capacity as u16).to_le_bytes());
        raw[46..48].copy_from_slice(&0u16.to_le_bytes());
    }
    RawInode::parse(&raw, inode_size).unwrap()
}

/// Root capacity of the inline extent area for a given inode size.
fn extent_root_capacity(inode_size: usize) -> usize {
    if inode_size >= 256 {
        (inode_size - 64) / 12
    } else {
        4
    }
}

/// Builds the VFS inode + attribute pair for an on-disk inode.
fn build_inode(fs: &Arc<Ext4Fs>, ino: u32, raw: &RawInode) -> Arc<INode> {
    let attr = INodeAttr {
        size: usize::try_from(raw.size()).unwrap_or(usize::MAX),
        mode: Mode::from_bits_truncate(raw.mode as u32),
        nlink: raw.links_count as usize,
        uid: raw.uid,
        gid: raw.gid,
        rdev: raw.block[0] as u64,
    };
    Arc::new(INode::new_with_attr(
        Box::new(Ext4Node {
            fs: Arc::clone(fs),
            ino,
        }),
        ino as usize,
        attr,
    ))
}

impl Ext4Fs {
    /// Constructs the VFS inode for an on-disk inode number.
    pub fn node_for(&self, ino: u32) -> Result<Arc<INode>> {
        let fs = self.as_arc()?;
        let raw = self.read_inode(ino)?;
        Ok(build_inode(&fs, ino, &raw))
    }

    /// The dirent type byte matching an inode's mode.
    fn ftype_of(raw: &RawInode) -> u8 {
        match raw.mode & 0o170000 {
            0o040000 => ftype::DIR,
            0o100000 => ftype::REGULAR,
            0o120000 => ftype::SYMLINK,
            0o020000 => ftype::CHR,
            0o060000 => ftype::BLK,
            0o010000 => ftype::FIFO,
            0o140000 => ftype::SOCK,
            _ => ftype::UNKNOWN,
        }
    }

    /// Removes a dirent by name; returns the `(ino, type)` it referenced.
    pub fn dir_remove(&self, dir_ino: u32, name: &str) -> Result<(u32, u8)> {
        let bs = self.block_size;
        let mut dir_raw = self.read_inode(dir_ino)?;
        let mut data = self.read_dir_data(dir_ino, &dir_raw)?;
        let entries = dir::parse_all(&data, bs);
        let target = dir::delete_entry(&mut data, &entries, name)?;
        self.write_dir_data(dir_ino, &mut dir_raw, &mut data)?;
        Ok(target)
    }

    /// Sets the `..` reference of a directory (rename fixup).
    pub fn dir_set_dotdot(&self, dir_ino: u32, parent: u32) -> Result<()> {
        let bs = self.block_size;
        let mut dir_raw = self.read_inode(dir_ino)?;
        let mut data = self.read_dir_data(dir_ino, &dir_raw)?;
        let entries = dir::parse_all(&data, bs);
        dir::set_dotdot(&mut data, &entries, parent)?;
        self.write_dir_data(dir_ino, &mut dir_raw, &mut data)?;
        Ok(())
    }

    /// Updates an entry's inode reference in-place.
    pub fn dir_update(&self, dir_ino: u32, name: &str, new_ino: u32) -> Result<()> {
        let bs = self.block_size;
        let mut dir_raw = self.read_inode(dir_ino)?;
        let mut data = self.read_dir_data(dir_ino, &dir_raw)?;
        let entries = dir::parse_all(&data, bs);
        dir::update_entry(&mut data, &entries, name, new_ino)?;
        self.write_dir_data(dir_ino, &mut dir_raw, &mut data)?;
        Ok(())
    }

    /// Inserts `name → child_ino` into in-memory directory `data`, growing
    /// the file by one block when out of slack; persists via `write_dir_data`.
    pub fn insert_dirent_with_growth(
        &self,
        dir_ino: u32,
        dir_raw: &mut RawInode,
        data: &mut Vec<u8>,
        name: &str,
        child_ino: u32,
        child_type: u8,
    ) -> Result<()> {
        let bs = self.block_size;
        let csum_on = self.sb.lock().has_metadata_csum();
        let dir_end = data.len();
        match dir::insert_entry(data, bs, name, child_ino, child_type, dir_end) {
            Ok(()) => {}
            Err(Errno::ENOSPC) => {
                data.resize(data.len() + bs, 0);
                let base = data.len() - bs;
                let need = ((8 + name.len()) + 3) & !3;
                dir::write_dirent(data, base, child_ino, child_type, name, need as u16);
                let rest = bs - need;
                let free_len = rest - if csum_on { 12 } else { 0 };
                if free_len >= 8 {
                    dir::write_dirent(data, base + need, 0, 0, "", free_len as u16);
                } else {
                    // Marginal: entry swallows the block's remaining slack.
                    dir::write_dirent(data, base, child_ino, child_type, name, rest as u16);
                }
                if csum_on {
                    dir::write_dirent(data, bs - 12, 0, 0, "", 12);
                    let tail_off = data.len() - 12;
                    data[tail_off - 12 + 7] = DIRTAIL_TYPE;
                }
            }
            Err(e) => return Err(e),
        }
        self.write_dir_data(dir_ino, dir_raw, data)
    }
}

// ---------------------------------------------------------------------------
// VFS node operations
// ---------------------------------------------------------------------------

/// One back-end for every on-disk inode kind; methods dispatch on the
/// inode's type bits.
pub struct Ext4Node {
    pub fs: Arc<Ext4Fs>,
    pub ino: u32,
}

impl NodeOps for Ext4Node {
    fn fsid(&self) -> Option<u64> {
        Some(self.fs.fsid)
    }

    /// Lookup handles ".", ".." and regular entries.
    fn lookup(&self, name: &str) -> Result<Arc<INode>> {
        let fs = &self.fs;
        let _g = fs.ops_lock.lock();
        if name.contains('/') {
            return Err(Errno::EINVAL);
        }
        let raw = fs.read_inode(self.ino)?;
        if !raw.is_dir() {
            return Err(Errno::ENOTDIR);
        }

        // Fast path: Ext4 HTree directory indexing
        if raw.is_indexed_dir() {
            match fs.htree_lookup(self.ino, &raw, name)? {
                super::htree::HtreeResult::Found(child, _) => return fs.node_for(child),
                super::htree::HtreeResult::NotFound => return Err(Errno::ENOENT),
                super::htree::HtreeResult::Fallback => {}
            }
        }

        let data = fs.read_dir_data(self.ino, &raw)?;
        let entries = dir::parse_all(&data, fs.block_size);
        let (child, _) = dir::find(&entries, name).ok_or(Errno::ENOENT)?;
        fs.node_for(child)
    }

    fn create(&self, name: &str, mode: Mode) -> Result<Arc<INode>> {
        let fs = &self.fs;
        let _g = fs.ops_lock.lock();
        let mut dir_raw = fs.read_inode(self.ino)?;
        if !dir_raw.is_dir() {
            return Err(Errno::ENOTDIR);
        }
        let mut data = fs.read_dir_data(self.ino, &dir_raw)?;
        if dir::find(&dir::parse_all(&data, fs.block_size), name).is_some() {
            return Err(Errno::EEXIST);
        }
        let cred = crate::fs::vfs::perms::current_cred();
        let (inode_size, uses_extents, groups) = {
            let sb = fs.sb.lock();
            (
                sb.inode_size as usize,
                sb.uses_extents(),
                fs.bgt.lock().count.max(1),
            )
        };
        let mut raw = encode_new_inode(
            inode_size,
            mode.bits() as u16,
            if uses_extents { iflags::EXTENTS } else { 0 },
            1,
            cred.cr_uid.as_u32(),
            cred.cr_gid.as_u32(),
            None,
            if uses_extents {
                Some(extent_root_capacity(inode_size))
            } else {
                None
            },
        );
        let (new_ino, _) = fs.alloc_inode((self.ino as usize * 7) % groups)?;
        fs.write_inode(new_ino, &mut raw)?;
        fs.insert_dirent_with_growth(
            self.ino,
            &mut dir_raw,
            &mut data,
            name,
            new_ino,
            ftype::REGULAR,
        )?;
        let node = fs.node_for(new_ino)?;
        Ok(node)
    }

    fn mkdir(&self, name: &str, mode: Mode) -> Result<Arc<INode>> {
        let fs = &self.fs;
        let _g = fs.ops_lock.lock();
        let mut dir_raw = fs.read_inode(self.ino)?;
        if !dir_raw.is_dir() {
            return Err(Errno::ENOTDIR);
        }
        let mut data = fs.read_dir_data(self.ino, &dir_raw)?;
        if dir::find(&dir::parse_all(&data, fs.block_size), name).is_some() {
            return Err(Errno::EEXIST);
        }
        let cred = crate::fs::vfs::perms::current_cred();
        let (inode_size, uses_extents, csum_on, groups) = {
            let sb = fs.sb.lock();
            (
                sb.inode_size as usize,
                sb.uses_extents(),
                sb.has_metadata_csum(),
                fs.bgt.lock().count.max(1),
            )
        };
        let mut raw = encode_new_inode(
            inode_size,
            (mode.bits() | 0o040000) as u16,
            if uses_extents { iflags::EXTENTS } else { 0 },
            2,
            cred.cr_uid.as_u32(),
            cred.cr_gid.as_u32(),
            None,
            if uses_extents {
                Some(extent_root_capacity(inode_size))
            } else {
                None
            },
        );
        let (new_ino, _) = fs.alloc_inode((self.ino as usize * 7) % groups)?;

        // Initial directory content: ".", "..", and the checksum tail.
        let bs = fs.block_size;
        let mut init = vec![0u8; bs];
        dir::write_dirent(&mut init, 0, new_ino, ftype::DIR, ".", 12);
        let rest = bs - 12 - if csum_on { 12 } else { 0 };
        dir::write_dirent(&mut init, 12, self.ino, ftype::DIR, "..", rest as u16);
        if csum_on {
            dir::write_dirent(&mut init, bs - 12, 0, 0, "", 12);
            init[bs - 12 + 7] = DIRTAIL_TYPE;
        }
        fs.write_inode(new_ino, &mut raw)?;
        fs.write_dir_data(new_ino, &mut raw, &mut init)?;
        fs.insert_dirent_with_growth(self.ino, &mut dir_raw, &mut data, name, new_ino, ftype::DIR)?;

        // Bump the parent's link count for the new `..` reference.
        dir_raw.links_count += 1;
        dir_raw.raw[26..28].copy_from_slice(&dir_raw.links_count.to_le_bytes());
        dir_raw.mtime = nowsecs32();
        fs.write_inode(self.ino, &mut dir_raw)?;
        fs.node_for(new_ino)
    }

    fn rmdir(&self, name: &str) -> Result<()> {
        let fs = &self.fs;
        let _g = fs.ops_lock.lock();
        if name == "." || name == ".." {
            return Err(Errno::EINVAL);
        }
        let mut dir_raw = fs.read_inode(self.ino)?;
        if !dir_raw.is_dir() {
            return Err(Errno::ENOTDIR);
        }
        let data = fs.read_dir_data(self.ino, &dir_raw)?;
        let entries = dir::parse_all(&data, fs.block_size);
        let (child, _) = dir::find(&entries, name).ok_or(Errno::ENOENT)?;
        let mut child_raw = fs.read_inode(child)?;
        if !child_raw.is_dir() {
            return Err(Errno::ENOTDIR);
        }
        let child_data = fs.read_dir_data(child, &child_raw)?;
        let child_entries = dir::parse_all(&child_data, fs.block_size);
        if child_entries
            .iter()
            .any(|e| e.ino != 0 && e.name != "." && e.name != "..")
        {
            return Err(Errno::ENOTEMPTY);
        }
        fs.file_truncate(child, &mut child_raw, 0)?;
        child_raw.dtime = nowsecs32();
        child_raw.links_count = 0;
        child_raw.raw[20..24].copy_from_slice(&child_raw.dtime.to_le_bytes());
        child_raw.raw[26..28].copy_from_slice(&0u16.to_le_bytes());
        fs.dir_remove(self.ino, name)?;
        dir_raw.links_count = dir_raw.links_count.saturating_sub(1);
        dir_raw.raw[26..28].copy_from_slice(&dir_raw.links_count.to_le_bytes());
        dir_raw.mtime = nowsecs32();
        fs.write_inode(self.ino, &mut dir_raw)?;
        fs.free_inode(child)?;
        fs.write_inode(child, &mut child_raw)
    }

    fn unlink(&self, name: &str) -> Result<()> {
        let fs = &self.fs;
        let _g = fs.ops_lock.lock();
        if name == "." || name == ".." {
            return Err(Errno::EINVAL);
        }
        let mut dir_raw = fs.read_inode(self.ino)?;
        if !dir_raw.is_dir() {
            return Err(Errno::ENOTDIR);
        }
        let data = fs.read_dir_data(self.ino, &dir_raw)?;
        let entries = dir::parse_all(&data, fs.block_size);
        let (child, _) = dir::find(&entries, name).ok_or(Errno::ENOENT)?;
        let mut child_raw = fs.read_inode(child)?;
        if child_raw.is_dir() {
            return Err(Errno::EISDIR);
        }
        fs.dir_remove(self.ino, name)?;
        dir_raw.mtime = nowsecs32();
        fs.write_inode(self.ino, &mut dir_raw)?;
        child_raw.links_count = child_raw.links_count.saturating_sub(1);
        if child_raw.links_count == 0 {
            child_raw.dtime = nowsecs32();
            child_raw.raw[20..24].copy_from_slice(&child_raw.dtime.to_le_bytes());
            child_raw.raw[26..28].copy_from_slice(&0u16.to_le_bytes());
            fs.write_inode(child, &mut child_raw)?;
            fs.file_truncate(child, &mut child_raw, 0)?;
            fs.free_inode(child)?;
            return Ok(());
        }
        fs.write_inode(child, &mut child_raw)
    }

    fn link(&self, name: &str, other: Arc<INode>) -> Result<()> {
        let fs = &self.fs;
        let _g = fs.ops_lock.lock();
        if other.node_ops.fsid() != Some(fs.fsid) {
            return Err(Errno::EXDEV);
        }
        let target = other.id as u32;
        let mut target_raw = fs.read_inode(target)?;
        if target_raw.is_dir() {
            return Err(Errno::EISDIR);
        }
        let mut dir_raw = fs.read_inode(self.ino)?;
        let mut data = fs.read_dir_data(self.ino, &dir_raw)?;
        fs.insert_dirent_with_growth(
            self.ino,
            &mut dir_raw,
            &mut data,
            name,
            target,
            ftype_of_mode(target_raw.mode),
        )?;
        target_raw.links_count += 1;
        target_raw.raw[26..28].copy_from_slice(&target_raw.links_count.to_le_bytes());
        target_raw.ctime = nowsecs32();
        fs.write_inode(target, &mut target_raw)
    }

    fn mknod(&self, name: &str, mode: Mode, rdev: u64) -> Result<Arc<INode>> {
        let fs = &self.fs;
        let _g = fs.ops_lock.lock();
        let mut dir_raw = fs.read_inode(self.ino)?;
        if !dir_raw.is_dir() {
            return Err(Errno::ENOTDIR);
        }
        let mut data = fs.read_dir_data(self.ino, &dir_raw)?;
        if dir::find(&dir::parse_all(&data, fs.block_size), name).is_some() {
            return Err(Errno::EEXIST);
        }
        let cred = crate::fs::vfs::perms::current_cred();
        let (inode_size, uses_extents, groups) = {
            let sb = fs.sb.lock();
            (
                sb.inode_size as usize,
                sb.uses_extents(),
                fs.bgt.lock().count.max(1),
            )
        };
        let mut raw = encode_new_inode(
            inode_size,
            mode.bits() as u16,
            if uses_extents { iflags::EXTENTS } else { 0 },
            1,
            cred.cr_uid.as_u32(),
            cred.cr_gid.as_u32(),
            Some(rdev as u32),
            if uses_extents {
                Some(extent_root_capacity(inode_size))
            } else {
                None
            },
        );
        let (new_ino, _) = fs.alloc_inode((self.ino as usize * 7) % groups)?;
        fs.write_inode(new_ino, &mut raw)?;
        fs.insert_dirent_with_growth(
            self.ino,
            &mut dir_raw,
            &mut data,
            name,
            new_ino,
            ftype_of_mode(raw.mode),
        )?;
        fs.node_for(new_ino)
    }

    fn symlink(&self, name: &str, target: &str) -> Result<Arc<INode>> {
        let fs = &self.fs;
        let _g = fs.ops_lock.lock();
        if name.len() > 255 {
            return Err(Errno::ENAMETOOLONG);
        }
        let mut dir_raw = fs.read_inode(self.ino)?;
        if !dir_raw.is_dir() {
            return Err(Errno::ENOTDIR);
        }
        let mut data = fs.read_dir_data(self.ino, &dir_raw)?;
        if dir::find(&dir::parse_all(&data, fs.block_size), name).is_some() {
            return Err(Errno::EEXIST);
        }
        let cred = crate::fs::vfs::perms::current_cred();
        let (inode_size, uses_extents, groups) = {
            let sb = fs.sb.lock();
            (
                sb.inode_size as usize,
                sb.uses_extents(),
                fs.bgt.lock().count.max(1),
            )
        };
        let mut raw = encode_new_inode(
            inode_size,
            0o120777u16,
            if uses_extents { iflags::EXTENTS } else { 0 },
            1,
            cred.cr_uid.as_u32(),
            cred.cr_gid.as_u32(),
            None,
            if uses_extents {
                Some(extent_root_capacity(inode_size))
            } else {
                None
            },
        );
        let (new_ino, _) = fs.alloc_inode((self.ino as usize * 7) % groups)?;
        if target.len() <= 60 {
            // Fast symlink: target lives inline in the i_block area.
            raw.raw[40..40 + target.len()].copy_from_slice(target.as_bytes());
            raw.set_size(target.len() as u64);
            fs.write_inode(new_ino, &mut raw)?;
        } else {
            fs.file_write(new_ino, &mut raw, 0, target.as_bytes())?;
        }
        fs.insert_dirent_with_growth(
            self.ino,
            &mut dir_raw,
            &mut data,
            name,
            new_ino,
            ftype::SYMLINK,
        )?;
        fs.node_for(new_ino)
    }

    fn readlink(&self) -> Result<String> {
        let fs = &self.fs;
        let raw = fs.read_inode(self.ino)?;
        if !raw.is_symlink() {
            return Err(Errno::EINVAL);
        }
        let len = raw.size().min(usize::MAX as u64) as usize;
        let ea_blocks = if raw.file_acl_lo != 0 {
            (fs.block_size / 512) as u64
        } else {
            0
        };
        if len <= 60 && raw.blocks().saturating_sub(ea_blocks) == 0 {
            let bytes = raw.raw[40..40 + len].to_vec();
            return Ok(String::from_utf8_lossy(&bytes).into_owned());
        }
        let mut buf = vec![0u8; len];
        let n = fs.file_read(self.ino, &raw, 0, &mut buf)?;
        Ok(String::from_utf8_lossy(&buf[..n]).into_owned())
    }

    /// Moves `old_name` to `new_name`, possibly across directories.
    ///
    /// Replacing destinations are freed according to their type. Moving a
    /// directory fixes its `..` reference and adjusts both parent link
    /// counts. Same-directory and same-inode no-ops are handled.
    fn rename(&self, old_name: &str, new_dir: &Arc<INode>, new_name: &str) -> Result<()> {
        let fs = &self.fs;
        let _g = fs.ops_lock.lock();
        if old_name == "." || old_name == ".." || new_name == "." || new_name == ".." {
            return Err(Errno::EINVAL);
        }
        if new_name.len() > 255 {
            return Err(Errno::ENAMETOOLONG);
        }
        if new_dir.node_ops.fsid() != Some(fs.fsid) {
            return Err(Errno::EXDEV);
        }
        let new_dir_ino = new_dir.id as u32;
        let same_dir = new_dir_ino == self.ino;

        // Read both directory contents.
        let mut dir_raw = fs.read_inode(self.ino)?;
        let mut old_data = fs.read_dir_data(self.ino, &dir_raw)?;
        let old_entries = dir::parse_all(&old_data, fs.block_size);
        let (target_ino, _target_ty) = dir::find(&old_entries, old_name).ok_or(Errno::ENOENT)?;

        // Existing destination handling.
        let mut new_raw_opt = if same_dir {
            None
        } else {
            Some(fs.read_inode(new_dir_ino)?)
        };
        let mut new_data_opt = match &new_raw_opt {
            Some(r) => Some(fs.read_dir_data(new_dir_ino, r)?),
            None => None,
        };
        let target_of_replace: Option<(u32, u8)> = if same_dir {
            dir::find(&old_entries, new_name)
        } else {
            dir::find(
                &dir::parse_all(new_data_opt.as_ref().unwrap(), fs.block_size),
                new_name,
            )
        };

        if let Some((dst_ino, dst_ty)) = target_of_replace {
            let dst_raw = fs.read_inode(dst_ino);
            if let Ok(mut d_raw) = dst_raw {
                if dst_ty == ftype::DIR || d_raw.is_dir() {
                    // Replaced directory must be empty, then removed.
                    let c_data = fs.read_dir_data(dst_ino, &d_raw)?;
                    let c_entries = dir::parse_all(&c_data, fs.block_size);
                    if c_entries
                        .iter()
                        .any(|e| e.ino != 0 && e.name != "." && e.name != "..")
                    {
                        return Err(Errno::ENOTEMPTY);
                    }
                    fs.file_truncate(dst_ino, &mut d_raw, 0)?;
                    d_raw.dtime = nowsecs32();
                    d_raw.raw[20..24].copy_from_slice(&d_raw.dtime.to_le_bytes());
                    fs.write_inode(dst_ino, &mut d_raw)?;
                    fs.free_inode(dst_ino)?;
                    // New dir loses the replaced subdir's `..` reference.
                    if !same_dir {
                        if let Some(nr) = new_raw_opt.as_mut() {
                            nr.links_count = nr.links_count.saturating_sub(1);
                            nr.raw[26..28].copy_from_slice(&nr.links_count.to_le_bytes());
                        }
                    }
                    dir_raw.links_count = dir_raw.links_count.saturating_sub(1);
                    dir_raw.raw[26..28].copy_from_slice(&dir_raw.links_count.to_le_bytes());
                } else {
                    // Non-directory replacement: decrement links, free at 0.
                    d_raw.links_count = d_raw.links_count.saturating_sub(1);
                    if d_raw.links_count == 0 {
                        d_raw.dtime = nowsecs32();
                        d_raw.raw[20..24].copy_from_slice(&d_raw.dtime.to_le_bytes());
                        d_raw.raw[26..28].copy_from_slice(&0u16.to_le_bytes());
                        fs.file_truncate(dst_ino, &mut d_raw, 0)?;
                        fs.free_inode(dst_ino)?;
                    } else {
                        fs.write_inode(dst_ino, &mut d_raw)?;
                    }
                }
            }
        }

        // Drop the old entry; insert under the new name.
        dir::delete_entry(&mut old_data, &old_entries, old_name)?;
        if same_dir {
            fs.insert_dirent_with_growth(
                self.ino,
                &mut dir_raw,
                &mut old_data,
                new_name,
                target_ino,
                _target_ty,
            )?;
        } else {
            fs.insert_dirent_with_growth(
                new_dir_ino,
                new_raw_opt.as_mut().unwrap(),
                new_data_opt.as_mut().unwrap(),
                new_name,
                target_ino,
                _target_ty,
            )?;
        }

        // Directory target: reparent `..` and adjust the parents' link counts.
        let t_raw = fs.read_inode(target_ino)?;
        if t_raw.is_dir() {
            fs.dir_set_dotdot(target_ino, new_dir_ino)?;
            if !same_dir {
                dir_raw.links_count = dir_raw.links_count.saturating_sub(1);
                dir_raw.raw[26..28].copy_from_slice(&dir_raw.links_count.to_le_bytes());
                let ndraw = new_raw_opt.as_mut().unwrap();
                ndraw.links_count += 1;
                ndraw.raw[26..28].copy_from_slice(&ndraw.links_count.to_le_bytes());
            }
        }

        // Persist both directories.
        fs.write_dir_data(self.ino, &mut dir_raw, &mut old_data)?;
        if !same_dir {
            let ndraw = new_raw_opt.as_mut().unwrap();
            let ndata = new_data_opt.as_mut().unwrap();
            fs.write_dir_data(new_dir_ino, ndraw, ndata)?;
        }
        Ok(())
    }

    fn open(&self, _flags: OpenFlags) -> Result<Box<dyn FileOps>> {
        let fs = &self.fs;
        let raw = fs.read_inode(self.ino)?;
        if raw.is_dir() {
            Ok(Box::new(Ext4DirFileOps {
                fs: Arc::clone(&self.fs),
                ino: self.ino,
            }))
        } else if raw.is_regular() {
            Ok(Box::new(Ext4FileOps {
                fs: Arc::clone(&self.fs),
                ino: self.ino,
            }))
        } else {
            Err(Errno::ENOSYS)
        }
    }

    fn getattr(&self) -> Result<INodeAttr> {
        let fs = &self.fs;
        let raw = fs.read_inode(self.ino)?;
        Ok(INodeAttr {
            size: usize::try_from(raw.size()).unwrap_or(usize::MAX),
            mode: Mode::from_bits_truncate(raw.mode as u32),
            nlink: raw.links_count as usize,
            uid: raw.uid,
            gid: raw.gid,
            rdev: raw.block[0] as u64,
        })
    }

    fn setattr(&self, attr: &INodeAttr) -> Result<()> {
        let fs = &self.fs;
        let _g = fs.ops_lock.lock();
        let mut raw = fs.read_inode(self.ino)?;
        // chown / chmod.
        if attr.uid != raw.uid || attr.gid != raw.gid {
            raw.uid = attr.uid;
            raw.gid = attr.gid;
            raw.raw[2..4].copy_from_slice(&(attr.uid as u16).to_le_bytes());
            raw.raw[24..26].copy_from_slice(&(attr.gid as u16).to_le_bytes());
            if raw.raw.len() >= 128 {
                raw.raw[120..124].copy_from_slice(&attr.uid.to_le_bytes());
                raw.raw[122..126].copy_from_slice(&attr.gid.to_le_bytes());
            }
        }
        let new_mode = attr.mode.bits() as u16;
        if (attr.mode.bits() & S_IFMT_BITS) == 0 {
            // Permission-only change: keep the type bits.
            raw.mode = (raw.mode & 0o170000) | (new_mode & !0o170000);
        } else {
            raw.mode = new_mode;
        }
        raw.raw[0..2].copy_from_slice(&raw.mode.to_le_bytes());
        // Truncation (regular files only; fs::truncate guards the type).
        let want = attr.size as u64;
        if raw.is_regular() && want != raw.size() {
            fs.file_truncate(self.ino, &mut raw, want)?;
        }
        raw.ctime = nowsecs32();
        fs.write_inode(self.ino, &mut raw)
    }
}

const S_IFMT_BITS: u32 = 0o170000;

fn ftype_of_mode(mode: u16) -> u8 {
    match mode & 0o170000 {
        0o040000 => ftype::DIR,
        0o100000 => ftype::REGULAR,
        0o120000 => ftype::SYMLINK,
        0o020000 => ftype::CHR,
        0o060000 => ftype::BLK,
        0o010000 => ftype::FIFO,
        _ => ftype::SOCK,
    }
}

// ---------------------------------------------------------------------------
// VFS file operations
// ---------------------------------------------------------------------------

/// Regular-file operations against on-disk state.
pub struct Ext4FileOps {
    pub fs: Arc<Ext4Fs>,
    pub ino: u32,
}

impl FileOps for Ext4FileOps {
    fn read(&self, offset: u64, buf: &mut [u8]) -> Result<usize> {
        let fs = &self.fs;
        let _g = fs.ops_lock.lock();
        let raw = fs.read_inode(self.ino)?;
        fs.file_read(self.ino, &raw, offset, buf)
    }

    fn write(&self, offset: u64, buf: &[u8]) -> Result<usize> {
        let fs = &self.fs;
        let _g = fs.ops_lock.lock();
        let mut raw = fs.read_inode(self.ino)?;
        fs.file_write(self.ino, &mut raw, offset, buf)
    }

    fn seek(&self, curr_offset: u64, anchor: SeekAnchor) -> Result<u64> {
        let fs = &self.fs;
        let raw = fs.read_inode(self.ino)?;
        let size = raw.size() as i64;
        let target = match anchor {
            SeekAnchor::Start(o) => o as i64,
            SeekAnchor::Current(d) => curr_offset as i64 + d,
            SeekAnchor::End(d) => size + d,
        };
        Ok(target.max(0) as u64)
    }

    fn mmap(&self, _offset: u64, _len: usize) -> Result<()> {
        Ok(())
    }

    fn fsync(&self, _datasync: bool) -> Result<()> {
        self.fs.sync_all()
    }
}

/// Directory open-file operations.
pub struct Ext4DirFileOps {
    pub fs: Arc<Ext4Fs>,
    pub ino: u32,
}

impl FileOps for Ext4DirFileOps {
    fn read(&self, _offset: u64, _buf: &mut [u8]) -> Result<usize> {
        Err(Errno::EISDIR)
    }

    fn write(&self, _offset: u64, _buf: &[u8]) -> Result<usize> {
        Err(Errno::EISDIR)
    }

    fn readdir(&self) -> Result<Vec<u8>> {
        let fs = &self.fs;
        let raw = fs.read_inode(self.ino)?;
        let data = fs.read_dir_data(self.ino, &raw)?;
        let entries = dir::parse_all(&data, fs.block_size);
        let mut out = Vec::new();
        for e in dir::listing(&entries) {
            out.extend_from_slice(e.0.as_bytes());
            out.push(b'\n');
        }
        Ok(out)
    }

    fn seek(&self, curr_offset: u64, anchor: SeekAnchor) -> Result<u64> {
        let fs = &self.fs;
        let raw = fs.read_inode(self.ino)?;
        let size = raw.size() as i64;
        let target = match anchor {
            SeekAnchor::Start(o) => o as i64,
            SeekAnchor::Current(d) => curr_offset as i64 + d,
            SeekAnchor::End(d) => size + d,
        };
        Ok(target.max(0) as u64)
    }
}

// ---------------------------------------------------------------------------
// FileSystem trait
// ---------------------------------------------------------------------------

impl FileSystem for Ext4Fs {
    fn name(&self) -> &'static str {
        "ext4"
    }

    fn root(&self) -> Result<Arc<INode>> {
        self.node_for(ROOT_INO)
    }

    fn statfs(&self) -> Result<StatFs> {
        Ok(self.statfs_snapshot())
    }

    fn sync(&self) -> Result<()> {
        self.sync_all()
    }
}
