// SPDX-License-Identifier: GPL-2.0

//! Inotify file monitoring subsystem (`inotify_init1`, `inotify_add_watch`, `inotify_rm_watch`).
//!
//! Exposes filesystem change notifications through readable file descriptors compatible
//! with Linux ABI, Qt/KDE `KDirWatch`, and epoll readiness multiplexing.

use alloc::{
    boxed::Box,
    collections::{BTreeMap, VecDeque},
    string::String,
    sync::Arc,
};
use core::sync::atomic::{AtomicI32, Ordering};
use ostd::sync::WaitQueue;
use spin::Mutex;

use crate::{
    fs::{
        fd::FdFlags,
        poll::PollEvents,
        vfs::{File, FileOps, INode, INodeAttr, Mode, NodeOps, OpenFlags},
    },
    proc::thread::Thread,
    uapi::{
        errno::{Errno, Result},
        inotify::{IN_IGNORED, IN_MASK_ADD, InotifyEventHeader},
    },
};

const INOTIFY_INODE_NUMBER: usize = 906;

/// A watch registered on an inotify instance.
#[derive(Clone, Debug)]
pub struct InotifyWatch {
    pub wd: i32,
    pub path: String,
    pub mask: u32,
}

/// A queued event ready to be consumed by `read(2)`.
#[derive(Clone, Debug)]
pub struct InotifyEvent {
    pub wd: i32,
    pub mask: u32,
    pub cookie: u32,
    pub name: String,
}

/// State of an inotify file description.
pub struct InotifyInstance {
    pub watches: Mutex<BTreeMap<i32, InotifyWatch>>,
    pub next_wd: AtomicI32,
    pub events: Mutex<VecDeque<InotifyEvent>>,
    pub wait_queue: WaitQueue,
}

impl InotifyInstance {
    /// Creates a new inotify instance.
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            watches: Mutex::new(BTreeMap::new()),
            next_wd: AtomicI32::new(1),
            events: Mutex::new(VecDeque::new()),
            wait_queue: WaitQueue::new(),
        })
    }

    /// Adds or updates a watch on `path` with `mask`.
    pub fn add_watch(&self, path: &str, mask: u32) -> Result<i32> {
        let mut watches = self.watches.lock();

        for watch in watches.values_mut() {
            if watch.path == path {
                if (mask & IN_MASK_ADD) != 0 {
                    watch.mask |= mask & !IN_MASK_ADD;
                } else {
                    watch.mask = mask;
                }
                return Ok(watch.wd);
            }
        }

        let wd = self.next_wd.fetch_add(1, Ordering::SeqCst);
        watches.insert(
            wd,
            InotifyWatch {
                wd,
                path: String::from(path),
                mask,
            },
        );
        Ok(wd)
    }

    /// Removes the watch specified by `wd`.
    pub fn rm_watch(&self, wd: i32) -> Result<()> {
        let removed = {
            let mut watches = self.watches.lock();
            watches.remove(&wd)
        };

        if removed.is_none() {
            return Err(Errno::EINVAL);
        }

        self.post_event(wd, IN_IGNORED, 0, "");
        Ok(())
    }

    /// Posts a notification event to the inotify queue.
    pub fn post_event(&self, wd: i32, mask: u32, cookie: u32, name: &str) {
        {
            let mut events = self.events.lock();
            events.push_back(InotifyEvent {
                wd,
                mask,
                cookie,
                name: String::from(name),
            });
        }
        self.wait_queue.wake_all();
        crate::fs::poll::notify();
    }

    /// Drains available events into `dest_buffer`.
    fn drain_events(&self, dest_buffer: &mut [u8]) -> Result<usize> {
        let min_size = core::mem::size_of::<InotifyEventHeader>();
        let mut events = self.events.lock();
        if events.is_empty() {
            return Ok(0);
        }

        let mut bytes_written = 0;
        while let Some(event) = events.pop_front() {
            let name_bytes = event.name.as_bytes();
            let name_len = if name_bytes.is_empty() {
                0
            } else {
                let raw = name_bytes.len() + 1;
                (raw + 15) & !15
            };

            let total_event_size = min_size + name_len;
            if bytes_written + total_event_size > dest_buffer.len() {
                events.push_front(event);
                break;
            }

            dest_buffer[bytes_written..bytes_written + 4]
                .copy_from_slice(&event.wd.to_ne_bytes());
            dest_buffer[bytes_written + 4..bytes_written + 8]
                .copy_from_slice(&event.mask.to_ne_bytes());
            dest_buffer[bytes_written + 8..bytes_written + 12]
                .copy_from_slice(&event.cookie.to_ne_bytes());
            dest_buffer[bytes_written + 12..bytes_written + 16]
                .copy_from_slice(&(name_len as u32).to_ne_bytes());
            bytes_written += min_size;

            if name_len > 0 {
                dest_buffer[bytes_written..bytes_written + name_bytes.len()]
                    .copy_from_slice(name_bytes);
                dest_buffer[bytes_written + name_bytes.len()..bytes_written + name_len]
                    .fill(0);
                bytes_written += name_len;
            }
        }
        Ok(bytes_written)
    }

    /// Reads events into `dest_buffer`, waiting if none are available.
    pub fn read_events(&self, dest_buffer: &mut [u8], nonblocking: bool) -> Result<usize> {
        let min_size = core::mem::size_of::<InotifyEventHeader>();
        if dest_buffer.len() < min_size {
            return Err(Errno::EINVAL);
        }

        let mut try_read = || -> Option<Result<usize>> {
            match self.drain_events(dest_buffer) {
                Ok(0) => {
                    if Thread::should_interrupt() {
                        Some(Err(Errno::EINTR))
                    } else {
                        None
                    }
                }
                Ok(count) => Some(Ok(count)),
                Err(err) => Some(Err(err)),
            }
        };

        if let Some(res) = try_read() {
            return res;
        }
        if nonblocking {
            return Err(Errno::EAGAIN);
        }
        self.wait_queue.wait_until(try_read)
    }

    /// Returns poll readiness.
    pub fn poll_events(&self) -> PollEvents {
        if !self.events.lock().is_empty() {
            PollEvents::IN | PollEvents::RDNORM
        } else {
            PollEvents::empty()
        }
    }
}

