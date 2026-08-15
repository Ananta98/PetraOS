use super::{File, VfsError};
use crate::sync::rwlock::RwLock;
use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use core::sync::atomic::{AtomicI32, Ordering};

/// Close-on-exec descriptor flag (`FD_CLOEXEC`).
pub const FD_CLOEXEC: u32 = 1;

/// Represents an individual open file descriptor entry.
#[derive(Clone)]
pub struct Descriptor {
    pub file: Arc<File>,
    pub flags: u32,
}

/// Per-process file descriptor table.
///
/// Manages the mapping from integer file descriptors to open [`Descriptor`] entries.
/// FDs 0, 1, 2 are reserved for stdin, stdout, stderr by convention;
/// user-allocated FDs start from 3.
pub struct FdTable {
    fds: RwLock<BTreeMap<i32, Descriptor>>,
    next_fd: AtomicI32,
}

impl FdTable {
    /// Create a new empty FD table. User FDs will start from 3.
    pub fn new() -> Self {
        Self {
            fds: RwLock::new(BTreeMap::new()),
            next_fd: AtomicI32::new(3),
        }
    }

    /// Allocate a new FD and associate it with `file`. Returns the FD number.
    pub fn alloc(&self, file: Arc<File>) -> i32 {
        self.alloc_with_flags(file, 0)
    }

    /// Allocate a new FD with specific descriptor flags (e.g. `FD_CLOEXEC`).
    pub fn alloc_with_flags(&self, file: Arc<File>, flags: u32) -> i32 {
        let mut map = self.fds.write();
        let mut candidate = self.next_fd.load(Ordering::SeqCst);
        while map.contains_key(&candidate) {
            candidate += 1;
        }
        self.next_fd.store(candidate + 1, Ordering::SeqCst);
        map.insert(candidate, Descriptor { file, flags });
        candidate
    }

    /// Allocate the lowest available FD >= `min_fd` (for `F_DUPFD` / `F_DUPFD_CLOEXEC`).
    pub fn alloc_from(&self, min_fd: i32, file: Arc<File>, flags: u32) -> Result<i32, VfsError> {
        if min_fd < 0 {
            return Err(VfsError::InvalidInput);
        }
        let mut map = self.fds.write();
        let mut candidate = min_fd;
        while map.contains_key(&candidate) {
            candidate += 1;
        }
        map.insert(candidate, Descriptor { file, flags });
        Ok(candidate)
    }

    /// Get the file associated with `fd`, or `VfsError::BadFd` if not open.
    pub fn get(&self, fd: i32) -> Result<Arc<File>, VfsError> {
        self.fds
            .read()
            .get(&fd)
            .map(|desc| desc.file.clone())
            .ok_or(VfsError::BadFd)
    }

    /// Get descriptor flags for `fd`.
    pub fn get_flags(&self, fd: i32) -> Result<u32, VfsError> {
        self.fds
            .read()
            .get(&fd)
            .map(|desc| desc.flags)
            .ok_or(VfsError::BadFd)
    }

    /// Set descriptor flags for `fd`.
    pub fn set_flags(&self, fd: i32, flags: u32) -> Result<(), VfsError> {
        let mut map = self.fds.write();
        let desc = map.get_mut(&fd).ok_or(VfsError::BadFd)?;
        desc.flags = flags;
        Ok(())
    }

    /// Close `fd`, returning `Ok(())` if it was open or `VfsError::BadFd` if not.
    pub fn close(&self, fd: i32) -> Result<(), VfsError> {
        if self.fds.write().remove(&fd).is_some() {
            Ok(())
        } else {
            Err(VfsError::BadFd)
        }
    }

    /// Associate a specific `fd` number with `file`. Closes previous file if `fd` was open.
    pub fn set(&self, fd: i32, file: Arc<File>) -> Result<(), VfsError> {
        self.set_with_flags(fd, file, 0)
    }

    /// Associate a specific `fd` number with `file` and flags.
    pub fn set_with_flags(&self, fd: i32, file: Arc<File>, flags: u32) -> Result<(), VfsError> {
        if fd < 0 {
            return Err(VfsError::BadFd);
        }
        self.fds.write().insert(fd, Descriptor { file, flags });
        Ok(())
    }

    /// Close all file descriptors marked with `FD_CLOEXEC` on `execve`.
    pub fn close_on_exec(&self) {
        let mut map = self.fds.write();
        let cloexec_fds: alloc::vec::Vec<i32> = map
            .iter()
            .filter_map(|(&fd, desc)| {
                if (desc.flags & FD_CLOEXEC) != 0 {
                    Some(fd)
                } else {
                    None
                }
            })
            .collect();

        for fd in cloexec_fds {
            map.remove(&fd);
        }
    }

    /// Duplicate the file descriptor table for process cloning (POSIX `fork()`).
    pub fn clone_table(&self) -> Self {
        let fds = self.fds.read().clone();
        let next_fd = self.next_fd.load(Ordering::SeqCst);
        Self {
            fds: RwLock::new(fds),
            next_fd: AtomicI32::new(next_fd),
        }
    }

    /// Set up standard file descriptors (0 = stdin, 1 = stdout, 2 = stderr)
    /// all pointing to the same console file.
    pub fn setup_std_fds(&self, console_file: Arc<File>) {
        let mut fds = self.fds.write();
        fds.insert(0, Descriptor { file: console_file.clone(), flags: 0 });
        fds.insert(1, Descriptor { file: console_file.clone(), flags: 0 });
        fds.insert(2, Descriptor { file: console_file, flags: 0 });
    }
}
