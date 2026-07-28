use crate::fs::vfs::resolve_path;
use crate::proc::process::Process;
use crate::proc::userspace::read_user_string;
use crate::syscall::{SyscallResult, to_continue_i32};
use crate::vm::vma::VmaManager;
use alloc::string::String;
use ostd::Error;
use ostd::arch::cpu::context::UserContext;

#[repr(C)]
#[derive(Default, Copy, Clone)]
pub struct Statfs {
    pub f_type: i64,
    pub f_bsize: i64,
    pub f_blocks: u64,
    pub f_bfree: u64,
    pub f_bavail: u64,
    pub f_files: u64,
    pub f_ffree: u64,
    pub f_fsid: [i32; 2],
    pub f_namelen: i64,
    pub f_frsize: i64,
    pub f_flags: i64,
    pub f_spare: [i64; 4],
}

fn split_parent_filename(path: &str) -> (&str, &str) {
    if let Some(pos) = path.rfind('/') {
        let (parent, file) = path.split_at(pos);
        let parent = if parent.is_empty() { "/" } else { parent };
        (parent, &file[1..])
    } else {
        (".", path)
    }
}

/// `getcwd()` — SYS_getcwd = 79
pub fn syscall_getcwd(
    arg0: usize,
    arg1: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    vm: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    let buf_ptr = arg0;
    let size = arg1;
    if buf_ptr == 0 || size < 2 {
        return to_continue_i32(Err(Error::InvalidArgs));
    }
    let cwd = String::from("/");
    let bytes = cwd.as_bytes();
    if bytes.len() + 1 > size {
        return to_continue_i32(Err(Error::InvalidArgs));
    }
    if vm.copy_to_user(buf_ptr, bytes).is_err() {
        return to_continue_i32(Err(Error::InvalidArgs));
    }
    if vm.copy_to_user(buf_ptr + bytes.len(), &[0u8]).is_err() {
        return to_continue_i32(Err(Error::InvalidArgs));
    }
    to_continue_i32(Ok(buf_ptr as i32))
}

/// `mkdir()` — SYS_mkdir = 83
pub fn syscall_mkdir(
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
    let mode = arg1 as u32;
    let (parent_path, filename) = split_parent_filename(&path);
    let parent_dentry = match resolve_path(parent_path) {
        Ok(p) => p,
        Err(e) => return to_continue_i32(Err(e)),
    };
    to_continue_i32(parent_dentry.inode.mkdir(filename, mode).map(|_| 0))
}

/// `mkdirat()` — SYS_mkdirat = 258
pub fn syscall_mkdirat(
    _dfd: usize,
    arg1: usize,
    arg2: usize,
    _: usize,
    _: usize,
    _: usize,
    vm: &VmaManager,
    ctx: &mut UserContext,
) -> SyscallResult {
    syscall_mkdir(arg1, arg2, 0, 0, 0, 0, vm, ctx)
}

/// `rmdir()` — SYS_rmdir = 84
pub fn syscall_rmdir(
    arg0: usize,
    _: usize,
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
    let (parent_path, filename) = split_parent_filename(&path);
    let parent_dentry = match resolve_path(parent_path) {
        Ok(p) => p,
        Err(e) => return to_continue_i32(Err(e)),
    };
    to_continue_i32(parent_dentry.inode.unlink(filename).map(|_| 0))
}

/// `unlink()` — SYS_unlink = 87
pub fn syscall_unlink(
    arg0: usize,
    _: usize,
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
    let (parent_path, filename) = split_parent_filename(&path);
    let parent_dentry = match resolve_path(parent_path) {
        Ok(p) => p,
        Err(e) => return to_continue_i32(Err(e)),
    };
    to_continue_i32(parent_dentry.inode.unlink(filename).map(|_| 0))
}

/// `unlinkat()` — SYS_unlinkat = 263
pub fn syscall_unlinkat(
    _dfd: usize,
    arg1: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    vm: &VmaManager,
    ctx: &mut UserContext,
) -> SyscallResult {
    syscall_unlink(arg1, 0, 0, 0, 0, 0, vm, ctx)
}

/// `rename()` — SYS_rename = 82
pub fn syscall_rename(
    arg0: usize,
    arg1: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    vm: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    let old_path = match read_user_string(vm, arg0) {
        Ok(p) => p,
        Err(e) => return to_continue_i32(Err(e)),
    };
    let new_path = match read_user_string(vm, arg1) {
        Ok(p) => p,
        Err(e) => return to_continue_i32(Err(e)),
    };
    let (old_parent_path, old_filename) = split_parent_filename(&old_path);
    let (new_parent_path, new_filename) = split_parent_filename(&new_path);
    let old_parent = match resolve_path(old_parent_path) {
        Ok(p) => p,
        Err(e) => return to_continue_i32(Err(e)),
    };
    let new_parent = match resolve_path(new_parent_path) {
        Ok(p) => p,
        Err(e) => return to_continue_i32(Err(e)),
    };
    to_continue_i32(old_parent.inode.rename(old_filename, &new_parent.inode, new_filename).map(|_| 0))
}