/// Operations on an open inotify descriptor.
pub struct InotifyFileOps {
    instance: Arc<InotifyInstance>,
    nonblocking: bool,
}

impl InotifyFileOps {
    pub fn new(instance: Arc<InotifyInstance>, nonblocking: bool) -> Self {
        Self {
            instance,
            nonblocking,
        }
    }
}

impl FileOps for InotifyFileOps {
    fn read(&self, _offset: u64, buf: &mut [u8]) -> Result<usize> {
        self.instance.read_events(buf, self.nonblocking)
    }

    fn write(&self, _offset: u64, _buf: &[u8]) -> Result<usize> {
        Err(Errno::EINVAL)
    }

    fn poll(&self) -> PollEvents {
        self.instance.poll_events()
    }

    fn as_inotify(&self) -> Option<Arc<InotifyInstance>> {
        Some(Arc::clone(&self.instance))
    }
}

/// Inode representation for inotify anonymous files.
pub struct InotifyNodeOps;

impl NodeOps for InotifyNodeOps {
    fn open(&self, _flags: OpenFlags) -> Result<Box<dyn FileOps>> {
        crate::return_errno!(EOPNOTSUPP, "cannot directly open inotify inode")
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

/// Creates a new inotify file and descriptor flags.
pub fn create_inotify_file(flags: i32) -> Result<(Arc<File>, FdFlags)> {
    let nonblocking = (flags & crate::uapi::inotify::IN_NONBLOCK) != 0;
    let cloexec = (flags & crate::uapi::inotify::IN_CLOEXEC) != 0;

    let instance = InotifyInstance::new();
    let ops = Box::new(InotifyFileOps::new(instance, nonblocking));
    let inode = Arc::new(INode::new(
        Box::new(InotifyNodeOps),
        Mode::FIFO | Mode::RUSR | Mode::WUSR,
        INOTIFY_INODE_NUMBER,
    ));

    let mut open_flags = OpenFlags::READ;
    if nonblocking {
        open_flags |= OpenFlags::NONBLOCK;
    }

    let file = Arc::new(File::new(ops, inode, open_flags, false));
    let fd_flags = if cloexec {
        FdFlags::FD_CLOEXEC
    } else {
        FdFlags::empty()
    };

    Ok((file, fd_flags))
}

#[cfg(ktest)]
mod tests {
    use super::*;
    use crate::uapi::inotify::{IN_CREATE, IN_DELETE, IN_MODIFY};
    use ostd::prelude::ktest;

    #[ktest]
    fn inotify_instance_lifecycle_and_drain() {
        let instance = InotifyInstance::new();
        let wd1 = instance.add_watch("/tmp", IN_CREATE | IN_DELETE).unwrap();
        assert_eq!(wd1, 1);

        let wd1_again = instance.add_watch("/tmp", IN_MODIFY).unwrap();
        assert_eq!(wd1_again, 1);

        let wd2 = instance.add_watch("/var", IN_CREATE).unwrap();
        assert_eq!(wd2, 2);

        // Initially no events
        assert_eq!(instance.poll_events(), PollEvents::empty());

        // Post event
        instance.post_event(wd1, IN_CREATE, 0, "test_file.txt");
        assert_eq!(instance.poll_events(), PollEvents::IN | PollEvents::RDNORM);

        // Drain event
        let mut buf = [0u8; 128];
        let bytes = instance.read_events(&mut buf, true).unwrap();
        assert!(bytes >= 16);

        let read_wd = i32::from_ne_bytes(buf[0..4].try_into().unwrap());
        let read_mask = u32::from_ne_bytes(buf[4..8].try_into().unwrap());
        let read_len = u32::from_ne_bytes(buf[12..16].try_into().unwrap());
        assert_eq!(read_wd, 1);
        assert_eq!(read_mask, IN_CREATE);
        assert!(read_len > 0);

        // Remove watch wd2
        instance.rm_watch(wd2).unwrap();
        assert_eq!(instance.poll_events(), PollEvents::IN | PollEvents::RDNORM);
    }
}

