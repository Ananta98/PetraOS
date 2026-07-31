use crate::fs::vfs::FileType;
use crate::proc::process::Process;
use crate::syscall::{SyscallResult, to_continue_i32};
use crate::vm::vma::VmaManager;
use ostd::Error;
use ostd::arch::cpu::context::UserContext;

pub const DT_UNKNOWN: u8 = 0;
pub const DT_FIFO: u8 = 1;
pub const DT_CHR: u8 = 2;
pub const DT_DIR: u8 = 4;
pub const DT_BLK: u8 = 6;
pub const DT_REG: u8 = 8;
pub const DT_LNK: u8 = 10;

/// `getdents64()` — SYS_getdents64 = 217
pub fn syscall_getdents64(
    arg0: usize,
    arg1: usize,
    arg2: usize,
    _: usize,
    _: usize,
    _: usize,
    vm: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    let fd = arg0 as i32;
    let dir_ptr = arg1;
    let count = arg2;

    if dir_ptr == 0 || count < 24 {
        return to_continue_i32(Err(Error::InvalidArgs));
    }

    let proc = Process::current();
    let fd_table = proc.fd_table.lock();
    let fd_entry = match fd_table.get_fd(fd) {
        Ok(f) => f,
        Err(e) => return to_continue_i32(Err(e)),
    };

    let mut open_file = fd_entry.open_file.lock();
    let entries = match open_file.file_ops.readdir() {
        Ok(e) => e,
        Err(e) => return to_continue_i32(Err(e)),
    };

    let mut written_bytes = 0usize;
    let mut current_offset = dir_ptr;

    for (idx, entry) in entries.iter().enumerate() {
        let name_bytes = entry.name.as_bytes();
        let name_len = name_bytes.len();
        // 8 (ino) + 8 (off) + 2 (reclen) + 1 (type) + name_len + 1 (null)
        let unaligned_rec_len = 19 + name_len + 1;
        let rec_len = (unaligned_rec_len + 7) & !7; // 8-byte align

        if written_bytes + rec_len > count {
            break;
        }

        let d_type = match entry.file_type {
            FileType::Regular => DT_REG,
            FileType::Directory => DT_DIR,
            FileType::Symlink => DT_LNK,
            FileType::CharDevice => DT_CHR,
            FileType::BlockDevice => DT_BLK,
        };

        // Write d_ino (8 bytes)
        if vm
            .copy_to_user(current_offset, &entry.inode_num.to_ne_bytes())
            .is_err()
        {
            return to_continue_i32(Err(Error::InvalidArgs));
        }
        // Write d_off (8 bytes)
        let next_off = (idx + 1) as i64;
        if vm
            .copy_to_user(current_offset + 8, &next_off.to_ne_bytes())
            .is_err()
        {
            return to_continue_i32(Err(Error::InvalidArgs));
        }
        // Write d_reclen (2 bytes)
        let rec_len_u16 = rec_len as u16;
        if vm
            .copy_to_user(current_offset + 16, &rec_len_u16.to_ne_bytes())
            .is_err()
        {
            return to_continue_i32(Err(Error::InvalidArgs));
        }
        // Write d_type (1 byte)
        if vm.copy_to_user(current_offset + 18, &[d_type]).is_err() {
            return to_continue_i32(Err(Error::InvalidArgs));
        }
        // Write d_name (name_len + null terminator)
        if vm.copy_to_user(current_offset + 19, name_bytes).is_err() {
            return to_continue_i32(Err(Error::InvalidArgs));
        }
        if vm
            .copy_to_user(current_offset + 19 + name_len, &[0u8])
            .is_err()
        {
            return to_continue_i32(Err(Error::InvalidArgs));
        }

        current_offset += rec_len;
        written_bytes += rec_len;
    }

    to_continue_i32(Ok(written_bytes as i32))
}
