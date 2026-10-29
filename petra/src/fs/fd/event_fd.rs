// SPDX-License-Identifier: GPL-2.0

//! Event notification file descriptors (`eventfd(2)` / `eventfd2(2)`).
//!
//! Provides a 64-bit integer counter mechanism for inter-thread and inter-process
//! event signaling, wait queuing, and epoll multiplexing.

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
        fd::{
            EFD_NONBLOCK, EFD_SEMAPHORE, EVENTFD_MAX_COUNTER, EVENTFD_PAYLOAD_SIZE,
        },
    },
};

pub use crate::uapi::fd::{EFD_CLOEXEC, EFD_VALID_FLAGS_MASK};

const EVENTFD_INODE_NUMBER: usize = 903;

/// Core state of an eventfd object.
pub struct EventFd {
    counter: Mutex<u64>,
    is_semaphore: bool,
    wait_queue: WaitQueue,
}

impl EventFd {
    pub fn new(initial_val: u32, is_semaphore: bool) -> Self {
        Self {
            counter: Mutex::new(initial_val as u64),
            is_semaphore,
            wait_queue: WaitQueue::new(),
        }
    }

    pub fn consume_event(&self, dest_buffer: &mut [u8], nonblocking: bool) -> Result<usize> {
        let mut try_consume = || -> Option<Result<usize>> {
            let mut counter_guard = self.counter.lock();
            let current_val = *counter_guard;
            if current_val > 0 {
                let return_val = if self.is_semaphore {
                    *counter_guard -= 1;
                    1
                } else {
                    *counter_guard = 0;
                    current_val
                };
                drop(counter_guard);

                dest_buffer[..EVENTFD_PAYLOAD_SIZE].copy_from_slice(&return_val.to_ne_bytes());
                self.wait_queue.wake_all();
                crate::fs::poll::notify();
                return Some(Ok(EVENTFD_PAYLOAD_SIZE));
            }
            drop(counter_guard);

            if Thread::should_interrupt() {
                return Some(Err(Errno::EINTR));
            }
            None
        };

        if let Some(result) = try_consume() {
            return result;
        }
        if nonblocking {
            return Err(Errno::EAGAIN);
        }
        self.wait_queue.wait_until(try_consume)
    }

    pub fn post_event(&self, increment_val: u64, nonblocking: bool) -> Result<usize> {
        let try_post = || -> Option<Result<usize>> {
            let mut counter_guard = self.counter.lock();
            let current_val = *counter_guard;
            if EVENTFD_MAX_COUNTER - current_val >= increment_val {
                *counter_guard = current_val + increment_val;
                drop(counter_guard);

                self.wait_queue.wake_all();
                crate::fs::poll::notify();
                return Some(Ok(EVENTFD_PAYLOAD_SIZE));
            }
            drop(counter_guard);

            if Thread::should_interrupt() {
                return Some(Err(Errno::EINTR));
            }
            None
        };

        if let Some(result) = try_post() {
            return result;
        }
        if nonblocking {
            return Err(Errno::EAGAIN);
        }
        self.wait_queue.wait_until(try_post)
    }

    pub fn poll_events(&self) -> PollEvents {
        let current_val = *self.counter.lock();
        let mut events = PollEvents::empty();
        if current_val > 0 {
            events |= PollEvents::IN | PollEvents::RDNORM;
        }
        if current_val < EVENTFD_MAX_COUNTER {
            events |= PollEvents::OUT | PollEvents::WRNORM;
        }
        events
    }
}

pub struct EventFdFileOps {
    instance: Arc<EventFd>,
    nonblocking: bool,
}

impl FileOps for EventFdFileOps {
    fn read(&self, _offset: u64, buf: &mut [u8]) -> Result<usize> {
        if buf.len() < EVENTFD_PAYLOAD_SIZE {
            return Err(Errno::EINVAL);
        }
        self.instance.consume_event(buf, self.nonblocking)
    }

    fn write(&self, _offset: u64, buf: &[u8]) -> Result<usize> {
        if buf.len() < EVENTFD_PAYLOAD_SIZE {
            return Err(Errno::EINVAL);
        }
        let increment_val = u64::from_ne_bytes(
            buf[..EVENTFD_PAYLOAD_SIZE]
                .try_into()
                .map_err(|_| Errno::EINVAL)?,
        );
        if increment_val == u64::MAX {
            return Err(Errno::EINVAL);
        }
        self.instance.post_event(increment_val, self.nonblocking)
    }

    fn poll(&self) -> PollEvents {
        self.instance.poll_events()
    }

    fn as_eventfd(&self) -> Option<Arc<EventFd>> {
        Some(Arc::clone(&self.instance))
    }
}

pub struct EventFdNodeOps;

impl NodeOps for EventFdNodeOps {
    fn open(&self, _flags: OpenFlags) -> Result<Box<dyn FileOps>> {
        crate::return_errno!(EOPNOTSUPP, "cannot directly open eventfd inode")
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

/// Creates a new anonymous `File` wrapping an [`EventFd`].
pub fn create_eventfd_file(initval: u32, flags: i32) -> Arc<File> {
    let is_semaphore = (flags & EFD_SEMAPHORE) != 0;
    let nonblocking = (flags & EFD_NONBLOCK) != 0;
    let instance = Arc::new(EventFd::new(initval, is_semaphore));

    let inode = Arc::new(INode::new(
        Box::new(EventFdNodeOps),
        Mode::FIFO | Mode::RUSR | Mode::WUSR,
        EVENTFD_INODE_NUMBER,
    ));

    let mut open_flags = OpenFlags::READ | OpenFlags::WRITE;
    if nonblocking {
        open_flags |= OpenFlags::NONBLOCK;
    }

    Arc::new(File::new(
        Box::new(EventFdFileOps {
            instance,
            nonblocking,
        }),
        inode,
        open_flags,
        false,
    ))
}
