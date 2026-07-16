/// Pipe Inter-Process Communication (IPC)
///
/// Implements unidirectional data channels (pipes) that can be used for
/// communication between processes or threads. A pipe has a read end and a
/// write end, both of which implement the VFS `FileOps` trait.
///
/// # Semantics
/// - **Read end:** Reading from an empty pipe blocks until data is written,
///   unless all write ends are closed, in which case it returns `0` (EOF).
/// - **Write end:** Writing to a full pipe blocks until space becomes available,
///   unless all read ends are closed, in which case it returns an error and
///   sends `SIGPIPE` to the calling process.
/// - **Capacity:** Fixed capacity of 65,536 bytes (matching Linux default).
use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicUsize, Ordering};
use ostd::Error;
use ostd::sync::{SpinLock, WaitQueue};

use crate::fs::vfs::{DirEntry, FileOps, SeekFrom};
use crate::proc::process::Process;

/// The maximum capacity of the pipe buffer in bytes.
const PIPE_CAPACITY: usize = 65536;

/// Shared internal state of a pipe.
struct PipeInner {
    /// The FIFO queue buffer.
    buffer: SpinLock<VecDeque<u8>>,
    /// Number of active read handles.
    reader_count: AtomicUsize,
    /// Number of active write handles.
    writer_count: AtomicUsize,
    /// Wait queue for readers blocking on an empty pipe.
    read_wait: WaitQueue,
    /// Wait queue for writers blocking on a full pipe.
    write_wait: WaitQueue,
}

impl PipeInner {
    fn new() -> Self {
        Self {
            buffer: SpinLock::new(VecDeque::new()),
            reader_count: AtomicUsize::new(1),
            writer_count: AtomicUsize::new(1),
            read_wait: WaitQueue::new(),
            write_wait: WaitQueue::new(),
        }
    }
}

// ──────────────────────────────────────────────────────────────
// PipeReadOps
// ──────────────────────────────────────────────────────────────

/// The read end of a pipe.
pub struct PipeReadOps {
    inner: Arc<PipeInner>,
}

impl FileOps for PipeReadOps {
    fn read(&mut self, buf: &mut [u8], _offset: &mut usize) -> Result<usize, Error> {
        if buf.is_empty() {
            return Ok(0);
        }

        loop {
            {
                let mut buffer = self.inner.buffer.lock();
                if !buffer.is_empty() {
                    let len = core::cmp::min(buf.len(), buffer.len());
                    for (i, byte) in buffer.drain(..len).enumerate() {
                        buf[i] = byte;
                    }
                    // Wake up blocked writers since space was freed.
                    self.inner.write_wait.wake_all();
                    return Ok(len);
                }

                // If empty and no writers are left, return EOF (0).
                if self.inner.writer_count.load(Ordering::Acquire) == 0 {
                    return Ok(0);
                }
            }

            // Block until data is written or all writers close.
            self.inner.read_wait.wait_until(|| {
                let buffer = self.inner.buffer.lock();
                let writers = self.inner.writer_count.load(Ordering::Acquire);
                (!buffer.is_empty() || writers == 0).then_some(())
            });
        }
    }

    fn write(&mut self, _buf: &[u8], _offset: &mut usize) -> Result<usize, Error> {
        Err(Error::AccessDenied)
    }

    fn seek(&mut self, _pos: SeekFrom, _offset: &mut usize) -> Result<usize, Error> {
        Err(Error::IoError)
    }

    fn readdir(&mut self) -> Result<Vec<DirEntry>, Error> {
        Err(Error::InvalidArgs)
    }
}

impl Drop for PipeReadOps {
    fn drop(&mut self) {
        if self.inner.reader_count.fetch_sub(1, Ordering::Release) == 1 {
            // Last reader closed; wake up blocked writers so they receive EPIPE/SIGPIPE.
            self.inner.write_wait.wake_all();
        }
    }
}

// ──────────────────────────────────────────────────────────────
// PipeWriteOps
// ──────────────────────────────────────────────────────────────

/// The write end of a pipe.
pub struct PipeWriteOps {
    inner: Arc<PipeInner>,
}

impl FileOps for PipeWriteOps {
    fn read(&mut self, _buf: &mut [u8], _offset: &mut usize) -> Result<usize, Error> {
        Err(Error::AccessDenied)
    }

