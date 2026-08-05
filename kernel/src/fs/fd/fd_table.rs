use crate::fs::vfs::{FileOps, InodeOps, SeekFrom};
use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use ostd::Error;
use ostd::sync::SpinLock;

/// An open file description in the VFS.
/// Contains the underlying `FileOps` implementation, the current offset,
/// and flags used to open the file.
pub struct OpenFile {
    /// Associated VFS inode if applicable.
    pub inode: Option<Arc<dyn InodeOps>>,
    /// The VFS file operations.
    pub file_ops: Box<dyn FileOps>,
    /// Current seek offset.
    pub offset: usize,
    /// File status and access flags.
    pub flags: u32,
    /// CLOEXEC flag.
    pub cloexec: bool,
}

impl OpenFile {
    pub fn new(file_ops: Box<dyn FileOps>, flags: u32) -> Self {
        Self {
            inode: None,
            file_ops,
            offset: 0,
            flags,
            cloexec: (flags & 0x80000) != 0,
        }
    }

    pub fn with_inode(inode: Arc<dyn InodeOps>, file_ops: Box<dyn FileOps>, flags: u32) -> Self {
        Self {
            inode: Some(inode),
            file_ops,
            offset: 0,
            flags,
            cloexec: (flags & 0x80000) != 0,
        }
    }
}

impl FileOps for OpenFile {
    fn read(&mut self, buf: &mut [u8], offset: &mut usize) -> Result<usize, Error> {
        self.file_ops.read(buf, offset)
    }

    fn write(&mut self, buf: &[u8], offset: &mut usize) -> Result<usize, Error> {
        self.file_ops.write(buf, offset)
    }

    fn seek(&mut self, pos: SeekFrom, offset: &mut usize) -> Result<usize, Error> {
        self.file_ops.seek(pos, offset)
    }

    fn readdir(&mut self) -> Result<alloc::vec::Vec<crate::fs::vfs::DirEntry>, Error> {
        self.file_ops.readdir()
    }

    fn as_any(&self) -> Option<&dyn core::any::Any> {
        self.file_ops.as_any()
    }

