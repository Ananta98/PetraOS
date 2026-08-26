use super::{SyscallError, SyscallResult, UserCStr, UserPtr};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::fs::File;
use crate::fs::vfs::types::{InodeType, LinuxStat, SeekWhence, Stat, StatFs};
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;

// ── Modular syscall submodules ──────────────────────────────────────────
pub mod read;
pub mod write;
pub mod pread64;
pub mod pwrite64;
pub mod open;
pub mod openat;
pub mod close;
pub mod stat;
pub mod fstat;
pub mod lseek;
pub mod dup;
pub mod dup2;
pub mod dup3;
pub mod pipe;
pub mod pipe2;
pub mod getcwd;
pub mod chdir;
pub mod fcntl;
pub mod flock;
pub mod fchdir;
pub mod statfs;
pub mod fstatfs;
pub mod access;
pub mod umask;
pub mod newfstatat;
pub mod lstat;
pub mod poll;
pub mod ppoll;
pub mod readv;
pub mod writev;
pub mod select;
pub mod pselect6;
pub mod fsync;
pub mod fdatasync;
pub mod truncate;
pub mod ftruncate;
pub mod rename;
pub mod renameat;
pub mod renameat2;
pub mod mkdir;
pub mod mkdirat;
pub mod rmdir;
pub mod link;
pub mod linkat;
pub mod unlink;
pub mod unlinkat;
pub mod symlink;
pub mod symlinkat;
pub mod readlink;
pub mod readlinkat;
pub mod chmod;
pub mod fchmod;
pub mod fchmodat;
pub mod chown;
pub mod fchown;
pub mod lchown;
pub mod fchownat;
pub mod mknodat;
pub mod utimensat;
pub mod futimesat;
pub mod faccessat;
pub mod getdents64;
pub mod fadvise64;

pub use read::sys_read;
pub use write::sys_write;
pub use pread64::sys_pread64;
pub use pwrite64::sys_pwrite64;
pub use open::sys_open;
pub use openat::sys_openat;
pub use close::sys_close;
pub use stat::sys_stat;
pub use fstat::sys_fstat;
pub use lseek::sys_lseek;
pub use dup::sys_dup;
pub use dup2::sys_dup2;
pub use dup3::sys_dup3;
pub use pipe::sys_pipe;
pub use pipe2::sys_pipe2;
pub use getcwd::sys_getcwd;
pub use chdir::sys_chdir;
pub use fcntl::sys_fcntl;
pub use flock::sys_flock;
pub use fchdir::sys_fchdir;
pub use statfs::sys_statfs;
pub use fstatfs::sys_fstatfs;
pub use access::sys_access;
pub use umask::sys_umask;
pub use newfstatat::sys_newfstatat;
pub use lstat::sys_lstat;
pub use poll::sys_poll;
pub use ppoll::sys_ppoll;
pub use readv::sys_readv;
pub use writev::sys_writev;
pub use select::sys_select;
pub use pselect6::sys_pselect6;
pub use fsync::sys_fsync;
pub use fdatasync::sys_fdatasync;
pub use truncate::sys_truncate;
pub use ftruncate::sys_ftruncate;
pub use rename::sys_rename;
pub use renameat::sys_renameat;
pub use renameat2::sys_renameat2;
pub use mkdir::sys_mkdir;
pub use mkdirat::sys_mkdirat;
pub use rmdir::sys_rmdir;
pub use link::sys_link;
pub use linkat::sys_linkat;
pub use unlink::sys_unlink;
pub use unlinkat::sys_unlinkat;
pub use symlink::sys_symlink;
pub use symlinkat::sys_symlinkat;
pub use readlink::sys_readlink;
pub use readlinkat::sys_readlinkat;
pub use chmod::sys_chmod;
pub use fchmod::sys_fchmod;
pub use fchmodat::sys_fchmodat;
pub use chown::sys_chown;
pub use fchown::sys_fchown;
pub use lchown::sys_lchown;
pub use fchownat::sys_fchownat;
pub use mknodat::sys_mknodat;
pub use utimensat::sys_utimensat;
pub use futimesat::sys_futimesat;
pub use faccessat::sys_faccessat;
pub use getdents64::sys_getdents64;
pub use fadvise64::sys_fadvise64;


pub const AT_FDCWD: i32 = -100;

pub const O_CLOEXEC: u32 = 0x80000;
pub const O_NONBLOCK: u32 = 0x800;

pub const F_DUPFD: i32 = 0;
pub const F_GETFD: i32 = 1;
pub const F_SETFD: i32 = 2;
pub const F_GETFL: i32 = 3;
pub const F_SETFL: i32 = 4;
pub const F_GETLK: i32 = 5;
pub const F_SETLK: i32 = 6;
pub const F_SETLKW: i32 = 7;
pub const F_SETOWN: i32 = 8;
pub const F_GETOWN: i32 = 9;
pub const F_DUPFD_CLOEXEC: i32 = 1030;

