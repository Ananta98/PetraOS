use crate::fs::vfs::{FileOps, SeekFrom};
use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use ostd::Error;
use ostd::sync::SpinLock;

/// An open file description in the VFS.
/// Contains the underlying `FileOps` implementation, the current offset,
/// and flags used to open the file.
pub struct OpenFile {
    /// The VFS file operations.
    pub file_ops: Box<dyn FileOps>,
    /// Current seek offset.
    pub offset: usize,
    /// File status and access flags.
    pub flags: u32,
}

impl OpenFile {
    pub fn new(file_ops: Box<dyn FileOps>, flags: u32) -> Self {
        Self {
            file_ops,
            offset: 0,
            flags,
        }
    }
}

/// A file descriptor entry in the process file descriptor table.
/// It wraps a shared reference to an `OpenFile` description, allowing sharing
/// of the file offset/status flags (e.g. after a `dup`/`dup2`).
#[derive(Clone)]
pub struct FileDescriptor {
    pub open_file: Arc<SpinLock<OpenFile>>,
}

impl FileDescriptor {
    pub fn new(file_ops: Box<dyn FileOps>, flags: u32) -> Self {
        Self {
            open_file: Arc::new(SpinLock::new(OpenFile::new(file_ops, flags))),
        }
    }
}

/// A file descriptor table for a process.
#[derive(Clone)]
pub struct FdTable {
    fds: BTreeMap<i32, FileDescriptor>,
}

impl FdTable {
    /// Create a new, empty file descriptor table.
    pub fn new() -> Self {
        Self {
            fds: BTreeMap::new(),
        }
    }

    /// Open a file at `path` with `flags` and `mode`, and allocate a new file descriptor.
    pub fn open(&mut self, path: &str, flags: u32, mode: u32) -> Result<i32, Error> {
        let dentry = match crate::fs::vfs::resolve_path(path) {
            Ok(d) => d,
            Err(e) => {
                if (flags & 0x40) != 0 {
                    // O_CREAT
                    let (parent_path, filename) = Self::split_path(path);
                    let parent_dentry = crate::fs::vfs::resolve_path(parent_path)?;
                    parent_dentry.inode.create(filename, mode)?;
                    crate::fs::vfs::resolve_path(path)?
                } else {
                    return Err(e);
                }
            }
        };
        let file_ops = dentry.inode.open(flags)?;
        let fd_entry = FileDescriptor::new(file_ops, flags);

        let fd = self.alloc_fd(0)?;
        self.fds.insert(fd, fd_entry);
        Ok(fd)
    }

    /// Close the file descriptor `fd`.
    pub fn close(&mut self, fd: i32) -> Result<(), Error> {
        if self.fds.remove(&fd).is_some() {
            Ok(())
        } else {
            Err(Error::InvalidArgs)
        }
    }

    /// Duplicate the file descriptor `oldfd`, returning a new descriptor.
    pub fn dup(&mut self, oldfd: i32) -> Result<i32, Error> {
        let fd_entry = self.fds.get(&oldfd).cloned().ok_or(Error::InvalidArgs)?;
        let new_fd = self.alloc_fd(0)?;
        self.fds.insert(new_fd, fd_entry);
        Ok(new_fd)
    }

    /// Duplicate `oldfd` onto `newfd`. If `newfd` is already open, it is silently closed.
    pub fn dup2(&mut self, oldfd: i32, newfd: i32) -> Result<i32, Error> {
        if newfd < 0 {
            return Err(Error::InvalidArgs);
        }
        let fd_entry = self.fds.get(&oldfd).cloned().ok_or(Error::InvalidArgs)?;
        if oldfd == newfd {
            return Ok(newfd);
        }
        self.fds.insert(newfd, fd_entry);
        Ok(newfd)
    }

    /// Read up to `buf.len()` bytes from file descriptor `fd` into `buf`.
    pub fn read(&self, fd: i32, buf: &mut [u8]) -> Result<usize, Error> {
        let fd_entry = self.fds.get(&fd).cloned().ok_or(Error::InvalidArgs)?;
        let mut open_file = fd_entry.open_file.lock();
        let OpenFile {
            file_ops, offset, ..
        } = &mut *open_file;
        let bytes_read = file_ops.read(buf, offset)?;
        Ok(bytes_read)
    }

    /// Write up to `buf.len()` bytes from `buf` to file descriptor `fd`.
    pub fn write(&self, fd: i32, buf: &[u8]) -> Result<usize, Error> {
        let fd_entry = self.fds.get(&fd).cloned().ok_or(Error::InvalidArgs)?;
        let mut open_file = fd_entry.open_file.lock();
        let OpenFile {
            file_ops, offset, ..
        } = &mut *open_file;
        let bytes_written = file_ops.write(buf, offset)?;
        Ok(bytes_written)
    }

    /// Reposition the read/write offset of the file descriptor `fd`.
    pub fn lseek(&self, fd: i32, offset: isize, whence: i32) -> Result<usize, Error> {
        let pos = match whence {
            0 => {
                if offset < 0 {
                    return Err(Error::InvalidArgs);
                }
                SeekFrom::Start(offset as usize)
            }
            1 => SeekFrom::Current(offset),
            2 => SeekFrom::End(offset),
            _ => return Err(Error::InvalidArgs),
        };
        let fd_entry = self.fds.get(&fd).cloned().ok_or(Error::InvalidArgs)?;
        let mut open_file = fd_entry.open_file.lock();
        let OpenFile {
            file_ops,
            offset: file_offset,
            ..
        } = &mut *open_file;
        let new_offset = file_ops.seek(pos, file_offset)?;
        Ok(new_offset)
    }

    fn split_path(path: &str) -> (&str, &str) {
        if let Some(pos) = path.rfind('/') {
            let (parent, file) = path.split_at(pos);
            let parent = if parent.is_empty() { "/" } else { parent };
            (parent, &file[1..])
        } else {
            (".", path)
        }
    }

    /// Get a clone of the file descriptor entry for a given fd.
    pub fn get_fd(&self, fd: i32) -> Result<FileDescriptor, Error> {
        self.fds.get(&fd).cloned().ok_or(Error::InvalidArgs)
    }

    /// Insert an open file descriptor entry into the table at `fd`.
    pub fn insert(&mut self, fd: i32, fd_entry: FileDescriptor) {
        self.fds.insert(fd, fd_entry);
    }

    /// Allocate a free file descriptor starting at `start`.
    pub fn alloc_fd(&self, start: i32) -> Result<i32, Error> {
        let mut fd = start;
        while self.fds.contains_key(&fd) {
            fd += 1;
            if fd < 0 {
                return Err(Error::NotEnoughResources);
            }
        }
        Ok(fd)
    }

    /// Return a sorted list of all currently open file descriptor numbers.
    ///
    /// Used by procfs to populate `/proc/<pid>/fd/`.
    pub fn list_fds(&self) -> alloc::vec::Vec<i32> {
        self.fds.keys().copied().collect()
    }
}
