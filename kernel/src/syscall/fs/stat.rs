use crate::fs::vfs::{FileType, Metadata, resolve_path};
use crate::proc::process::Process;
use crate::proc::userspace::read_user_string;
use crate::syscall::{SyscallResult, to_continue_i32};
use crate::vm::vma::VmaManager;
use ostd::Error;
use ostd::arch::cpu::context::UserContext;

#[repr(C)]
#[derive(Default, Copy, Clone)]
pub struct LinuxStat {
    pub st_dev: u64,
    pub st_ino: u64,
    pub st_nlink: u64,
    pub st_mode: u32,
    pub st_uid: u32,
    pub st_gid: u32,
    pub __pad0: u32,
    pub st_rdev: u64,
    pub st_size: i64,
    pub st_blksize: i64,
    pub st_blocks: i64,
    pub st_atime: i64,
    pub st_atime_nsec: u64,
    pub st_mtime: i64,
    pub st_mtime_nsec: u64,
    pub st_ctime: i64,
    pub st_ctime_nsec: u64,
    pub __glibc_reserved: [i64; 3],
}

pub fn metadata_to_linux_stat(meta: &Metadata) -> LinuxStat {
    let mode_type = match meta.file_type {
        FileType::Regular => 0o100000,
        FileType::Directory => 0o040000,
        FileType::Symlink => 0o120000,
        FileType::CharDevice => 0o020000,
        FileType::BlockDevice => 0o060000,
    };

    LinuxStat {
        st_dev: 1,
        st_ino: meta.inode_num,
        st_nlink: meta.nlink as u64,
        st_mode: mode_type | (meta.mode & 0o7777),
        st_uid: meta.uid,
        st_gid: meta.gid,
        __pad0: 0,
        st_rdev: 0,
        st_size: meta.size as i64,
        st_blksize: 4096,
        st_blocks: (meta.size as i64 + 511) / 512,
        st_atime: 0,
        st_atime_nsec: 0,
        st_mtime: 0,
        st_mtime_nsec: 0,
        st_ctime: 0,
        st_ctime_nsec: 0,
        __glibc_reserved: [0; 3],
    }
}

pub fn write_linux_stat(vm: &VmaManager, user_ptr: usize, stat: &LinuxStat) -> Result<(), Error> {
    if user_ptr == 0 {
        return Err(Error::InvalidArgs);
    }
    let mut buf = [0u8; 144];
    buf[0..8].copy_from_slice(&stat.st_dev.to_ne_bytes());
    buf[8..16].copy_from_slice(&stat.st_ino.to_ne_bytes());
    buf[16..24].copy_from_slice(&stat.st_nlink.to_ne_bytes());
    buf[24..28].copy_from_slice(&stat.st_mode.to_ne_bytes());
    buf[28..32].copy_from_slice(&stat.st_uid.to_ne_bytes());
    buf[32..36].copy_from_slice(&stat.st_gid.to_ne_bytes());
    buf[36..40].copy_from_slice(&stat.__pad0.to_ne_bytes());
    buf[40..48].copy_from_slice(&stat.st_rdev.to_ne_bytes());
    buf[48..56].copy_from_slice(&stat.st_size.to_ne_bytes());
    buf[56..64].copy_from_slice(&stat.st_blksize.to_ne_bytes());
    buf[64..72].copy_from_slice(&stat.st_blocks.to_ne_bytes());
    buf[72..80].copy_from_slice(&stat.st_atime.to_ne_bytes());
    buf[80..88].copy_from_slice(&stat.st_atime_nsec.to_ne_bytes());
    buf[88..96].copy_from_slice(&stat.st_mtime.to_ne_bytes());
    buf[96..104].copy_from_slice(&stat.st_mtime_nsec.to_ne_bytes());
    buf[104..112].copy_from_slice(&stat.st_ctime.to_ne_bytes());
    buf[112..120].copy_from_slice(&stat.st_ctime_nsec.to_ne_bytes());

    vm.copy_to_user(user_ptr, &buf)
}

/// `stat()` — SYS_stat = 4
pub fn syscall_stat(
    arg0: usize,
    arg1: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    vm: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    let path = match read_user_string(vm, arg0) {
        Ok(p) => p,
        Err(e) => return to_continue_i32(Err(e)),
    };

    match resolve_path(&path) {
        Ok(dentry) => match dentry.inode.metadata() {
            Ok(meta) => {
                let linux_stat = metadata_to_linux_stat(&meta);
                to_continue_i32(write_linux_stat(vm, arg1, &linux_stat).map(|_| 0))
            }
            Err(e) => to_continue_i32(Err(e)),
        },
        Err(e) => to_continue_i32(Err(e)),
    }
}

/// `fstat()` — SYS_fstat = 5
pub fn syscall_fstat(
    arg0: usize,
    arg1: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    vm: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    let fd = arg0 as i32;
    let proc = Process::current();
    let fd_table = proc.fd_table.lock();
    let fd_entry = match fd_table.get_fd(fd) {
        Ok(f) => f,
        Err(e) => return to_continue_i32(Err(e)),
    };

    let open_file = fd_entry.open_file.lock();
    let meta = if let Some(ref inode) = open_file.inode {
        match inode.metadata() {
            Ok(m) => m,
            Err(e) => return to_continue_i32(Err(e)),
        }
    } else {
        Metadata {
            size: 0,
            file_type: FileType::Regular,
            mode: 0o600,
            uid: 0,
            gid: 0,
            inode_num: 1,
            nlink: 1,
        }
    };

    let linux_stat = metadata_to_linux_stat(&meta);
    to_continue_i32(write_linux_stat(vm, arg1, &linux_stat).map(|_| 0))
}

/// `lstat()` — SYS_lstat = 6
pub fn syscall_lstat(
    arg0: usize,
    arg1: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    vm: &VmaManager,
    ctx: &mut UserContext,
) -> SyscallResult {
    syscall_stat(arg0, arg1, 0, 0, 0, 0, vm, ctx)
}

/// `newfstatat()` — SYS_newfstatat = 262
pub fn syscall_newfstatat(
    arg0: usize,
    arg1: usize,
    arg2: usize,
    _arg3: usize,
    _: usize,
    _: usize,
    vm: &VmaManager,
    ctx: &mut UserContext,
) -> SyscallResult {
    let dfd = arg0 as i32;
    if dfd == -100 || arg1 != 0 {
        // AT_FDCWD
        syscall_stat(arg1, arg2, 0, 0, 0, 0, vm, ctx)
    } else {
        syscall_fstat(arg0, arg2, 0, 0, 0, 0, vm, ctx)
    }
}
