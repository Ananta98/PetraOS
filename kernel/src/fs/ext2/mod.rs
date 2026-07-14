pub mod bitmap;
pub mod ondisk;
pub mod structs;
pub mod vfs;

pub use ondisk::Ext2FsState;
pub use structs::{GroupDescriptor, Inode, Superblock};
pub use vfs::{Ext2File, Ext2Fs, Ext2Inode};

use crate::drivers::block::BlockDevice;
use crate::fs::vfs::Result;
use ondisk::write_blocks;
use structs::{EXT2_S_IFDIR, GroupDescriptor as Gd, Inode as In, Superblock as Sb};

/// EXT2 Formatter Utility
///
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

    // Layout layout:
    // Block 0: Boot record / padding
    // Block 1: Superblock
    // Block 2: Group Descriptor Table (GDT)
    // Block 3: Block bitmap
    // Block 4: Inode bitmap
    // Block 5..132: Inode Table (128 blocks, 1024 inodes * 128 bytes = 131,072 bytes)
    // Block 133: Root directory data block
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
        s_state: 1, // Valid
        s_errors: 1,
        s_minor_rev_level: 0,
        s_rev_level: 0, // Revision 0 (legacy)
        s_first_ino: 11,
        s_inode_size: 128,
    };

    // Serialize and write superblock to block 1 (offset 1024 bytes)
    let mut sb_buf = [0u8; 1024];
    sb.serialize(&mut sb_buf);
    write_blocks(block_dev, block_size, 1, &sb_buf)?;

    // Create and write GDT
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

    // Block Bitmap (Block 3): Blocks 0..133 are used (134 blocks)
    let mut block_bitmap_buf = [0u8; 1024];
    // 134 bits used: 16 bytes of 0xFF (128 bits) + 1 byte of 0x3F (6 bits)
    for i in 0..16 {
        block_bitmap_buf[i] = 0xFF;
    }
    block_bitmap_buf[16] = 0x3F;
    write_blocks(block_dev, block_size, block_bitmap, &block_bitmap_buf)?;

    // Inode Bitmap (Block 4): Inodes 1..10 are used (10 inodes)
    let mut inode_bitmap_buf = [0u8; 1024];
    // 10 bits used: 1 byte of 0xFF (8 bits) + 1 byte of 0x03 (2 bits)
    inode_bitmap_buf[0] = 0xFF;
    inode_bitmap_buf[1] = 0x03;
    write_blocks(block_dev, block_size, inode_bitmap, &inode_bitmap_buf)?;

    // Inode Table (Blocks 5..132): Inode 2 is root directory
    let zeros = [0u8; 1024];
    for b in inode_table..root_dir_block {
        write_blocks(block_dev, block_size, b, &zeros)?;
    }

    // Root Inode (Inode 2 is index 1 inside Block 5)
    let mut root_inode = In {
        i_mode: EXT2_S_IFDIR | 0o755,
        i_uid: 0,
        i_size: block_size,
        i_atime: 0,
        i_ctime: 0,
        i_mtime: 0,
        i_dtime: 0,
        i_gid: 0,
        i_links_count: 2,
        i_blocks: sectors_per_block as u32, // sectors count
        i_flags: 0,
        i_block: [0; 15],
    };
    root_inode.i_block[0] = root_dir_block;

    let mut inode_buf = [0u8; 128];
    root_inode.serialize(&mut inode_buf);

    let mut block_5_buf = [0u8; 1024];
    // Inode 2 offset = 1 * 128 = 128
    block_5_buf[128..256].copy_from_slice(&inode_buf);
    write_blocks(block_dev, block_size, inode_table, &block_5_buf)?;

    // Root directory block (Block 133)
    let mut root_dir_buf = [0u8; 1024];
    // "." entry pointing to inode 2
    root_dir_buf[0..4].copy_from_slice(&2u32.to_le_bytes()); // inode = 2
    root_dir_buf[4..6].copy_from_slice(&(12u16).to_le_bytes()); // rec_len = 12
    root_dir_buf[6] = 1; // name_len
    root_dir_buf[7] = 2; // file_type (DIR)
    root_dir_buf[8..9].copy_from_slice(b".");

    // ".." entry pointing to inode 2 (occupies the remainder of the block)
    let remaining = block_size - 12;
    root_dir_buf[12..16].copy_from_slice(&2u32.to_le_bytes()); // inode = 2
    root_dir_buf[16..18].copy_from_slice(&(remaining as u16).to_le_bytes()); // rec_len
    root_dir_buf[18] = 2; // name_len
    root_dir_buf[19] = 2; // file_type (DIR)
    root_dir_buf[20..22].copy_from_slice(b"..");

    write_blocks(block_dev, block_size, root_dir_block, &root_dir_buf)?;

    Ok(())
}

// =============================================================================
// Unit Tests Block
// =============================================================================

