//! Anonymous Inter-Process Communication (IPC) Pipe Subsystem.

use crate::fs::File;
use crate::fs::vfs::dentry::Dentry;
use crate::fs::vfs::types::{
    FileOps, Inode, InodeOps, InodeType, O_RDONLY, O_WRONLY, SeekWhence, Stat, VfsError,
};
use crate::sync::{Mutex, WaitQueue};
use crate::syscalls::fs::{POLLERR, POLLOUT};
use alloc::collections::VecDeque;
use alloc::sync::Arc;
use core::sync::atomic::AtomicU64;

/// Default POSIX pipe buffer capacity (64 KB).
pub const PIPE_BUFFER_CAPACITY: usize = 65536;

/// Shared thread-safe in-memory ring buffer for anonymous pipes.
pub struct PipeInner {
    buffer: VecDeque<u8>,
    capacity: usize,
    readers: usize,
    writers: usize,
}

impl PipeInner {
    pub fn new(capacity: usize) -> Self {
        Self {
            buffer: VecDeque::with_capacity(core::cmp::min(capacity, 4096)),
            capacity,
            readers: 1,
            writers: 1,
        }
    }
}

/// Shared thread-safe anonymous pipe representation with event-driven wait queues.
pub struct Pipe {
    pub inner: Mutex<PipeInner>,
    pub read_wait: WaitQueue,
    pub write_wait: WaitQueue,
}

impl Pipe {
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: Mutex::new(PipeInner::new(capacity)),
            read_wait: WaitQueue::new(),
            write_wait: WaitQueue::new(),
        }
    }
}

/// Read end file operations for an anonymous pipe.
pub struct PipeReadFileOps {
    pipe: Arc<Pipe>,
    nonblocking: bool,
}

impl FileOps for PipeReadFileOps {
    fn read(&self, _offset: usize, buf: &mut [u8]) -> Result<usize, VfsError> {
        self.read_with_flags(_offset, buf, 0)
    }

    fn read_with_flags(
        &self,
        _offset: usize,
        buf: &mut [u8],
        flags: u32,
    ) -> Result<usize, VfsError> {
        if buf.is_empty() {
            return Ok(0);
        }
        let nonblocking = self.nonblocking || (flags & crate::fs::vfs::types::O_NONBLOCK) != 0;
        loop {
            let mut inner = self.pipe.inner.lock();
            if !inner.buffer.is_empty() {
                let to_read = core::cmp::min(buf.len(), inner.buffer.len());
                for byte in buf.iter_mut().take(to_read) {
                    *byte = inner.buffer.pop_front().unwrap_or(0);
                }
                drop(inner);
                // Wake any writers blocked waiting for buffer capacity
                self.pipe.write_wait.wake_one();
                return Ok(to_read);
            }

            // Buffer is empty: if no writers remain, return EOF (0 bytes)
            if inner.writers == 0 {
                return Ok(0);
            }

            if nonblocking {
                return Err(VfsError::WouldBlock);
            }

            // Event-driven sleep until a writer pushes data or closes the write end
            self.pipe.read_wait.wait_with(|| drop(inner));
        }
    }

    fn poll_events(&self, events: i16) -> i16 {
        use crate::syscalls::fs::{POLLHUP, POLLIN};
        let inner = self.pipe.inner.lock();
        let mut revents = 0;
        if (events & POLLIN) != 0 && (!inner.buffer.is_empty() || inner.writers == 0) {
            revents |= POLLIN;
        }
        // Read end is never writable; report hangup once all writers are gone
        // so pollers waiting on HUP/ERR wake up instead of blocking forever.
        if inner.writers == 0 {
            revents |= POLLHUP;
        }
        revents
    }

    fn lseek(&self, _offset: i64, _whence: SeekWhence) -> Result<usize, VfsError> {
        Err(VfsError::NotSupported)
    }

    fn stat(&self) -> Result<Stat, VfsError> {
        let inner = self.pipe.inner.lock();
        Ok(Stat {
            ino: 0,
            mode: 0o010600, // S_IFIFO | rw-------
            nlink: 1,
            size: inner.buffer.len() as u64,
            blksize: 4096,
            ..Default::default()
        })
    }
}

impl Drop for PipeReadFileOps {
    fn drop(&mut self) {
        let mut inner = self.pipe.inner.lock();
        if inner.readers > 0 {
            inner.readers -= 1;
        }
        let readers = inner.readers;
        drop(inner);
        if readers == 0 {
            // Wake any blocked writers so they receive SIGPIPE/EPIPE
            self.pipe.write_wait.wake_all();
        }
    }
}

/// Write end file operations for an anonymous pipe.
pub struct PipeWriteFileOps {
    pipe: Arc<Pipe>,
    nonblocking: bool,
}

