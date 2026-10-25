// SPDX-License-Identifier: GPL-2.0

//! File descriptor table.
//!
//! Manages open file descriptors, `FD_CLOEXEC` flags, descriptor duplication,
//! and close-on-exec lifecycle actions.

use alloc::{sync::Arc, vec::Vec};
use bitflags::bitflags;

use crate::drivers::char::tty::create_console_file;
use crate::fs::vfs::{File, OpenFlags};
use crate::uapi::errno::Result;

bitflags! {
    /// File descriptor flags.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
    pub struct FdFlags: u32 {
        /// Close the descriptor during `execve(2)`.
        const FD_CLOEXEC = 1 << 0;
    }
}

/// An entry in the file descriptor table.
#[derive(Clone)]
pub struct FdEntry {
    pub file: Arc<File>,
    pub flags: FdFlags,
}

/// Process file descriptor table.
pub struct Filedesc {
    pub files: Vec<Option<FdEntry>>,
    pub cmask: u32,
    pub max_files: usize,
}

impl Filedesc {
    /// Default maximum open files per process.
    pub const DEFAULT_MAX_FILES: usize = 1024;

    /// Creates an empty file descriptor table.
    pub fn new() -> Self {
        Self {
            files: Vec::new(),
            cmask: 0o022,
            max_files: Self::DEFAULT_MAX_FILES,
        }
    }

    /// Creates a table with stdin (0) as readable ConsoleFile, stdout (1) and stderr (2) as writable ConsoleFile.
    pub fn with_stdio() -> Self {
        let mut table = Self::new();
        let stdin = create_console_file(OpenFlags::READ);
        let stdout = create_console_file(OpenFlags::WRITE);
        let stderr = create_console_file(OpenFlags::WRITE);
        let _ = table.alloc_fd(stdin, FdFlags::empty());
        let _ = table.alloc_fd(stdout, FdFlags::empty());
        let _ = table.alloc_fd(stderr, FdFlags::empty());
        table
    }

    /// Allocates the lowest available file descriptor for `file`.
    pub fn alloc_fd(&mut self, file: Arc<File>, flags: FdFlags) -> Result<i32> {
        self.alloc_fd_at(0, file, flags)
    }

    /// Allocates the lowest available file descriptor greater than or equal to `min_fd`.
    pub fn alloc_fd_at(&mut self, min_fd: i32, file: Arc<File>, flags: FdFlags) -> Result<i32> {
        if min_fd < 0 || min_fd as usize >= self.max_files {
            crate::return_errno!(EINVAL, "invalid minimum file descriptor {min_fd}");
        }
        let min_idx = min_fd as usize;
        if self.files.len() <= min_idx {
            self.files.resize(min_idx, None);
        }
        for (idx, slot) in self.files.iter_mut().enumerate().skip(min_idx) {
            if slot.is_none() {
                *slot = Some(FdEntry { file, flags });
                return Ok(idx as i32);
            }
        }
        if self.files.len() >= self.max_files {
            crate::return_errno!(EMFILE, "too many open files");
        }
        let new_fd = self.files.len() as i32;
        self.files.push(Some(FdEntry { file, flags }));
        Ok(new_fd)
    }

    /// Retrieves the file referenced by `fd`.
    pub fn get(&self, fd: i32) -> Result<Arc<File>> {
        if fd < 0 {
            crate::return_errno!(EBADF, "negative file descriptor {fd}");
        }
        let idx = fd as usize;
        match self.files.get(idx) {
            Some(Some(entry)) => Ok(Arc::clone(&entry.file)),
            _ => crate::return_errno!(EBADF, "file descriptor {fd} is not open"),
        }
    }

    /// Retrieves descriptor flags for `fd`.
    pub fn get_flags(&self, fd: i32) -> Result<FdFlags> {
        if fd < 0 {
            crate::return_errno!(EBADF, "negative file descriptor {fd}");
        }
        let idx = fd as usize;
        match self.files.get(idx) {
            Some(Some(entry)) => Ok(entry.flags),
            _ => crate::return_errno!(EBADF, "file descriptor {fd} is not open"),
        }
    }

    /// Sets descriptor flags for `fd`.
    pub fn set_flags(&mut self, fd: i32, flags: FdFlags) -> Result<()> {
        if fd < 0 {
            crate::return_errno!(EBADF, "negative file descriptor {fd}");
        }
        let idx = fd as usize;
        match self.files.get_mut(idx) {
            Some(Some(entry)) => {
                entry.flags = flags;
                Ok(())
            }
            _ => crate::return_errno!(EBADF, "file descriptor {fd} is not open"),
        }
    }

