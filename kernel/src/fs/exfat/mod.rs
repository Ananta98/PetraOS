pub mod ondisk;
pub mod structs;
pub mod vfs;

pub use ondisk::ExFatFsState;
pub use structs::{BootSector, ExFatFileInfo};
pub use vfs::{ExFatFile, ExFatFs, ExFatInode};

use crate::drivers::block::BlockDevice;
use crate::fs::vfs::Result;
use ondisk::write_bytes;

/// exFAT Formatter Utility
///
/// Formats a block device with valid exFAT structures: Boot sector, FAT, Allocation Bitmap,
/// and Root directory containing default bitmap/upcase entries.
pub fn format_exfat(block_dev: &dyn BlockDevice) -> Result<()> {
    let sector_size = 512;
    let cluster_size = 4096;
    let sectors_per_cluster = cluster_size / sector_size;

    let total_sectors = block_dev.num_blocks() as u64;

    let fat_offset = 64;
    let fat_length = 64;
    let cluster_heap_offset = 128;

    let heap_sectors = total_sectors
        .checked_sub(cluster_heap_offset as u64)
        .unwrap_or(0);
    let cluster_count = (heap_sectors / sectors_per_cluster as u64) as u32;

    let first_cluster_of_root = 2;

    let boot = BootSector {
        jump_boot: [0xEB, 0x76, 0x90],
        fs_name: *b"EXFAT   ",
        must_be_zero: [0u8; 53],
        partition_offset: 0,
        volume_length: total_sectors,
        fat_offset,
        fat_length,
        cluster_heap_offset,
        cluster_count,
        first_cluster_of_root,
        volume_guid: [0u8; 16],
        fs_revision: 0x0100,
        flags: 0,
        bytes_per_sector_shift: 9,    // 512
        sectors_per_cluster_shift: 3, // 8
        number_of_fats: 1,
        drive_select: 0x80,
        percent_in_use: 0,
        reserved: [0u8; 7],
        boot_code: [0u8; 378],
        signature: 0xAA55,
    };

    // Serialize Boot Sector safely (no unsafe pointer cast)
    let mut boot_bytes = [0u8; 512];
    boot_bytes[0..3].copy_from_slice(&boot.jump_boot);
    boot_bytes[3..11].copy_from_slice(&boot.fs_name);
    boot_bytes[11..64].copy_from_slice(&boot.must_be_zero);
    boot_bytes[64..72].copy_from_slice(&boot.partition_offset.to_le_bytes());
    boot_bytes[72..80].copy_from_slice(&boot.volume_length.to_le_bytes());
    boot_bytes[80..84].copy_from_slice(&boot.fat_offset.to_le_bytes());
    boot_bytes[84..88].copy_from_slice(&boot.fat_length.to_le_bytes());
    boot_bytes[88..92].copy_from_slice(&boot.cluster_heap_offset.to_le_bytes());
    boot_bytes[92..96].copy_from_slice(&boot.cluster_count.to_le_bytes());
    boot_bytes[96..100].copy_from_slice(&boot.first_cluster_of_root.to_le_bytes());
    boot_bytes[100..116].copy_from_slice(&boot.volume_guid);
    boot_bytes[116..118].copy_from_slice(&boot.fs_revision.to_le_bytes());
    boot_bytes[118..120].copy_from_slice(&boot.flags.to_le_bytes());
    boot_bytes[120] = boot.bytes_per_sector_shift;
    boot_bytes[121] = boot.sectors_per_cluster_shift;
    boot_bytes[122] = boot.number_of_fats;
    boot_bytes[123] = boot.drive_select;
    boot_bytes[124] = boot.percent_in_use;
    boot_bytes[125..132].copy_from_slice(&boot.reserved);
    boot_bytes[132..510].copy_from_slice(&boot.boot_code);
    boot_bytes[510..512].copy_from_slice(&boot.signature.to_le_bytes());

    write_bytes(block_dev, 0, &boot_bytes)?;

    // Zero out the FAT
    let zeros = [0u8; 512];
    for sector in 0..fat_length {
        write_bytes(block_dev, (fat_offset as u64 + sector as u64) * 512, &zeros)?;
    }

    // Set FAT entry 0 and 1 (reserved)
    let fat_init = [0xF8, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF];
    write_bytes(block_dev, (fat_offset as u64) * 512, &fat_init)?;

    // Cluster 2 (Root) -> EOF
    // Cluster 3 (Bitmap) -> EOF
    // Cluster 4 (Upcase) -> EOF
    let entry = 0xFFFFFFFFu32.to_le_bytes();
    write_bytes(block_dev, (fat_offset as u64) * 512 + 2 * 4, &entry)?;
    write_bytes(block_dev, (fat_offset as u64) * 512 + 3 * 4, &entry)?;
    write_bytes(block_dev, (fat_offset as u64) * 512 + 4 * 4, &entry)?;

    // Initialize Allocation Bitmap: mark clusters 2, 3, 4 as used (byte 0 = 0x1C)
    let mut bitmap_data = [0u8; 512];
    bitmap_data[0] = 0x1C;
    let bitmap_sector = cluster_heap_offset as u64 + (3 - 2) * sectors_per_cluster as u64;
    write_bytes(block_dev, bitmap_sector * 512, &bitmap_data)?;

    // Initialize Root Directory with Allocation Bitmap and Upcase Table directory entries
    let root_sector = cluster_heap_offset as u64;

    let mut bitmap_entry = [0u8; 32];
    bitmap_entry[0] = 0x81;
    bitmap_entry[20..24].copy_from_slice(&3u32.to_le_bytes()); // first cluster = 3
    bitmap_entry[24..32].copy_from_slice(&(512u64).to_le_bytes()); // size = 512 bytes

    let mut upcase_entry = [0u8; 32];
    upcase_entry[0] = 0x82;
    upcase_entry[20..24].copy_from_slice(&4u32.to_le_bytes()); // first cluster = 4
    upcase_entry[24..32].copy_from_slice(&(512u64).to_le_bytes()); // size = 512 bytes

    let mut root_data = [0u8; 512];
    root_data[0..32].copy_from_slice(&bitmap_entry);
    root_data[32..64].copy_from_slice(&upcase_entry);
    write_bytes(block_dev, root_sector * 512, &root_data)?;

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
    fn test_exfat_filesystem() {
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
        // Format it as exFAT
        format_exfat(&*mock_block).unwrap();

        // Register the mock block device in devfs
        crate::drivers::register_block_device("mock-exfat-disk", mock_block.clone()).unwrap();

        // Register the exfat filesystem driver
        let exfat_fs = Arc::new(ExFatFs);
        register_filesystem(exfat_fs).unwrap();

        // Create mountpoint directory
        root.inode.mkdir("mnt", 0o755).unwrap();

        // Mount exfat at /mnt using the block device path
        mount("exfat", "/mnt", 0, b"/dev/mock-exfat-disk").unwrap();

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
            .write(b"exfat driver is fully functional!", &mut offset)
            .unwrap();

        // 3. Read data from /mnt/hello.txt
        let mut read_buf = [0u8; 33];
        let mut read_offset = 0;
        file_ops.read(&mut read_buf, &mut read_offset).unwrap();
        assert_eq!(&read_buf, b"exfat driver is fully functional!");

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
        crate::drivers::unregister_device("mock-exfat-disk").unwrap();
        crate::fs::vfs::unregister_filesystem("exfat").unwrap();
        crate::fs::vfs::unregister_filesystem("devfs").unwrap();
        crate::fs::vfs::unregister_filesystem("ramfs").unwrap();
        *crate::fs::vfs::ROOT_DENTRY.lock() = None;
        *crate::fs::vfs::CWD_DENTRY.lock() = None;
        crate::fs::vfs::DENTRY_CACHE.clear();
    }
}
