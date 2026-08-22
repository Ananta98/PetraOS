use super::inode::Ext2Volume;
use super::superblock::Ext2BlockGroupDescriptor;
use crate::fs::vfs::types::VfsError;

/// Ext2 Bitmap manipulation and physical block/inode allocator.
///
/// NOTE: The current implementation targets single block group (group 0) volumes,
/// which is typical for standard boot/init storage. Multi-group scaling is structured
/// via `with_block_group`.
pub struct Ext2Bitmap;

impl Ext2Bitmap {
    /// Read a specific bit state (true if set / allocated, false if free).
    pub fn is_bit_set(bitmap: &[u8], bit_index: usize) -> bool {
        let byte_idx = bit_index / 8;
        let bit_off = bit_index % 8;
        if byte_idx >= bitmap.len() {
            return false;
        }
        (bitmap[byte_idx] & (1 << bit_off)) != 0
    }

    /// Set a bit (mark as allocated). Returns `true` if it was previously free.
    pub fn set_bit(bitmap: &mut [u8], bit_index: usize) -> bool {
        let byte_idx = bit_index / 8;
        let bit_off = bit_index % 8;
        if byte_idx >= bitmap.len() {
            return false;
        }
        let was_set = (bitmap[byte_idx] & (1 << bit_off)) != 0;
        bitmap[byte_idx] |= 1 << bit_off;
        !was_set
    }

    /// Clear a bit (mark as free). Returns `true` if it was previously set.
    pub fn clear_bit(bitmap: &mut [u8], bit_index: usize) -> bool {
        let byte_idx = bit_index / 8;
        let bit_off = bit_index % 8;
        if byte_idx >= bitmap.len() {
            return false;
        }
        let was_set = (bitmap[byte_idx] & (1 << bit_off)) != 0;
        bitmap[byte_idx] &= !(1 << bit_off);
        was_set
    }

    /// Find the first free (0) bit in `bitmap`, set it to 1, and return its 0-based index.
    pub fn alloc_free_bit(bitmap: &mut [u8], max_bits: usize) -> Result<usize, VfsError> {
        let limit = core::cmp::min(max_bits, bitmap.len() * 8);
        for bit_idx in 0..limit {
            let byte_idx = bit_idx / 8;
            let bit_off = bit_idx % 8;
            if (bitmap[byte_idx] & (1 << bit_off)) == 0 {
                bitmap[byte_idx] |= 1 << bit_off;
                return Ok(bit_idx);
            }
        }
        Err(VfsError::NotSupported)
    }

    /// Helper to read, mutate, and write back a Block Group Descriptor table entry.
    fn with_block_group<F, R>(volume: &Ext2Volume, group: u32, f: F) -> Result<R, VfsError>
    where
        F: FnOnce(&mut Ext2BlockGroupDescriptor) -> Result<R, VfsError>,
    {
        let bg_desc_table_block = if volume.sb.block_size == 1024 { 2 } else { 1 };
        let bg_desc_offset =
            (bg_desc_table_block * volume.sb.block_size) as u64 + (group as u64 * 32);

        let mut bg_buf = alloc::vec![0u8; 32];
        volume.reader.read_bytes(bg_desc_offset, &mut bg_buf)?;
        let mut bg = Ext2BlockGroupDescriptor::parse(&bg_buf).ok_or(VfsError::InvalidInput)?;

        let result = f(&mut bg)?;

        let bg_bytes = bg.serialize();
        volume.reader.write_bytes(bg_desc_offset, &bg_bytes)?;

        Ok(result)
    }

    /// Allocate a fresh physical block from the filesystem block bitmap.
    pub fn alloc_block(volume: &Ext2Volume) -> Result<u32, VfsError> {
        let block_size = volume.sb.block_size as u64;

        Self::with_block_group(volume, 0, |bg| {
            if bg.free_blocks_count == 0 {
                return Err(VfsError::NotSupported);
            }

            let bitmap_offset = bg.block_bitmap as u64 * block_size;
            let mut bitmap_buf = alloc::vec![0u8; block_size as usize];
            volume.reader.read_bytes(bitmap_offset, &mut bitmap_buf)?;

            let bit_idx =
                Self::alloc_free_bit(&mut bitmap_buf, volume.sb.blocks_per_group as usize)?;
            let physical_block = volume.sb.first_data_block + bit_idx as u32;

            volume.reader.write_bytes(bitmap_offset, &bitmap_buf)?;
            bg.free_blocks_count -= 1;

            // Zero out the newly allocated physical block
            let zero_buf = alloc::vec![0u8; block_size as usize];
            volume
                .reader
                .write_bytes(physical_block as u64 * block_size, &zero_buf)?;

            Ok(physical_block)
        })
    }

    /// Free an allocated physical block back to the block bitmap.
    pub fn free_block(volume: &Ext2Volume, block_id: u32) -> Result<(), VfsError> {
        if block_id < volume.sb.first_data_block {
            return Ok(());
        }

        let block_size = volume.sb.block_size as u64;

        Self::with_block_group(volume, 0, |bg| {
            let bitmap_offset = bg.block_bitmap as u64 * block_size;
            let mut bitmap_buf = alloc::vec![0u8; block_size as usize];
            volume.reader.read_bytes(bitmap_offset, &mut bitmap_buf)?;

            let bit_idx = (block_id - volume.sb.first_data_block) as usize;
            if Self::clear_bit(&mut bitmap_buf, bit_idx) {
                volume.reader.write_bytes(bitmap_offset, &bitmap_buf)?;
                bg.free_blocks_count += 1;
            }

            Ok(())
        })
    }

    /// Allocate a fresh 1-based inode index from the filesystem inode bitmap.
    pub fn alloc_inode(volume: &Ext2Volume) -> Result<u32, VfsError> {
        let block_size = volume.sb.block_size as u64;

        Self::with_block_group(volume, 0, |bg| {
            if bg.free_inodes_count == 0 {
                return Err(VfsError::NotSupported);
            }

            let bitmap_offset = bg.inode_bitmap as u64 * block_size;
            let mut bitmap_buf = alloc::vec![0u8; block_size as usize];
            volume.reader.read_bytes(bitmap_offset, &mut bitmap_buf)?;

            let bit_idx =
                Self::alloc_free_bit(&mut bitmap_buf, volume.sb.inodes_per_group as usize)?;
            let ino = bit_idx as u32 + 1;

            volume.reader.write_bytes(bitmap_offset, &bitmap_buf)?;
            bg.free_inodes_count -= 1;

            Ok(ino)
        })
    }

    /// Free an allocated inode index back to the inode bitmap.
    pub fn free_inode(volume: &Ext2Volume, ino: u32) -> Result<(), VfsError> {
        if ino == 0 {
            return Ok(());
        }

        let block_size = volume.sb.block_size as u64;

        Self::with_block_group(volume, 0, |bg| {
            let bitmap_offset = bg.inode_bitmap as u64 * block_size;
            let mut bitmap_buf = alloc::vec![0u8; block_size as usize];
            volume.reader.read_bytes(bitmap_offset, &mut bitmap_buf)?;

            let bit_idx = (ino - 1) as usize;
            if Self::clear_bit(&mut bitmap_buf, bit_idx) {
                volume.reader.write_bytes(bitmap_offset, &bitmap_buf)?;
                bg.free_inodes_count += 1;
            }

            Ok(())
        })
    }
}