/// `renameat()` — SYS_renameat = 264
pub fn syscall_renameat(
    _olddfd: usize,
    arg1: usize,
    _newdfd: usize,
    arg3: usize,
    _: usize,
    _: usize,
    vm: &VmaManager,
    ctx: &mut UserContext,
) -> SyscallResult {
    syscall_rename(arg1, arg3, 0, 0, 0, 0, vm, ctx)
}

/// `readlink()` — SYS_readlink = 89
pub fn syscall_readlink(
    arg0: usize,
    arg1: usize,
    arg2: usize,
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
    let buf_ptr = arg1;
    let bufsiz = arg2;
    if buf_ptr == 0 || bufsiz == 0 {
        return to_continue_i32(Err(Error::InvalidArgs));
    }
    let dentry = match resolve_path(&path) {
        Ok(d) => d,
        Err(e) => return to_continue_i32(Err(e)),
    };
    let target = match dentry.inode.read_link() {
        Ok(t) => t,
        Err(e) => return to_continue_i32(Err(e)),
    };
    let bytes = target.as_bytes();
    let copy_len = bytes.len().min(bufsiz);
    if vm.copy_to_user(buf_ptr, &bytes[..copy_len]).is_err() {
        return to_continue_i32(Err(Error::InvalidArgs));
    }
    to_continue_i32(Ok(copy_len as i32))
}

/// `readlinkat()` — SYS_readlinkat = 267
pub fn syscall_readlinkat(
    _dfd: usize,
    arg1: usize,
    arg2: usize,
    arg3: usize,
    _: usize,
    _: usize,
    vm: &VmaManager,
    ctx: &mut UserContext,
) -> SyscallResult {
    syscall_readlink(arg1, arg2, arg3, 0, 0, 0, vm, ctx)
}

/// `umask()` — SYS_umask = 95
pub fn syscall_umask(
    arg0: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    let _mask = arg0 as u32;
    to_continue_i32(Ok(0o022))
}

/// `statfs()` — SYS_statfs = 137
pub fn syscall_statfs(
    _arg0: usize,
    arg1: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    vm: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    if arg1 == 0 {
        return to_continue_i32(Err(Error::InvalidArgs));
    }
    let sfs = Statfs {
        f_type: 0xadf5, // TMPFS_MAGIC
        f_bsize: 4096,
        f_blocks: 1_000_000,
        f_bfree: 800_000,
        f_bavail: 800_000,
        f_files: 100_000,
        f_ffree: 90_000,
        f_fsid: [0, 0],
        f_namelen: 255,
        f_frsize: 4096,
        f_flags: 0,
        f_spare: [0; 4],
    };
    let mut buf = [0u8; 120];
    buf[0..8].copy_from_slice(&sfs.f_type.to_ne_bytes());
    buf[8..16].copy_from_slice(&sfs.f_bsize.to_ne_bytes());
    buf[16..24].copy_from_slice(&sfs.f_blocks.to_ne_bytes());
    buf[24..32].copy_from_slice(&sfs.f_bfree.to_ne_bytes());
    buf[32..40].copy_from_slice(&sfs.f_bavail.to_ne_bytes());
    buf[40..48].copy_from_slice(&sfs.f_files.to_ne_bytes());
    buf[48..56].copy_from_slice(&sfs.f_ffree.to_ne_bytes());
    buf[56..60].copy_from_slice(&sfs.f_fsid[0].to_ne_bytes());
    buf[60..64].copy_from_slice(&sfs.f_fsid[1].to_ne_bytes());
    buf[64..72].copy_from_slice(&sfs.f_namelen.to_ne_bytes());
    buf[72..80].copy_from_slice(&sfs.f_frsize.to_ne_bytes());
    buf[80..88].copy_from_slice(&sfs.f_flags.to_ne_bytes());

    to_continue_i32(vm.copy_to_user(arg1, &buf).map(|_| 0))
}

/// `fstatfs()` — SYS_fstatfs = 138
pub fn syscall_fstatfs(
    arg0: usize,
    arg1: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    vm: &VmaManager,
    ctx: &mut UserContext,
) -> SyscallResult {
    syscall_statfs(arg0, arg1, 0, 0, 0, 0, vm, ctx)
}

/// `openat()` — SYS_openat = 257
pub fn syscall_openat(
    _dfd: usize,
    arg1: usize,
    arg2: usize,
    arg3: usize,
    _: usize,
    _: usize,
    vm: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    let flags = arg2 as u32;
    let mode = arg3 as u32;
    let path = match read_user_string(vm, arg1) {
        Ok(p) => p,
        Err(e) => return to_continue_i32(Err(e)),
    };
    let proc = Process::current();
    to_continue_i32(proc.fd_table.lock().open(&path, flags, mode))
}