    /// Closes a file descriptor.
    pub fn close(&mut self, fd: i32) -> Result<()> {
        if fd < 0 {
            crate::return_errno!(EBADF, "negative file descriptor {fd}");
        }
        let idx = fd as usize;
        if idx >= self.files.len() || self.files[idx].is_none() {
            crate::return_errno!(EBADF, "file descriptor {fd} is not open");
        }
        let entry = self.files[idx].take();
        if let Some(entry) = entry {
            release_entry(entry);
        }
        Ok(())
    }

    /// Closes or sets `FD_CLOEXEC` on a range of file descriptors from `first` to `last` (`close_range(2)`).
    pub fn close_range(&mut self, first: u32, last: u32, cloexec: bool) -> Result<()> {
        if first > last {
            crate::return_errno!(EINVAL, "close_range: first ({first}) > last ({last})");
        }
        let start = first as usize;
        let end = (last as usize).min(self.files.len().saturating_sub(1));
        if start >= self.files.len() {
            return Ok(());
        }

        for idx in start..=end {
            if cloexec {
                if let Some(entry) = &mut self.files[idx] {
                    entry.flags.insert(FdFlags::FD_CLOEXEC);
                }
            } else if let Some(entry) = self.files[idx].take() {
                release_entry(entry);
            }
        }
        Ok(())
    }


    /// Duplicates `oldfd` onto `newfd` (`dup2(2)`).
    pub fn dup2(&mut self, oldfd: i32, newfd: i32, flags: FdFlags) -> Result<i32> {
        if oldfd < 0 || newfd < 0 || newfd as usize >= self.max_files {
            crate::return_errno!(EBADF, "invalid file descriptor for dup2");
        }
        if oldfd == newfd {
            let _ = self.get(oldfd)?;
            return Ok(newfd);
        }
        let file = self.get(oldfd)?;
        let new_idx = newfd as usize;
        if new_idx >= self.files.len() {
            self.files.resize(new_idx + 1, None);
        } else if let Some(old_entry) = self.files[new_idx].take() {
            release_entry(old_entry);
        }
        self.files[new_idx] = Some(FdEntry { file, flags });
        Ok(newfd)
    }

    /// Closes all file descriptors that have `FD_CLOEXEC` set.
    pub fn close_on_exec(&mut self) {
        for slot in self.files.iter_mut() {
            if slot
                .as_ref()
                .is_some_and(|e| e.flags.contains(FdFlags::FD_CLOEXEC))
            {
                let e = slot.take().unwrap();
                release_entry(e);
            }
        }
    }

    /// Closes all open file descriptors.
    pub fn close_all(&mut self) {
        for slot in self.files.iter_mut() {
            if let Some(entry) = slot.take() {
                release_entry(entry);
            }
        }
        self.files.clear();
    }

    /// Clones the descriptor table for `fork(2)`.
    pub fn clone_table(&self) -> Self {
        Self {
            files: self.files.clone(),
            cmask: self.cmask,
            max_files: self.max_files,
        }
    }
}

/// Drops a descriptor entry, running the file's close hook only if this was the
/// last reference. Files shared through `dup`/`fork` must stay open for the
/// other holders; anything left over is cleaned up by the owner's `Drop`.
fn release_entry(entry: FdEntry) {
    if Arc::strong_count(&entry.file) == 1 {
        let _ = entry.file.close();
    }
}

impl Default for Filedesc {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(ktest)]
mod tests {
    use super::*;
    use ostd::prelude::ktest;

    #[ktest]
    fn filedesc_close_range_semantics() {
        let mut table = Filedesc::with_stdio();
        let stdin = table.get(0).unwrap();
        let fd3 = table.alloc_fd(Arc::clone(&stdin), FdFlags::empty()).unwrap();
        let fd4 = table.alloc_fd(Arc::clone(&stdin), FdFlags::empty()).unwrap();
        let fd5 = table.alloc_fd(Arc::clone(&stdin), FdFlags::empty()).unwrap();
        assert_eq!(fd3, 3);
        assert_eq!(fd4, 4);
        assert_eq!(fd5, 5);

        // Close range 3..=4
        table.close_range(3, 4, false).unwrap();
        assert!(table.get(3).is_err());
        assert!(table.get(4).is_err());
        assert!(table.get(5).is_ok());

        // Cloexec range 5..=5
        table.close_range(5, 5, true).unwrap();
        assert!(table.get_flags(5).unwrap().contains(FdFlags::FD_CLOEXEC));
    }
}

