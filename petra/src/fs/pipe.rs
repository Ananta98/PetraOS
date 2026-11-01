// SPDX-License-Identifier: GPL-2.0

//! Anonymous pipes (`pipe(2)` / `pipe2(2)`).
//!
//! A pipe is a bounded byte queue shared by a read end and a write end. Each end
//! is an ordinary [`File`], so descriptors, `dup`, `fork` and `poll` work on it
//! unchanged.
//!
//! POSIX semantics implemented here:
//! * `read` blocks while the pipe is empty and a writer exists; it returns 0
//!   (EOF) once every write end is gone.
//! * `write` of at most [`PIPE_BUF`] bytes is atomic: it completes in full or
//!   not at all. Larger writes may be split.
//! * `write` with no reader fails with `EPIPE` and raises `SIGPIPE`.
//! * `O_NONBLOCK` ends return `EAGAIN` instead of sleeping.
//!
//! An end is released when its last `Arc<File>` is dropped (not on `close`,
//! because a `dup`ed or forked descriptor shares one `File`), so the reader and
//! writer counts follow open file descriptions exactly as POSIX requires.
//!
//! Known limits: the non-blocking mode is fixed at creation (`fcntl(F_SETFL)`
//! does not reach the pipe), and a blocked pipe call is not interruptible by
//! signals.

use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::sync::Arc;
use core::sync::atomic::{AtomicUsize, Ordering};
use ostd::sync::WaitQueue;
use spin::Mutex;

use crate::uapi::errno::{Errno, Result};
use crate::uapi::signal::{SigInfo, Signal};
use crate::fs::poll::{self, PollEvents};
use crate::fs::vfs::{File, FileOps, INode, INodeAttr, Mode, NodeOps, OpenFlags};
use crate::proc::thread::Thread;

/// Writes up to this many bytes are atomic.
pub const PIPE_BUF: usize = 4096;
/// Total bytes a pipe buffers before writers block.
pub const PIPE_CAPACITY: usize = 65536;

pub struct PipeState {
    pub buf: VecDeque<u8>,
    pub readers: usize,
    pub writers: usize,
    pub writers_opened: bool,
}

pub struct Pipe {
    pub state: Mutex<PipeState>,
    /// Sleepers waiting for data (readers) or space (writers).
    pub queue: WaitQueue,
}