    fn ioctl(
        &mut self,
        cmd: usize,
        arg: usize,
        vm: &crate::vm::vma::VmaManager,
    ) -> Result<usize, Error> {
        self.file_ops.ioctl(cmd, arg, vm)
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

    pub fn with_inode(inode: Arc<dyn InodeOps>, file_ops: Box<dyn FileOps>, flags: u32) -> Self {
        Self {
            open_file: Arc::new(SpinLock::new(OpenFile::with_inode(inode, file_ops, flags))),
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
        let fd_entry = FileDescriptor::with_inode(dentry.inode.clone(), file_ops, flags);

        let fd = self.alloc_fd(0)?;
        self.fds.insert(fd, fd_entry);
        Ok(fd)
    }

    /// Insert a custom inode and file_ops into the descriptor table.
    pub fn insert_custom(
        &mut self,
        inode: Arc<dyn InodeOps>,
        file_ops: Box<dyn FileOps>,
        flags: u32,
        mode: u32,
    ) -> Result<i32, Error> {
        let _ = mode;
        let fd = self.alloc_fd(0)?;
        let fd_entry = FileDescriptor::with_inode(inode, file_ops, flags);
        self.fds.insert(fd, fd_entry);
        Ok(fd)
    }

    pub fn insert_pipe_reader(&mut self, ops: Box<dyn FileOps>) -> Result<i32, Error> {
        let fd = self.alloc_fd(0)?;
        let fd_entry = FileDescriptor::new(ops, 0);
        self.fds.insert(fd, fd_entry);
        Ok(fd)
    }

    pub fn insert_pipe_writer(&mut self, ops: Box<dyn FileOps>) -> Result<i32, Error> {
        let fd = self.alloc_fd(0)?;
        let fd_entry = FileDescriptor::new(ops, 0);
        self.fds.insert(fd, fd_entry);
        Ok(fd)
    }

    pub fn insert_at_or_above(
        &mut self,
        file: FileDescriptor,
        min_fd: i32,
        cloexec: bool,
    ) -> Result<i32, Error> {
        let fd = self.alloc_fd(min_fd.max(0))?;
        let new_entry = file.clone();
        new_entry.open_file.lock().cloexec = cloexec;
        self.fds.insert(fd, new_entry);
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

    /// Duplicate `oldfd` onto `newfd` with flags. Fails if `oldfd == newfd`.
    pub fn dup3(&mut self, oldfd: i32, newfd: i32, _flags: u32) -> Result<i32, Error> {
        if newfd < 0 || oldfd == newfd {
            return Err(Error::InvalidArgs);
        }
        let fd_entry = self.fds.get(&oldfd).cloned().ok_or(Error::InvalidArgs)?;
        self.fds.insert(newfd, fd_entry);
        Ok(newfd)
    }

    /// Read up to `buf.len()` bytes from file descriptor `fd` into `buf`.
    pub fn read(&self, fd: i32, buf: &mut [u8]) -> Result<usize, Error> {
        let fd_entry = self.fds.get(&fd).cloned().ok_or(Error::InvalidArgs)?;
        let mut open_file = fd_entry.open_file.lock();
        let initial_offset = open_file.offset;
        let OpenFile {
            file_ops, offset, ..
        } = &mut *open_file;
        let bytes_read = file_ops.read(buf, offset)?;
        if *offset == initial_offset && bytes_read > 0 {
            *offset += bytes_read;
        }
        Ok(bytes_read)
    }

    /// Write up to `buf.len()` bytes from `buf` to file descriptor `fd`.
    pub fn write(&self, fd: i32, buf: &[u8]) -> Result<usize, Error> {
        let fd_entry = self.fds.get(&fd).cloned().ok_or(Error::InvalidArgs)?;
        let mut open_file = fd_entry.open_file.lock();
        let initial_offset = open_file.offset;
        let OpenFile {
            file_ops, offset, ..
        } = &mut *open_file;
        let bytes_written = file_ops.write(buf, offset)?;
        if *offset == initial_offset && bytes_written > 0 {
            *offset += bytes_written;
        }
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

    pub fn get(&self, fd: i32) -> Result<FileDescriptor, Error> {
        self.get_fd(fd)
    }

    pub fn is_cloexec(&self, fd: i32) -> bool {
        if let Ok(entry) = self.get_fd(fd) {
            entry.open_file.lock().cloexec
        } else {
            false
        }
    }

    pub fn set_cloexec(&mut self, fd: i32, cloexec: bool) -> Result<(), Error> {
        if let Ok(entry) = self.get_fd(fd) {
            entry.open_file.lock().cloexec = cloexec;
            Ok(())
        } else {
            Err(Error::InvalidArgs)
        }
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

pub struct FdFileRef {
    pub inode: Arc<dyn InodeOps>,
    pub ops: Box<dyn FileOps>,
    pub flags: u32,
}

impl FileDescriptor {
    pub fn get_file_ref(&self) -> FdFileRef {
        let open = self.open_file.lock();
        let dummy_inode = Arc::new(DummyInode);
        FdFileRef {
            inode: open.inode.clone().unwrap_or(dummy_inode),
            ops: open
                .file_ops
                .as_ref()
                .as_any()
                .map(|_| Box::new(DummyOps) as Box<dyn FileOps>)
                .unwrap_or_else(|| Box::new(DummyOps)),
            flags: open.flags,
        }
    }
}

pub struct DummyInode;
impl InodeOps for DummyInode {
    fn lookup(&self, _: &str) -> Result<Arc<dyn InodeOps>, Error> {
        Err(Error::InvalidArgs)
    }
    fn create(&self, _: &str, _: u32) -> Result<Arc<dyn InodeOps>, Error> {
        Err(Error::InvalidArgs)
    }
    fn mkdir(&self, _: &str, _: u32) -> Result<Arc<dyn InodeOps>, Error> {
        Err(Error::InvalidArgs)
    }
    fn symlink(&self, _: &str, _: &str) -> Result<Arc<dyn InodeOps>, Error> {
        Err(Error::InvalidArgs)
    }
    fn metadata(&self) -> Result<crate::fs::vfs::Metadata, Error> {
        Ok(crate::fs::vfs::Metadata {
            size: 0,
            file_type: crate::fs::vfs::FileType::Regular,
            mode: 0o600,
            uid: 0,
            gid: 0,
            inode_num: 1,
            nlink: 1,
        })
    }
    fn read_link(&self) -> Result<alloc::string::String, Error> {
        Err(Error::InvalidArgs)
    }
    fn open(&self, _: u32) -> Result<Box<dyn FileOps>, Error> {
        Ok(Box::new(DummyOps))
    }
    fn as_any(&self) -> &dyn core::any::Any {
        self
    }
    fn unlink(&self, _: &str) -> Result<(), Error> {
        Err(Error::InvalidArgs)
    }
    fn rename(&self, _: &str, _: &Arc<dyn InodeOps>, _: &str) -> Result<(), Error> {
        Err(Error::InvalidArgs)
    }
}

pub struct DummyOps;
impl FileOps for DummyOps {
    fn read(&mut self, _: &mut [u8], _: &mut usize) -> Result<usize, Error> {
        Ok(0)
    }
    fn write(&mut self, _: &[u8], _: &mut usize) -> Result<usize, Error> {
        Ok(0)
    }
    fn seek(&mut self, _: SeekFrom, _: &mut usize) -> Result<usize, Error> {
        Err(Error::InvalidArgs)
    }
    fn readdir(&mut self) -> Result<alloc::vec::Vec<crate::fs::vfs::DirEntry>, Error> {
        Err(Error::InvalidArgs)
    }
}