    fn write(&mut self, buf: &[u8], _offset: &mut usize) -> Result<usize, Error> {
        if buf.is_empty() {
            return Ok(0);
        }

        // If no readers are left, write fails with EPIPE / SIGPIPE.
        if self.inner.reader_count.load(Ordering::Acquire) == 0 {
            if let Some(task) = ostd::task::Task::current() {
                if let Some(task_data) = task.data().downcast_ref::<crate::scheduler::TaskData>() {
                    let _ = crate::ipc::send_signal_to_pid(task_data.pid, crate::ipc::SIGPIPE, 0);
                }
            }
            return Err(Error::IoError);
        }

        let mut total_written = 0;
        while total_written < buf.len() {
            let written = {
                let mut buffer = self.inner.buffer.lock();
                let space = PIPE_CAPACITY.saturating_sub(buffer.len());
                if space > 0 {
                    let to_write = core::cmp::min(buf.len() - total_written, space);
                    for &byte in &buf[total_written..total_written + to_write] {
                        buffer.push_back(byte);
                    }
                    // Wake up blocked readers.
                    self.inner.read_wait.wake_all();
                    to_write
                } else {
                    0
                }
            };

            if written > 0 {
                total_written += written;
            } else {
                // Buffer is full. If readers closed in the meantime, fail.
                if self.inner.reader_count.load(Ordering::Acquire) == 0 {
                    if let Some(task) = ostd::task::Task::current() {
                        if let Some(task_data) =
                            task.data().downcast_ref::<crate::scheduler::TaskData>()
                        {
                            let _ = crate::ipc::send_signal_to_pid(
                                task_data.pid,
                                crate::ipc::SIGPIPE,
                                0,
                            );
                        }
                    }
                    return if total_written > 0 {
                        Ok(total_written)
                    } else {
                        Err(Error::IoError)
                    };
                }

                // Block until space is freed or all readers close.
                self.inner.write_wait.wait_until(|| {
                    let buffer = self.inner.buffer.lock();
                    let readers = self.inner.reader_count.load(Ordering::Acquire);
                    (buffer.len() < PIPE_CAPACITY || readers == 0).then_some(())
                });
            }
        }

        Ok(total_written)
    }

    fn seek(&mut self, _pos: SeekFrom, _offset: &mut usize) -> Result<usize, Error> {
        Err(Error::IoError)
    }

    fn readdir(&mut self) -> Result<Vec<DirEntry>, Error> {
        Err(Error::InvalidArgs)
    }
}

impl Drop for PipeWriteOps {
    fn drop(&mut self) {
        if self.inner.writer_count.fetch_sub(1, Ordering::Release) == 1 {
            // Last writer closed; wake up blocked readers so they receive EOF.
            self.inner.read_wait.wake_all();
        }
    }
}

// ──────────────────────────────────────────────────────────────
// Constructor
// ──────────────────────────────────────────────────────────────

/// Create a new unidirectional pipe channel.
///
/// Returns `(read_end, write_end)` representing the unidirectional pipe.
pub fn create_pipe() -> (Box<dyn FileOps>, Box<dyn FileOps>) {
    let inner = Arc::new(PipeInner::new());
    (
        Box::new(PipeReadOps {
            inner: inner.clone(),
        }),
        Box::new(PipeWriteOps { inner }),
    )
}

// ──────────────────────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────────────────────

#[cfg(ktest)]
mod tests {
    use super::*;
    use ostd::prelude::ktest;

    #[ktest]
    fn test_pipe_basic_read_write() {
        let (mut reader, mut writer) = create_pipe();
        let mut offset = 0;

        // Write small chunk.
        let data = b"hello pipe";
        let written = writer.write(data, &mut offset).unwrap();
        assert_eq!(written, data.len());

        // Read chunk back.
        let mut buf = [0u8; 10];
        let read_bytes = reader.read(&mut buf, &mut offset).unwrap();
        assert_eq!(read_bytes, 10);
        assert_eq!(&buf, data);
    }

    #[ktest]
    fn test_pipe_eof_on_writer_close() {
        let (mut reader, writer) = create_pipe();
        let mut offset = 0;

        // Drop the writer end immediately.
        drop(writer);

        // Read should return 0 (EOF) immediately.
        let mut buf = [0u8; 10];
        let read_bytes = reader.read(&mut buf, &mut offset).unwrap();
        assert_eq!(read_bytes, 0);
    }

    #[ktest]
    fn test_pipe_epipe_on_reader_close() {
        let (reader, mut writer) = create_pipe();
        let mut offset = 0;

        // Drop reader end.
        drop(reader);

        // Write should return Error (EPIPE).
        let res = writer.write(b"fail", &mut offset);
        assert!(res.is_err());
    }
}