#[cfg(ktest)]
pub mod tests {
    use super::*;
    use crate::drivers::block::BlockDevice;
    use crate::fs::vfs::{
        FileOps, FileType, init_root_fs, mount, register_filesystem, resolve_path,
    };
    use alloc::sync::Arc;
    use ostd::Error;
    use ostd::prelude::ktest;
    use ostd::sync::SpinLock;

    struct MockBlock {
        data: SpinLock<alloc::vec::Vec<u8>>,
    }

    impl BlockDevice for MockBlock {
        fn block_size(&self) -> usize {
            512
        }

        fn num_blocks(&self) -> usize {
            4096 // 2MB disk
        }

        fn read_blocks(
            &self,
            block_id: usize,
            buf: &mut [u8],
        ) -> core::result::Result<(), ostd::Error> {
            if block_id >= 4096 {
                return Err(Error::InvalidArgs);
            }
            let guard = self.data.lock();
            let offset = block_id * 512;
            buf[..512].copy_from_slice(&guard[offset..offset + 512]);
            Ok(())
        }

        fn write_blocks(
            &self,
            block_id: usize,
            buf: &[u8],
        ) -> core::result::Result<(), ostd::Error> {
            if block_id >= 4096 {
                return Err(Error::InvalidArgs);
            }
            let mut guard = self.data.lock();
            let offset = block_id * 512;
            guard[offset..offset + 512].copy_from_slice(&buf[..512]);
            Ok(())
        }
    }

    #[ktest]
    fn test_ext2_filesystem() {
        let ramfs = Arc::new(crate::fs::ramfs::RamFs);
        let _ = register_filesystem(ramfs);
        let _ = init_root_fs("ramfs", &[]);

        let devfs = Arc::new(crate::fs::devfs::DevFs);
        let _ = register_filesystem(devfs);

        let root = crate::fs::vfs::ROOT_DENTRY
            .lock()
            .as_ref()
            .cloned()
            .unwrap();
        root.inode.mkdir("dev", 0o755).unwrap();
        mount("devfs", "/dev", 0, &[]).unwrap();

        // Create mock block device
        let mock_block = Arc::new(MockBlock {
            data: SpinLock::new(alloc::vec![0u8; 4096 * 512]),
        });
        // Format it as EXT2
        format_ext2(&*mock_block).unwrap();

        // Register the mock block device in devfs
        crate::drivers::register_block_device("mock-ext2-disk", mock_block.clone()).unwrap();

        // Register the ext2 filesystem driver
        let ext2_fs = Arc::new(Ext2Fs);
        register_filesystem(ext2_fs).unwrap();

        // Create mountpoint directory
        root.inode.mkdir("mnt", 0o755).unwrap();

        // Mount ext2 at /mnt using the block device path
        mount("ext2", "/mnt", 0, b"/dev/mock-ext2-disk").unwrap();

        // Resolve /mnt
        let mnt_dentry = resolve_path("/mnt").unwrap();
        assert_eq!(
            mnt_dentry.inode.metadata().unwrap().file_type,
            FileType::Directory
        );

        // 1. Create a file /mnt/hello.txt
        let file_inode = mnt_dentry.inode.create("hello.txt", 0o644).unwrap();
        let mut file_ops = file_inode.open(0).unwrap();

        // 2. Write data to /mnt/hello.txt
        let mut offset = 0;
        file_ops
            .write(b"ext2 driver is fully functional!", &mut offset)
            .unwrap();

        // 3. Read data from /mnt/hello.txt
        let mut read_buf = [0u8; 32];
        let mut read_offset = 0;
        file_ops.read(&mut read_buf, &mut read_offset).unwrap();
        assert_eq!(&read_buf, b"ext2 driver is fully functional!");

        // 4. Create a directory /mnt/mydir
        let mydir_inode = mnt_dentry.inode.mkdir("mydir", 0o755).unwrap();

        // 5. Create a file /mnt/mydir/foo.txt
        let child_file = mydir_inode.create("foo.txt", 0o644).unwrap();
        let mut child_ops = child_file.open(0).unwrap();
        let mut child_offset = 0;
        child_ops.write(b"hello world", &mut child_offset).unwrap();

        let mut child_read_buf = [0u8; 11];
        let mut child_read_offset = 0;
        child_ops
            .read(&mut child_read_buf, &mut child_read_offset)
            .unwrap();
        assert_eq!(&child_read_buf, b"hello world");

        // 6. Delete a file /mnt/hello.txt
        mnt_dentry.inode.unlink("hello.txt").unwrap();

        // Verify hello.txt is deleted
        assert!(mnt_dentry.inode.lookup("hello.txt").is_err());

        // Cleanup
        crate::drivers::unregister_device("mock-ext2-disk").unwrap();
        crate::fs::vfs::unregister_filesystem("ext2").unwrap();
        crate::fs::vfs::unregister_filesystem("devfs").unwrap();
        crate::fs::vfs::unregister_filesystem("ramfs").unwrap();
        *crate::fs::vfs::ROOT_DENTRY.lock() = None;
        *crate::fs::vfs::CWD_DENTRY.lock() = None;
        crate::fs::vfs::DENTRY_CACHE.clear();
    }
}
