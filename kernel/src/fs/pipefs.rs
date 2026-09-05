//! Anonymous Inter-Process Communication (IPC) Pipe Subsystem.

use crate::fs::File;
use crate::fs::vfs::dentry::Dentry;
use crate::fs::vfs::types::{
    FileOps, Inode, InodeOps, InodeType, O_RDONLY, O_WRONLY, Stat, VfsError,
};
use crate::proc::thread::Thread;
use crate::sync::Mutex;
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

/// Read end file operations for an anonymous pipe.
pub struct PipeReadFileOps {
    pipe: Arc<Mutex<PipeInner>>,
    nonblocking: bool,
}

impl FileOps for PipeReadFileOps {
    fn read(&self, _offset: usize, buf: &mut [u8]) -> Result<usize, VfsError> {
        if buf.is_empty() {
            return Ok(0);
        }

        loop {
            let mut pipe = self.pipe.lock();
            if !pipe.buffer.is_empty() {
                let to_read = core::cmp::min(buf.len(), pipe.buffer.len());
                for byte in buf.iter_mut().take(to_read) {
                    *byte = pipe.buffer.pop_front().unwrap_or(0);
                }
                return Ok(to_read);
            }

            // Buffer is empty: if no writers remain, return EOF (0 bytes)
            if pipe.writers == 0 {
                return Ok(0);
            }

            if self.nonblocking {
                return Err(VfsError::InvalidInput); // EAGAIN / WouldBlock
            }

            drop(pipe);
            Thread::yield_cpu();
        }
    }

    fn stat(&self) -> Result<Stat, VfsError> {
        let pipe = self.pipe.lock();
        Ok(Stat {
            ino: 0,
            mode: 0o010600, // S_IFIFO | rw-------
            nlink: 1,
            size: pipe.buffer.len() as u64,
            blksize: 4096,
            ..Default::default()
        })
    }
}

impl Drop for PipeReadFileOps {
    fn drop(&mut self) {
        let mut pipe = self.pipe.lock();
        if pipe.readers > 0 {
            pipe.readers -= 1;
        }
    }
}

/// Write end file operations for an anonymous pipe.
pub struct PipeWriteFileOps {
    pipe: Arc<Mutex<PipeInner>>,
    nonblocking: bool,
}

impl FileOps for PipeWriteFileOps {
    fn write(&self, _offset: usize, buf: &[u8]) -> Result<usize, VfsError> {
        if buf.is_empty() {
            return Ok(0);
        }

        let mut total_written = 0;

        while total_written < buf.len() {
            let mut pipe = self.pipe.lock();

            // Broken pipe: no readers remain
            if pipe.readers == 0 {
                if let Some(proc_arc) = crate::proc::current_process() {
                    let mut proc = proc_arc.lock();
                    let _ = proc.send_signal(13); // SIGPIPE
                }
                return Err(VfsError::PermissionDenied); // EPIPE
            }

            let available = pipe.capacity.saturating_sub(pipe.buffer.len());
            if available > 0 {
                let remaining = buf.len() - total_written;
                let chunk_size = core::cmp::min(remaining, available);
                for &byte in &buf[total_written..total_written + chunk_size] {
                    pipe.buffer.push_back(byte);
                }
                total_written += chunk_size;

                if total_written == buf.len() {
                    return Ok(total_written);
                }
            }

            if self.nonblocking {
                if total_written > 0 {
                    return Ok(total_written);
                }
                return Err(VfsError::InvalidInput); // EAGAIN / WouldBlock
            }

            drop(pipe);
            Thread::yield_cpu();
        }

        Ok(total_written)
    }

    fn stat(&self) -> Result<Stat, VfsError> {
        let pipe = self.pipe.lock();
        Ok(Stat {
            ino: 0,
            mode: 0o010600, // S_IFIFO | rw-------
            nlink: 1,
            size: pipe.buffer.len() as u64,
            blksize: 4096,
            ..Default::default()
        })
    }
}

impl Drop for PipeWriteFileOps {
    fn drop(&mut self) {
        let mut pipe = self.pipe.lock();
        if pipe.writers > 0 {
            pipe.writers -= 1;
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

    let pipe = Arc::new(Mutex::new(PipeInner::new(PIPE_BUFFER_CAPACITY)));

    let read_ops = Arc::new(PipeReadFileOps {
        pipe: pipe.clone(),
        nonblocking,
    });

    let write_ops = Arc::new(PipeWriteFileOps { pipe, nonblocking });

    let inode = Arc::new(Inode {
        ino,
        inode_type: InodeType::File,
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
