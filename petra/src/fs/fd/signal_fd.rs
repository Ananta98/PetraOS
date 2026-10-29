// SPDX-License-Identifier: GPL-2.0

//! Signal notification file descriptors (`signalfd(2)` / `signalfd4(2)`).
//!
//! Exposes asynchronous process/thread signals via readable file descriptors,
//! supporting synchronous signal consumption and epoll readiness integration.

use alloc::{boxed::Box, sync::Arc};
use ostd::sync::WaitQueue;
use spin::Mutex;

use crate::{
    fs::{
        poll::PollEvents,
        vfs::{File, FileOps, INode, INodeAttr, Mode, NodeOps, OpenFlags},
    },
    proc::thread::Thread,
    uapi::{
        errno::{Errno, Result},
        fd::{SFD_NONBLOCK, SIGNALFD_SIGINFO_SIZE, SignalFdSiginfo},
        signal::SigSet,
    },
};

pub use crate::uapi::fd::{
    BYTES_PER_SIGSET_WORD, SFD_CLOEXEC, SFD_VALID_FLAGS_MASK, SIGSET_ABI_SIZE_MAX,
    SIGSET_ABI_SIZE_MIN,
};

const SIGNALFD_INODE_NUMBER: usize = 905;

/// Internal state of a signalfd file description.
pub struct SignalFd {
    pub mask: Mutex<SigSet>,
    pub wait_queue: WaitQueue,
}

impl SignalFd {
    pub fn new(mask: SigSet) -> Self {
        Self {
            mask: Mutex::new(mask),
            wait_queue: WaitQueue::new(),
        }
    }

    pub fn set_mask(&self, new_mask: SigSet) {
        *self.mask.lock() = new_mask;
        crate::fs::poll::notify();
    }

    fn drain_matching_signals(
        &self,
        dest_buffer: &mut [u8],
        max_records: usize,
    ) -> Result<usize> {
        let thread = Thread::current().ok_or(Errno::ESRCH)?;
        let process = thread.proc().ok_or(Errno::ESRCH)?;
        let mask = *self.mask.lock();
        let mut records_read = 0;

        while records_read < max_records {
            let queued_signal = thread
                .inner
                .lock()
                .sigqueue
                .take_matching(|sig| mask.contains(sig))
                .or_else(|| process.sigqueue.take_matching(|sig| mask.contains(sig)));

            let Some(queued) = queued_signal else {
                break;
            };

            let item_info = SignalFdSiginfo::from_siginfo(&queued.info);
            let write_offset = records_read * SIGNALFD_SIGINFO_SIZE;
            dest_buffer[write_offset..write_offset + SIGNALFD_SIGINFO_SIZE]
                .copy_from_slice(&item_info.to_bytes());
            records_read += 1;
        }

        Ok(records_read)
    }

    pub fn read_signals(&self, dest_buffer: &mut [u8], nonblocking: bool) -> Result<usize> {
        let max_records = dest_buffer.len() / SIGNALFD_SIGINFO_SIZE;

        let mut try_read = || -> Option<Result<usize>> {
            match self.drain_matching_signals(dest_buffer, max_records) {
                Ok(0) => {
                    if Thread::should_interrupt() {
                        Some(Err(Errno::EINTR))
                    } else {
                        None
                    }
                }
                Ok(count) => Some(Ok(count * SIGNALFD_SIGINFO_SIZE)),
                Err(err) => Some(Err(err)),
            }
        };

        if let Some(result) = try_read() {
            return result;
        }
        if nonblocking {
            return Err(Errno::EAGAIN);
        }
        self.wait_queue.wait_until(try_read)
    }

    pub fn poll_events(&self) -> PollEvents {
        let Some(thread) = Thread::current() else {
            return PollEvents::empty();
        };
        let Some(process) = thread.proc() else {
            return PollEvents::empty();
        };

        let mask = *self.mask.lock();
        let has_pending = thread
            .inner
            .lock()
            .sigqueue
            .has_matching(|sig| mask.contains(sig))
            || process.sigqueue.has_matching(|sig| mask.contains(sig));

        if has_pending {
            PollEvents::IN | PollEvents::RDNORM
        } else {
            PollEvents::empty()
        }
    }
}

pub struct SignalFdFileOps {
    instance: Arc<SignalFd>,
    nonblocking: bool,
}

impl FileOps for SignalFdFileOps {
    fn read(&self, _offset: u64, buf: &mut [u8]) -> Result<usize> {
        if buf.len() < SIGNALFD_SIGINFO_SIZE {
            return Err(Errno::EINVAL);
        }
        self.instance.read_signals(buf, self.nonblocking)
    }

    fn write(&self, _offset: u64, _buf: &[u8]) -> Result<usize> {
        crate::return_errno!(EINVAL, "cannot write to signalfd descriptor")
    }

    fn poll(&self) -> PollEvents {
        self.instance.poll_events()
    }

    fn as_signalfd(&self) -> Option<Arc<SignalFd>> {
        Some(Arc::clone(&self.instance))
    }
}

pub struct SignalFdNodeOps;

impl NodeOps for SignalFdNodeOps {
    fn open(&self, _flags: OpenFlags) -> Result<Box<dyn FileOps>> {
        crate::return_errno!(EOPNOTSUPP, "cannot directly open signalfd inode")
    }

    fn getattr(&self) -> Result<INodeAttr> {
        Ok(INodeAttr {
            size: 0,
            mode: Mode::FIFO | Mode::RUSR | Mode::WUSR,
            nlink: 1,
            uid: 0,
            gid: 0,
            rdev: 0,
        })
    }
}

/// Creates a new anonymous `File` wrapping a [`SignalFd`].
pub fn create_signalfd_file(mask: SigSet, flags: i32) -> Arc<File> {
    let nonblocking = (flags & SFD_NONBLOCK) != 0;
    let instance = Arc::new(SignalFd::new(mask));

    let inode = Arc::new(INode::new(
        Box::new(SignalFdNodeOps),
        Mode::FIFO | Mode::RUSR | Mode::WUSR,
        SIGNALFD_INODE_NUMBER,
    ));

    let mut open_flags = OpenFlags::READ;
    if nonblocking {
        open_flags |= OpenFlags::NONBLOCK;
    }

    Arc::new(File::new(
        Box::new(SignalFdFileOps {
            instance,
            nonblocking,
        }),
        inode,
        open_flags,
        false,
    ))
}
