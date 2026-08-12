use crate::fs::vfs::types::{File, VfsError};
use crate::sync::spinlock::Spinlock;
use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use core::sync::atomic::{AtomicI32, Ordering};

/// Per-process file descriptor table.
///
/// Manages the mapping from integer file descriptors to open [`File`] objects.
/// FDs 0, 1, 2 are reserved for stdin, stdout, stderr by convention;
/// user-allocated FDs start from 3.
pub struct FdTable {
    fds: Spinlock<BTreeMap<i32, Arc<File>>>,
    next_fd: AtomicI32,
}

impl FdTable {
    /// Create a new empty FD table. User FDs will start from 3.
    pub fn new() -> Self {
        Self {
            fds: Spinlock::new(BTreeMap::new()),
            next_fd: AtomicI32::new(3),
        }
    }

    /// Allocate a new FD and associate it with `file`. Returns the FD number.
    pub fn alloc(&self, file: Arc<File>) -> i32 {
        let fd = self.next_fd.fetch_add(1, Ordering::SeqCst);
        self.fds.lock().insert(fd, file);
        fd
    }

    /// Get the file associated with `fd`, or `VfsError::BadFd` if not open.
    pub fn get(&self, fd: i32) -> Result<Arc<File>, VfsError> {
        self.fds.lock().get(&fd).cloned().ok_or(VfsError::BadFd)
    }

    /// Close `fd`, returning `Ok(())` if it was open or `VfsError::BadFd` if not.
    pub fn close(&self, fd: i32) -> Result<(), VfsError> {
        if self.fds.lock().remove(&fd).is_some() {
            Ok(())
        } else {
            Err(VfsError::BadFd)
        }
    }

    /// Associate a specific `fd` number with `file`. Closes previous file if `fd` was open.
    pub fn set(&self, fd: i32, file: Arc<File>) -> Result<(), VfsError> {
        if fd < 0 {
            return Err(VfsError::BadFd);
        }
        self.fds.lock().insert(fd, file);
        Ok(())
    }

    /// Duplicate the file descriptor table for process cloning (POSIX `fork()`).
    pub fn clone_table(&self) -> Self {
        let fds = self.fds.lock().clone();
        let next_fd = self.next_fd.load(Ordering::SeqCst);
        Self {
            fds: Spinlock::new(fds),
            next_fd: AtomicI32::new(next_fd),
        }
    }

    /// Set up standard file descriptors (0 = stdin, 1 = stdout, 2 = stderr)
    /// all pointing to the same console file.
    pub fn setup_std_fds(&self, console_file: Arc<File>) {
        let mut fds = self.fds.lock();
        fds.insert(0, console_file.clone());
        fds.insert(1, console_file.clone());
        fds.insert(2, console_file);
    }
}

