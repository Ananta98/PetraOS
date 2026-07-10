use alloc::string::String;

#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct BootSector {
    pub jump_boot: [u8; 3],
    pub fs_name: [u8; 8], // "EXFAT   "
    pub must_be_zero: [u8; 53],
    pub partition_offset: u64,
    pub volume_length: u64,
    pub fat_offset: u32,
    pub fat_length: u32,
    pub cluster_heap_offset: u32,
    pub cluster_count: u32,
    pub first_cluster_of_root: u32,
    pub volume_guid: [u8; 16],
    pub fs_revision: u16,
    pub flags: u16,
    pub bytes_per_sector_shift: u8,
    pub sectors_per_cluster_shift: u8,
    pub number_of_fats: u8,
    pub drive_select: u8,
    pub percent_in_use: u8,
    pub reserved: [u8; 7],
    pub boot_code: [u8; 378],
    pub signature: u16, // 0xAA55
}

#[derive(Debug, Clone)]
pub struct ExFatFileInfo {
    pub name: String,
    pub file_attributes: u16,
    pub first_cluster: u32,
    pub size: u64,
    pub is_dir: bool,
    pub no_fat_chain: bool,
    pub entry_cluster: u32,
    pub entry_offset_in_dir: usize,
    pub entry_count: usize,
}