impl Pipe {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(PipeState {
                buf: VecDeque::new(),
                readers: 0,
                writers: 0,
                writers_opened: false,
            }),
            queue: WaitQueue::new(),
        }
    }

    pub fn new_anonymous() -> Self {
        Self {
            state: Mutex::new(PipeState {
                buf: VecDeque::new(),
                readers: 1,
                writers: 1,
                writers_opened: true,
            }),
            queue: WaitQueue::new(),
        }
    }

    /// Wakes blocked readers/writers and any poller.
    pub fn wake(&self) {
        self.queue.wake_all();
        poll::notify();
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum End {
    Read,
    Write,
    Both,
}

pub struct PipeEnd {
    pub pipe: Arc<Pipe>,
    pub end: End,
    pub nonblock: bool,
}

impl Drop for PipeEnd {
    fn drop(&mut self) {
        let mut st = self.pipe.state.lock();
        match self.end {
            End::Read => st.readers = st.readers.saturating_sub(1),
            End::Write => st.writers = st.writers.saturating_sub(1),
            End::Both => {
                st.readers = st.readers.saturating_sub(1);
                st.writers = st.writers.saturating_sub(1);
            }
        }
        if st.readers == 0 && st.writers == 0 {
            st.writers_opened = false;
        }
        drop(st);
        // EOF for readers / EPIPE for writers must be observable.
        self.pipe.wake();
    }
}

fn raise_sigpipe() {
    if let Some(proc) = Thread::current_proc() {
        let _ = proc.send_signal(SigInfo::from_user(Signal::SIGPIPE, 0), 0);
    }
}

impl FileOps for PipeEnd {
    fn read(&self, _offset: u64, buf: &mut [u8]) -> Result<usize> {
        if self.end != End::Read && self.end != End::Both {
            return Err(Errno::EBADF);
        }
        if buf.is_empty() {
            return Ok(0);
        }
        let pipe = &self.pipe;
        let mut try_read = || -> Option<usize> {
            let mut st = pipe.state.lock();
            if !st.buf.is_empty() {
                let n = buf.len().min(st.buf.len());
                for (slot, byte) in buf.iter_mut().zip(st.buf.drain(..n)) {
                    *slot = byte;
                }
                Some(n)
            } else if st.writers == 0 && st.writers_opened {
                Some(0) // EOF
            } else {
                None
            }
        };

        let n = match try_read() {
            Some(n) => n,
            None if self.nonblock => return Err(Errno::EAGAIN),
            None => pipe.queue.wait_until(try_read),
        };
        if n > 0 {
            pipe.wake(); // space freed
        }
        Ok(n)
    }

    fn write(&self, _offset: u64, buf: &[u8]) -> Result<usize> {
        if self.end != End::Write && self.end != End::Both {
            return Err(Errno::EBADF);
        }
        if buf.is_empty() {
            return Ok(0);
        }
        let pipe = &self.pipe;
        let atomic = buf.len() <= PIPE_BUF;
        let mut written = 0;

        while written < buf.len() {
            let remaining = buf.len() - written;
            let step = || -> Option<Result<usize>> {
                let mut st = pipe.state.lock();
                if st.readers == 0 {
                    return Some(Err(Errno::EPIPE));
                }
                let free = PIPE_CAPACITY - st.buf.len();
                // Atomic writes need room for everything; large ones take what fits.
                let needed = if atomic { remaining } else { 1 };
                if free >= needed {
                    let n = free.min(remaining);
                    st.buf.extend(&buf[written..written + n]);
                    Some(Ok(n))
                } else {
                    None
                }
            };

            let result = match step() {
                Some(r) => r,
                None if self.nonblock => {
                    return if written > 0 {
                        Ok(written)
                    } else {
                        Err(Errno::EAGAIN)
                    };
                }
                None => pipe.queue.wait_until(step),
            };

            match result {
                Ok(n) => {
                    written += n;
                    pipe.wake(); // data available
                }
                Err(e) => {
                    if written > 0 {
                        return Ok(written);
                    }
                    raise_sigpipe();
                    return Err(e);
                }
            }
        }
        Ok(written)
    }

    fn poll(&self) -> PollEvents {
        let st = self.pipe.state.lock();
        let mut ev = PollEvents::empty();
        let can_read = self.end == End::Read || self.end == End::Both;
        let can_write = self.end == End::Write || self.end == End::Both;

        if can_read {
            if !st.buf.is_empty() {
                ev |= PollEvents::IN | PollEvents::RDNORM;
            }
            if st.writers == 0 && st.writers_opened {
                // POSIX / Linux: when all write ends are closed, read() returns 0 (EOF)
                // immediately without blocking, so IN | RDNORM must be set alongside HUP.
                ev |= PollEvents::IN | PollEvents::RDNORM | PollEvents::HUP;
            }
        }
        if can_write {
            if st.readers == 0 {
                ev |= PollEvents::ERR | PollEvents::HUP;
            } else if PIPE_CAPACITY.saturating_sub(st.buf.len()) > 0 {
                ev |= PollEvents::OUT | PollEvents::WRNORM;
            }
        }
        ev
    }
}

/// Node operations for a named pipe (FIFO) inode in the filesystem.
pub struct FifoNodeOps {
    pipe: Arc<Pipe>,
    attr: spin::RwLock<INodeAttr>,
}

impl FifoNodeOps {
    pub fn new(attr: INodeAttr) -> Self {
        Self {
            pipe: Arc::new(Pipe::new()),
            attr: spin::RwLock::new(attr),
        }
    }

    pub fn pipe(&self) -> &Arc<Pipe> {
        &self.pipe
    }
}

impl NodeOps for FifoNodeOps {
    fn open(&self, flags: OpenFlags) -> Result<Box<dyn FileOps>> {
        let is_read = flags.contains(OpenFlags::READ);
        let is_write = flags.contains(OpenFlags::WRITE);
        let nonblock = flags.contains(OpenFlags::NONBLOCK);

        let end = match (is_read, is_write) {
            (true, true) => End::Both,
            (false, true) => End::Write,
            _ => End::Read,
        };

        {
            let mut st = self.pipe.state.lock();
            match end {
                End::Read => st.readers += 1,
                End::Write => {
                    st.writers += 1;
                    st.writers_opened = true;
                }
                End::Both => {
                    st.readers += 1;
                    st.writers += 1;
                    st.writers_opened = true;
                }
            }
        }
        self.pipe.wake();

        Ok(Box::new(PipeEnd {
            pipe: self.pipe.clone(),
            end,
            nonblock,
        }))
    }

    fn getattr(&self) -> Result<INodeAttr> {
        Ok(*self.attr.read())
    }

    fn setattr(&self, attr: &INodeAttr) -> Result<()> {
        *self.attr.write() = *attr;
        Ok(())
    }
}

/// Node operations of the anonymous inode both ends share.
struct PipeNodeOps;

impl NodeOps for PipeNodeOps {
    fn getattr(&self) -> Result<INodeAttr> {
        Ok(INodeAttr {
            mode: Mode::FIFO | Mode::RUSR | Mode::WUSR,
            ..INodeAttr::default()
        })
    }
}

static NEXT_PIPE_INO: AtomicUsize = AtomicUsize::new(1 << 32);

/// Creates a pipe, returning `(read_end, write_end)`.
///
/// `nonblock` sets `O_NONBLOCK` on both ends. Close-on-exec is a descriptor
/// property, so the caller applies it when installing the files.
pub fn create_pipe(nonblock: bool) -> (Arc<File>, Arc<File>) {
    let pipe = Arc::new(Pipe::new_anonymous());

    let inode = Arc::new(INode::new(
        Box::new(PipeNodeOps),
        Mode::FIFO | Mode::RUSR | Mode::WUSR,
        NEXT_PIPE_INO.fetch_add(1, Ordering::Relaxed),
    ));

    let extra = if nonblock {
        OpenFlags::NONBLOCK
    } else {
        OpenFlags::empty()
    };
    let read_end = Arc::new(File::new(
        Box::new(PipeEnd {
            pipe: pipe.clone(),
            end: End::Read,
            nonblock,
        }),
        inode.clone(),
        OpenFlags::READ | extra,
        false,
    ));
    let write_end = Arc::new(File::new(
        Box::new(PipeEnd {
            pipe,
            end: End::Write,
            nonblock,
        }),
        inode,
        OpenFlags::WRITE | extra,
        false,
    ));
    (read_end, write_end)
}
