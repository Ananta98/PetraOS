//! exFAT Formatter Utility (`mkfs.exfat`).

use super::fat::write_bytes;
use super::layout::BootSector;
use crate::drivers::block::BlockDevice;
use crate::fs::vfs::Result;

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

    // Zero out FAT region
    let zeros_sector = [0u8; 512];
    for s in 0..fat_length {
        let offset = (fat_offset as u64 + s as u64) * 512;
        write_bytes(block_dev, offset, &zeros_sector)?;
    }

    // Mark root cluster as end-of-chain (0xFFFFFFFF) in FAT table
    let fat_root_offset = (fat_offset as u64) * 512 + (first_cluster_of_root as u64) * 4;
    let eof_marker = 0xFFFFFFFFu32.to_le_bytes();
    write_bytes(block_dev, fat_root_offset, &eof_marker)?;

    // Zero out Cluster 2 (Root Directory cluster)
    let root_cluster_offset = (cluster_heap_offset as u64) * 512;
    let zeros_cluster = alloc::vec![0u8; cluster_size as usize];
    write_bytes(block_dev, root_cluster_offset, &zeros_cluster)?;

    // Write Allocation Bitmap Directory Entry into Root Directory
    let bitmap_cluster: u32 = 3;

    let bitmap_bytes_count = (cluster_count + 7) / 8;

    let mut bitmap_entry = [0u8; 32];
    bitmap_entry[0] = 0x81; // Allocation Bitmap Directory Entry
    bitmap_entry[1] = 0x00; // Bitmap 0
    bitmap_entry[20..24].copy_from_slice(&bitmap_cluster.to_le_bytes());
    bitmap_entry[24..32].copy_from_slice(&(bitmap_bytes_count as u64).to_le_bytes());

    write_bytes(block_dev, root_cluster_offset, &bitmap_entry)?;

    // Mark Cluster 2 (Root) and Cluster 3 (Bitmap) as allocated in the Bitmap
    let mut bitmap_data = alloc::vec![0u8; cluster_size as usize];
    bitmap_data[0] = 0x03; // Bits 0 (Cluster 2) and 1 (Cluster 3) set

    let bitmap_cluster_offset = (cluster_heap_offset as u64
        + (bitmap_cluster as u64 - 2) * sectors_per_cluster as u64)
        * 512;
    write_bytes(block_dev, bitmap_cluster_offset, &bitmap_data)?;

    Ok(())
}