pub const DT_UNKNOWN: u8 = 0;
pub const DT_FIFO: u8 = 1;
pub const DT_CHR: u8 = 2;
pub const DT_DIR: u8 = 4;
pub const DT_BLK: u8 = 6;
pub const DT_REG: u8 = 8;
pub const DT_LNK: u8 = 10;
pub const DT_SOCK: u8 = 12;

pub const POLLIN: i16 = 0x0001;
pub const POLLPRI: i16 = 0x0002;
pub const POLLOUT: i16 = 0x0004;
pub const POLLERR: i16 = 0x0008;
pub const POLLHUP: i16 = 0x0010;
pub const POLLNVAL: i16 = 0x0020;

/// `utimensat` special `tv_nsec` value: set the timestamp to the current time.
pub(crate) const UTIME_NOW: i64 = 0x3FFF_FFFF;
/// `utimensat` special `tv_nsec` value: leave the corresponding timestamp unchanged.
pub(crate) const UTIME_OMIT: i64 = 0x3FFF_FFFE;
/// Maximum valid nanosecond value within a timespec.
pub(crate) const NSEC_MAX: i64 = 999_999_999;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct IoVec {
    pub iov_base: u64,
    pub iov_len: usize,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PollFd {
    pub fd: i32,
    pub events: i16,
    pub revents: i16,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct LinuxTimespec {
    pub tv_sec: i64,
    pub tv_nsec: i64,
}

/// Current wall-clock time in seconds since the UNIX epoch.
pub(crate) fn wall_now_secs() -> u64 {
    crate::drivers::time::cmos_rtc::get_wall_time().0
}

/// Resolve one user-provided timespec against the value currently stored on disk.
pub(crate) fn resolve_timespec(ts: LinuxTimespec, current: u64) -> Result<u64, SyscallError> {
    match ts.tv_nsec {
        UTIME_NOW => Ok(wall_now_secs()),
        UTIME_OMIT => Ok(current),
        n if (0..=NSEC_MAX).contains(&n) => Ok(ts.tv_sec as u64),
        _ => Err(SyscallError::EINVAL),
    }
}

/// Convert a user-provided timespec pair into concrete `(atime, mtime)`
/// seconds for `utimensat`. A null pointer selects "current time" for both
/// fields; `UTIME_OMIT` fields fall back to the current on-disk values.
pub(crate) fn read_utimens(
    times_ptr: UserPtr<LinuxTimespec>,
    cur_atime: u64,
    cur_mtime: u64,
) -> Result<(u64, u64), SyscallError> {
    if times_ptr.is_null() {
        let now = wall_now_secs();
        return Ok((now, now));
    }
    let times = times_ptr.as_slice(2).ok_or(SyscallError::EFAULT)?;
    Ok((
        resolve_timespec(times[0], cur_atime)?,
        resolve_timespec(times[1], cur_mtime)?,
    ))
}

/// Substitute the `(uid_t)-1` / `(gid_t)-1` "leave unchanged" sentinels with
/// the ownership currently recorded in `st`.
pub(crate) fn effective_owner(st: &Stat, uid: u32, gid: u32) -> (u32, u32) {
    (
        if uid == u32::MAX { st.uid } else { uid },
        if gid == u32::MAX { st.gid } else { gid },
    )
}

pub const FD_SETSIZE: usize = 1024;
#[repr(C)]
#[derive(Clone, Copy)]
pub struct FdSet {
    pub fds_bits: [u64; FD_SETSIZE / 64],
}

pub fn resolve_at_path(dfd: i32, path: &str) -> Result<String, SyscallError> {
    if path.starts_with('/') {
        Ok(crate::fs::normalize_path("/", path))
    } else if dfd == AT_FDCWD || dfd as u32 == 0xffffff9c {
        let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
        let proc = proc_arc.lock();
        Ok(crate::fs::normalize_path(&proc.cwd, path))
    } else if dfd >= 0 {
        let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
        let proc = proc_arc.lock();
        let file = proc.fd_table.get(dfd)?;
        let dir_path = crate::fs::build_path(&file.dentry);
        Ok(crate::fs::normalize_path(&dir_path, path))
    } else {
        let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
        let proc = proc_arc.lock();
        Ok(crate::fs::normalize_path(&proc.cwd, path))
    }
}

pub(crate) fn do_openat(dfd: i32, path: &str, flags: u32) -> SyscallResult {
    let full_path = resolve_at_path(dfd, path)?;

    let dentry = match crate::fs::resolve_path(&full_path) {
        Ok(d) => {
            if (flags & crate::fs::O_CREAT) != 0 && (flags & crate::fs::O_EXCL) != 0 {
                return Err(SyscallError::EEXIST);
            }
            if (flags & crate::fs::O_DIRECTORY) != 0 && d.inode.inode_type != InodeType::Directory {
                return Err(SyscallError::ENOTDIR);
            }
            if d.inode.inode_type == InodeType::Directory && (crate::fs::can_write(flags)) {
                return Err(SyscallError::EISDIR);
            }
            if (flags & crate::fs::O_TRUNC) != 0 && crate::fs::can_write(flags) {
                let _ = d.inode.ops.truncate(0);
            }
            d
        }
        Err(crate::fs::vfs::types::VfsError::NotFound) if (flags & crate::fs::O_CREAT) != 0 => {
            if (flags & crate::fs::O_DIRECTORY) != 0 {
                return Err(SyscallError::ENOENT);
            }
            crate::fs::create_file(&full_path)?
        }
        Err(err) => return Err(SyscallError::from(err)),
    };

    let file_ops = dentry.inode.ops.open()?;
    let file = Arc::new(File::new(dentry, flags, file_ops));

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    let fd = proc.fd_table.alloc(file);

    Ok(fd as usize)
}

pub(crate) const RAMFS_MAGIC: i64 = 0x858458f6;

pub(crate) fn make_statfs() -> StatFs {
    StatFs {
        f_type: RAMFS_MAGIC,
        f_bsize: 4096,
        f_blocks: 262144, // ~1GB
        f_bfree: 200000,
        f_bavail: 200000,
        f_files: 65536,
        f_ffree: 60000,
        f_fsid: [0, 0],
        f_namelen: 255,
        f_frsize: 4096,
        f_flags: 0,
        f_spare: [0; 4],
    }
}

pub(crate) fn do_poll(fds_ptr: UserPtr<PollFd>, nfds: usize, timeout_ms: i32) -> SyscallResult {
    if nfds == 0 {
        if timeout_ms > 0 {
            let start_ns = crate::arch::timer::hpet::elapsed_ns();
            let dur_ns = (timeout_ms as u64) * 1_000_000;
            while crate::arch::timer::hpet::elapsed_ns().saturating_sub(start_ns) < dur_ns {
                crate::arch::enable_interrupts();
                crate::proc::thread::Thread::yield_cpu();
            }
        }
        return Ok(0);
    }
    if nfds > 1024 {
        return Err(SyscallError::EFAULT);
    }
    let fds_slice = fds_ptr.as_slice_mut(nfds).ok_or(SyscallError::EFAULT)?;

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;

    let start_ns = crate::arch::timer::hpet::elapsed_ns();
    let has_timeout = timeout_ms >= 0;
    let dur_ns = if timeout_ms > 0 {
        (timeout_ms as u64) * 1_000_000
    } else {
        0
    };

    loop {
        // Retrieve file handles under proc lock, then drop proc lock before polling events
        let files: Vec<Option<Arc<File>>> = {
            let proc = proc_arc.lock();
            fds_slice
                .iter()
                .map(|pfd| {
                    if pfd.fd >= 0 {
                        proc.fd_table.get(pfd.fd).ok()
                    } else {
                        None
                    }
                })
                .collect()
        };

        let mut ready_count = 0;
        for (pfd, file_opt) in fds_slice.iter_mut().zip(files.iter()) {
            if pfd.fd < 0 {
                pfd.revents = 0;
                continue;
            }
            match file_opt {
                Some(file) => {
                    let revents = file.ops.poll_events(pfd.events);
                    pfd.revents = revents;
                    if revents != 0 {
                        ready_count += 1;
                    }
                }
                None => {
                    pfd.revents = POLLNVAL;
                    ready_count += 1;
                }
            }
        }

        if ready_count > 0 {
            return Ok(ready_count);
        }

        if has_timeout {
            if timeout_ms == 0 {
                return Ok(0);
            }
            if crate::arch::timer::hpet::elapsed_ns().saturating_sub(start_ns) >= dur_ns {
                return Ok(0);
            }
        }

        // Check if there are pending signals interrupting poll
        {
            let proc = proc_arc.lock();
            if proc.pending_signals.mask != 0 {
                return Err(SyscallError::EINTR);
            }
        }

        crate::arch::enable_and_hlt();
    }
}

pub(crate) fn copy_to_linux_stat(stat: &Stat) -> LinuxStat {
    LinuxStat {
        st_dev: 1,
        st_ino: stat.ino,
        st_nlink: stat.nlink as u64,
        st_mode: stat.mode,
        st_uid: stat.uid,
        st_gid: stat.gid,
        __pad0: 0,
        st_rdev: 0,
        st_size: stat.size as i64,
        st_blksize: if stat.blksize > 0 {
            stat.blksize as i64
        } else {
            4096
        },
        st_blocks: stat.blocks as i64,
        st_atime: stat.atime,
        st_atime_nsec: 0,
        st_mtime: stat.mtime,
        st_mtime_nsec: 0,
        st_ctime: stat.ctime,
        st_ctime_nsec: 0,
        __glibc_reserved: [0; 3],
    }
}