impl FileOps for PipeWriteFileOps {
    fn write(&self, _offset: usize, buf: &[u8]) -> Result<usize, VfsError> {
        self.write_with_flags(_offset, buf, 0)
    }

    fn write_with_flags(&self, _offset: usize, buf: &[u8], flags: u32) -> Result<usize, VfsError> {
        if buf.is_empty() {
            return Ok(0);
        }

        let nonblocking = self.nonblocking || (flags & crate::fs::vfs::types::O_NONBLOCK) != 0;
        let mut total_written = 0;

        while total_written < buf.len() {
            let mut inner = self.pipe.inner.lock();

            // Broken pipe: no readers remain
            if inner.readers == 0 {
                if let Some(proc_arc) = crate::proc::current_process() {
                    let mut proc = proc_arc.lock();
                    let _ = proc.send_signal(13); // SIGPIPE
                }
                return Err(VfsError::PermissionDenied); // EPIPE
            }

            let available = inner.capacity.saturating_sub(inner.buffer.len());
            if available > 0 {
                let remaining = buf.len() - total_written;
                let chunk_size = core::cmp::min(remaining, available);
                for &byte in &buf[total_written..total_written + chunk_size] {
                    inner.buffer.push_back(byte);
                }
                total_written += chunk_size;
                drop(inner);
                // Wake any readers waiting for incoming data
                self.pipe.read_wait.wake_one();

                if total_written == buf.len() {
                    return Ok(total_written);
                }
                continue;
            }

            if nonblocking {
                if total_written > 0 {
                    return Ok(total_written);
                }
                return Err(VfsError::WouldBlock);
            }

            // Buffer is full: event-driven sleep until a reader consumes data
            self.pipe.write_wait.wait_with(|| drop(inner));
        }

        Ok(total_written)
    }

    fn poll_events(&self, events: i16) -> i16 {
        let inner = self.pipe.inner.lock();
        let mut revents = 0;
        // Write end is never readable; report error once all readers are gone
        // so pollers don't block forever on a broken pipe.
        if inner.readers == 0 {
            revents |= POLLERR;
        }
        if (events & POLLOUT) != 0 && (inner.buffer.len() < inner.capacity || inner.readers == 0) {
            revents |= POLLOUT;
        }
        revents
    }

    fn lseek(&self, _offset: i64, _whence: SeekWhence) -> Result<usize, VfsError> {
        Err(VfsError::NotSupported)
    }

    fn stat(&self) -> Result<Stat, VfsError> {
        let inner = self.pipe.inner.lock();
        Ok(Stat {
            ino: 0,
            mode: 0o010600, // S_IFIFO | rw-------
            nlink: 1,
            size: inner.buffer.len() as u64,
            blksize: 4096,
            ..Default::default()
        })
    }
}

impl Drop for PipeWriteFileOps {
    fn drop(&mut self) {
        let mut inner = self.pipe.inner.lock();
        if inner.writers > 0 {
            inner.writers -= 1;
        }
        let writers = inner.writers;
        drop(inner);
        if writers == 0 {
            // Wake any blocked readers so they receive EOF
            self.pipe.read_wait.wake_all();
        }
    }
}

/// Dummy InodeOps implementation for pipe descriptors.
struct PipeInodeOps;
impl InodeOps for PipeInodeOps {}

/// Create a new connected anonymous pipe pair `(read_file, write_file)`.
pub fn create_pipe(nonblocking: bool) -> Result<(Arc<File>, Arc<File>), VfsError> {
    static NEXT_PIPE_INO: AtomicU64 = AtomicU64::new(100_000);
    let ino = NEXT_PIPE_INO.fetch_add(1, core::sync::atomic::Ordering::Relaxed);

    let pipe = Arc::new(Pipe::new(PIPE_BUFFER_CAPACITY));

    let read_ops = Arc::new(PipeReadFileOps {
        pipe: pipe.clone(),
        nonblocking,
    });

    let write_ops = Arc::new(PipeWriteFileOps { pipe, nonblocking });

    let inode = Arc::new(Inode {
        ino,
        inode_type: InodeType::Fifo,
        ops: Arc::new(PipeInodeOps),
    });

    let dentry = Arc::new(Dentry {
        name: alloc::string::String::from("pipe:[anon]"),
        inode,
        parent: Mutex::new(None),
        children: Mutex::new(alloc::collections::BTreeMap::new()),
    });

    let read_file = Arc::new(File::new(dentry.clone(), O_RDONLY, read_ops));
    let write_file = Arc::new(File::new(dentry, O_WRONLY, write_ops));

    Ok((read_file, write_file))
}
