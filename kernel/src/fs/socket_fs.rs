//! VFS File Operations and Inode Backing for Network and Unix Sockets
//!
//! Exposes sockets as first-class POSIX file descriptions so that standard
//! `read()`, `write()`, `poll()`, `select()`, `ioctl()`, and `close()` operations work seamlessly.

use alloc::sync::Arc;

use crate::fs::File;
use crate::fs::vfs::dentry::Dentry;
use crate::fs::vfs::types::{FileOps, Inode, InodeOps, InodeType, Stat, VfsError};
use crate::net::socket::Socket;
use crate::sync::spinlock::Spinlock;

/// Dummy inode operations for socket file descriptions.
struct SocketInodeOps;

impl InodeOps for SocketInodeOps {
    fn stat(&self) -> Result<Stat, VfsError> {
        Ok(Stat {
            ino: 0,
            mode: 0o140777, // S_IFSOCK | rwxrwxrwx
            nlink: 1,
            size: 0,
            blksize: 4096,
            ..Default::default()
        })
    }
}

/// VFS File operations implementation backed by a network or Unix domain socket.
pub struct SocketFileOps {
    pub socket: Arc<Spinlock<Socket>>,
}

impl FileOps for SocketFileOps {
    fn read(&self, _offset: usize, buf: &mut [u8]) -> Result<usize, VfsError> {
        let mut sock = self.socket.lock();
        sock.recv(buf, 0).map_err(|err| match err {
            crate::syscalls::SyscallError::EAGAIN => VfsError::InvalidInput,
            crate::syscalls::SyscallError::EINTR => VfsError::Interrupted,
            _ => VfsError::InvalidInput,
        })
    }

    fn write(&self, _offset: usize, buf: &[u8]) -> Result<usize, VfsError> {
        let mut sock = self.socket.lock();
        sock.send(buf, 0).map_err(|err| match err {
            crate::syscalls::SyscallError::EAGAIN => VfsError::InvalidInput,
            crate::syscalls::SyscallError::EINTR => VfsError::Interrupted,
            _ => VfsError::InvalidInput,
        })
    }

    fn stat(&self) -> Result<Stat, VfsError> {
        Ok(Stat {
            ino: 0,
            mode: 0o140777, // S_IFSOCK | rwxrwxrwx
            nlink: 1,
            size: 0,
            blksize: 4096,
            ..Default::default()
        })
    }

    fn ioctl(&self, cmd: u64, arg: usize) -> Result<usize, VfsError> {
        match cmd {
            0x5421 /* FIONBIO */ => {
                let val = if arg != 0 {
                    // SAFETY: Read user pointer for FIONBIO boolean flag
                    unsafe { *(arg as *const i32) }
                } else {
                    0
                };
                let mut sock = self.socket.lock();
                match &mut *sock {
                    Socket::Tcp(s) => s.nonblocking = val != 0,
                    Socket::Udp(s) => s.nonblocking = val != 0,
                    Socket::Raw(s) => s.nonblocking = val != 0,
                    Socket::Unix(u) => u.lock().nonblocking = val != 0,
                }
                Ok(0)
            }
            0x541B /* FIONREAD */ => {
                // Return 0 pending bytes as fallback
                if arg != 0 {
                    unsafe {
                        *(arg as *mut i32) = 0;
                    }
                }
                Ok(0)
            }
            _ => Err(VfsError::NotSupported),
        }
    }

    fn poll_events(&self, events: i16) -> i16 {
        const POLLIN: i16 = 0x0001;
        const POLLOUT: i16 = 0x0004;

        let sock = self.socket.lock();
        let mut revents = 0;
        if (events & POLLIN) != 0 && sock.poll_read_ready() {
            revents |= POLLIN;
        }
        if (events & POLLOUT) != 0 && sock.poll_write_ready() {
            revents |= POLLOUT;
        }
        revents
    }

    fn as_socket(&self) -> Option<Arc<Spinlock<Socket>>> {
        Some(self.socket.clone())
    }
}

/// Wrap an active socket in a VFS `File` description.
pub fn create_socket_file(socket: Arc<Spinlock<Socket>>, flags: u32) -> Arc<File> {
    let inode = Arc::new(Inode {
        ino: 0,
        inode_type: InodeType::File,
        ops: Arc::new(SocketInodeOps),
    });

    let dentry = Arc::new(Dentry::new(alloc::string::String::from("[socket]"), inode));
    let ops = Arc::new(SocketFileOps { socket });

    Arc::new(File::new(dentry, flags, ops))
}
