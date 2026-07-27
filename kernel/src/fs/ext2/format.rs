//! EXT2 Filesystem Formatter (`mkfs.ext2`).

use super::layout::{EXT2_S_IFDIR, GroupDescriptor as Gd, Inode as In, Superblock as Sb};
use super::superblock::write_blocks;
use crate::drivers::block::BlockDevice;
use crate::fs::vfs::Result;

/// Formats a block device as a valid EXT2 filesystem with a 1024-byte block size,
/// a primary superblock, a single block group descriptor, bitmap blocks, an inode table,
/// and a pre-allocated root directory (inode 2).
pub fn format_ext2(block_dev: &dyn BlockDevice) -> Result<()> {
    let block_size = 1024;
    let sectors_per_block = 2; // 1024 / 512

    let total_blocks = (block_dev.num_blocks() / sectors_per_block) as u32;

    let inodes_count = 1024;
    let blocks_per_group = 8192;
    let inodes_per_group = 1024;

    let gdt_start = 2;
    let block_bitmap = 3;
    let inode_bitmap = 4;
    let inode_table = 5;
    let root_dir_block = 133;

    let reserved_blocks = 134; // 0..133 are reserved
    let free_blocks = total_blocks.checked_sub(reserved_blocks).unwrap_or(0);
    let free_inodes = inodes_count - 10; // Inodes 1..10 are reserved. Inode 2 is root dir (used)

    let sb = Sb {
        s_inodes_count: inodes_count,
        s_blocks_count: total_blocks,
        s_r_blocks_count: 0,
        s_free_blocks_count: free_blocks,
        s_free_inodes_count: free_inodes,
        s_first_data_block: 1,
        s_log_block_size: 0, // 1024 bytes
        s_log_frag_size: 0,
        s_blocks_per_group: blocks_per_group,
        s_frags_per_group: blocks_per_group,
        s_inodes_per_group: inodes_per_group,
        s_magic: 0xEF53,
        s_state: 1,
        s_errors: 1,
        s_minor_rev_level: 0,
        s_rev_level: 0,
        s_first_ino: 11,
        s_inode_size: 128,
    };

    let mut sb_buf = [0u8; 1024];
    sb.serialize(&mut sb_buf);
    write_blocks(block_dev, block_size, 1, &sb_buf)?;

    let gd = Gd {
        bg_block_bitmap: block_bitmap,
        bg_inode_bitmap: inode_bitmap,
        bg_inode_table: inode_table,
        bg_free_blocks_count: free_blocks as u16,
        bg_free_inodes_count: free_inodes as u16,
        bg_used_dirs_count: 1, // root directory
    };

    let mut gd_buf = [0u8; 1024];
    gd.serialize(&mut gd_buf[0..32]);
    write_blocks(block_dev, block_size, gdt_start, &gd_buf)?;

    // Block Bitmap
    let mut block_bm_buf = alloc::vec![0u8; 1024];
    for i in 0..134 {
        let byte_idx = i / 8;
        let bit_idx = i % 8;
        block_bm_buf[byte_idx] |= 1 << bit_idx;
    }
    write_blocks(block_dev, block_size, block_bitmap, &block_bm_buf)?;

    // Inode Bitmap
    let mut inode_bm_buf = alloc::vec![0u8; 1024];
    for i in 0..10 {
        let byte_idx = i / 8;
        let bit_idx = i % 8;
        inode_bm_buf[byte_idx] |= 1 << bit_idx;
    }
    write_blocks(block_dev, block_size, inode_bitmap, &inode_bm_buf)?;

    // Inode Table
    let mut root_inode = In {
        i_mode: EXT2_S_IFDIR | 0o755,
        i_uid: 0,
        i_size: block_size,
        i_atime: 0,
        i_ctime: 0,
        i_mtime: 0,
        i_dtime: 0,
        i_gid: 0,
        i_links_count: 2, // "." and ".."
        i_blocks: 2,      // 2 sectors = 1024 bytes
        i_flags: 0,
        i_block: [0; 15],
    };
    root_inode.i_block[0] = root_dir_block;

    let mut inode_table_block0 = alloc::vec![0u8; 1024];
    root_inode.serialize(&mut inode_table_block0[128..256]);
    write_blocks(block_dev, block_size, inode_table, &inode_table_block0)?;

    // Zero remaining inode table blocks
    let zeros = alloc::vec![0u8; 1024];
    for b in (inode_table + 1)..133 {
        write_blocks(block_dev, block_size, b, &zeros)?;
    }

    // Root directory data block
    let mut root_dir_buf = alloc::vec![0u8; 1024];
    // "." entry
    root_dir_buf[0..4].copy_from_slice(&2u32.to_le_bytes()); // Inode 2
    root_dir_buf[4..6].copy_from_slice(&12u16.to_le_bytes()); // rec_len = 12
    root_dir_buf[6] = 1; // name_len
    root_dir_buf[7] = 2; // file_type (DIR)
    root_dir_buf[8..9].copy_from_slice(b".");

    // ".." entry
    root_dir_buf[12..16].copy_from_slice(&2u32.to_le_bytes()); // Inode 2 (root is its own parent)
    root_dir_buf[16..18].copy_from_slice(&1012u16.to_le_bytes()); // rec_len = 1024 - 12 = 1012
    root_dir_buf[18] = 2; // name_len
    root_dir_buf[19] = 2; // file_type (DIR)
    root_dir_buf[20..22].copy_from_slice(b"..");

    write_blocks(block_dev, block_size, root_dir_block, &root_dir_buf)?;
    Ok(())
}
