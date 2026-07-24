//! exFAT Directory Entry Set Parsing & Management (`dir.rs`).

use alloc::string::String;
use alloc::vec::Vec;
use ostd::Error;

use crate::fs::vfs::Result;
use super::file::{read_file_data, write_file_data};
use super::layout::ExFatFileInfo;
use super::superblock::ExFatFsState;

pub fn read_directory_entries(
    fs_state: &ExFatFsState,
    first_cluster: u32,
    no_fat_chain: bool,
    dir_size: u64,
) -> Result<Vec<ExFatFileInfo>> {
    let mut buf = alloc::vec![0u8; dir_size as usize];
    let bytes_read = read_file_data(fs_state, first_cluster, no_fat_chain, dir_size, 0, &mut buf)?;
    buf.truncate(bytes_read);

    let mut files = Vec::new();
    let mut offset = 0;

    while offset + 32 <= buf.len() {
        let entry_type = buf[offset];
        if entry_type == 0x00 {
            break;
        }

        if entry_type == 0x85 {
            let secondary_count = buf[offset + 1] as usize;
            let file_attributes = u16::from_le_bytes([buf[offset + 4], buf[offset + 5]]);
            let total_entries = 1 + secondary_count;

            if offset + total_entries * 32 <= buf.len() {
                let stream_offset = offset + 32;
                if buf[stream_offset] == 0xC0 {
                    let flags = buf[stream_offset + 1];
                    let stream_no_fat_chain = (flags & 0x02) != 0;
                    let file_first_cluster = u32::from_le_bytes([
                        buf[stream_offset + 20],
                        buf[stream_offset + 21],
                        buf[stream_offset + 22],
                        buf[stream_offset + 23],
                    ]);
                    let data_length = u64::from_le_bytes([
                        buf[stream_offset + 24],
                        buf[stream_offset + 25],
                        buf[stream_offset + 26],
                        buf[stream_offset + 27],
                        buf[stream_offset + 28],
                        buf[stream_offset + 29],
                        buf[stream_offset + 30],
                        buf[stream_offset + 31],
                    ]);

                    let mut name_utf16 = Vec::new();
                    let name_entries_count = secondary_count.saturating_sub(1);
                    for i in 0..name_entries_count {
                        let name_offset = offset + (2 + i) * 32;
                        if name_offset + 32 <= buf.len() && buf[name_offset] == 0xC1 {
                            for k in 0..15 {
                                let ch_offset = name_offset + 2 + k * 2;
                                let code_unit = u16::from_le_bytes([
                                    buf[ch_offset],
                                    buf[ch_offset + 1],
                                ]);
                                if code_unit == 0 {
                                    break;
                                }
                                name_utf16.push(code_unit);
                            }
                        }
                    }

                    let filename = String::from_utf16(&name_utf16).unwrap_or_default();
                    let is_dir = (file_attributes & 0x10) != 0;

                    files.push(ExFatFileInfo {
                        name: filename,
                        file_attributes,
                        first_cluster: file_first_cluster,
                        size: data_length,
                        is_dir,
                        no_fat_chain: stream_no_fat_chain,
                        entry_cluster: first_cluster,
                        entry_offset_in_dir: offset,
                        entry_count: total_entries,
                    });
                }
            }
            offset += total_entries * 32;
        } else {
            offset += 32;
        }
    }

    Ok(files)
}

pub fn find_free_dir_slots(
    fs_state: &ExFatFsState,
    dir_cluster: u32,
    dir_no_fat: &mut bool,
    dir_size: &mut u64,
    parent_parent: u32,
    _is_root: bool,
    parent_entry_offset: usize,
    slots_needed: usize,
) -> Result<usize> {
    let mut buf = alloc::vec![0u8; *dir_size as usize];
    read_file_data(fs_state, dir_cluster, *dir_no_fat, *dir_size, 0, &mut buf)?;

    let mut consecutive = 0;
    let mut start_idx = 0;

    let mut offset = 0;
    while offset + 32 <= buf.len() {
        let entry_type = buf[offset];
        if entry_type == 0x00 || (entry_type & 0x80) == 0 {
            if consecutive == 0 {
                start_idx = offset;
            }
            consecutive += 1;
            if consecutive == slots_needed {
                return Ok(start_idx);
            }
        } else {
            consecutive = 0;
        }
        offset += 32;
    }

    // Extend directory if needed
    let sector_size = 1u64 << fs_state.boot_sector.bytes_per_sector_shift;
    let cluster_size = sector_size * (1u64 << fs_state.boot_sector.sectors_per_cluster_shift);
    let old_size = *dir_size;

    let mut first_cluster = dir_cluster;
    let mut no_fat = *dir_no_fat;
    let mut size = *dir_size;

    super::file::extend_file(
        fs_state,
        &mut first_cluster,
        &mut no_fat,
        &mut size,
        old_size + cluster_size,
        parent_parent,
        false,
        old_size,
        parent_entry_offset,
    )?;

    *dir_no_fat = no_fat;
    *dir_size = size;

    let zeros = alloc::vec![0u8; cluster_size as usize];
    write_file_data(fs_state, first_cluster, no_fat, size, old_size, &zeros)?;

    find_free_dir_slots(
        fs_state,
        first_cluster,
        dir_no_fat,
        dir_size,
        parent_parent,
        false,
        parent_entry_offset,
        slots_needed,
    )
}

pub fn write_dir_entry_set(
    fs_state: &ExFatFsState,
    dir_cluster: u32,
    dir_no_fat: bool,
    dir_size: u64,
    start_offset: usize,
    name: &str,
    attributes: u16,
    first_cluster: u32,
    file_size: u64,
) -> Result<()> {
    let name_utf16: Vec<u16> = name.encode_utf16().collect();
    let name_entries_count = (name_utf16.len() + 14) / 15;
    let secondary_count = 1 + name_entries_count;

    // File Directory Entry (0x85)
    let mut file_entry = [0u8; 32];
    file_entry[0] = 0x85;
    file_entry[1] = secondary_count as u8;
    file_entry[4..6].copy_from_slice(&attributes.to_le_bytes());

    write_file_data(fs_state, dir_cluster, dir_no_fat, dir_size, start_offset as u64, &file_entry)?;

    // Stream Extension Entry (0xC0)
    let mut stream_entry = [0u8; 32];
    stream_entry[0] = 0xC0;
    stream_entry[1] = 0x03; // AllocationPossible | NoFatChain
    stream_entry[2] = name_utf16.len() as u8;
    stream_entry[20..24].copy_from_slice(&first_cluster.to_le_bytes());
    stream_entry[24..32].copy_from_slice(&file_size.to_le_bytes());

    write_file_data(fs_state, dir_cluster, dir_no_fat, dir_size, (start_offset + 32) as u64, &stream_entry)?;

    // File Name Entries (0xC1)
    for i in 0..name_entries_count {
        let mut name_entry = [0u8; 32];
        name_entry[0] = 0xC1;
        let start_ch = i * 15;
        for k in 0..15 {
            if start_ch + k < name_utf16.len() {
                let ch_offset = 2 + k * 2;
                name_entry[ch_offset..ch_offset + 2]
                    .copy_from_slice(&name_utf16[start_ch + k].to_le_bytes());
            }
        }
        write_file_data(
            fs_state,
            dir_cluster,
            dir_no_fat,
            dir_size,
            (start_offset + (2 + i) * 32) as u64,
            &name_entry,
        )?;
    }
    Ok(())
}

pub fn delete_dir_entry_set(
    fs_state: &ExFatFsState,
    dir_cluster: u32,
    dir_no_fat: bool,
    dir_size: u64,
    start_offset: usize,
    entry_count: usize,
) -> Result<()> {
    for i in 0..entry_count {
        let mut entry = [0u8; 32];
        read_file_data(
            fs_state,
            dir_cluster,
            dir_no_fat,
            dir_size,
            (start_offset + i * 32) as u64,
            &mut entry,
        )?;
        entry[0] &= 0x7F; // Clear in-use bit
        write_file_data(
            fs_state,
            dir_cluster,
            dir_no_fat,
            dir_size,
            (start_offset + i * 32) as u64,
            &entry,
        )?;
    }
    Ok(())
}
