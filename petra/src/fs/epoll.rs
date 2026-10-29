// SPDX-License-Identifier: GPL-2.0

//! Epoll I/O event notification facility.
//!
//! Provides the core data structures and VFS integration for `epoll(7)`,
//! supporting level-triggered and edge-triggered monitoring, `EPOLLONESHOT`,
//! and integration with the kernel wait queues and `poll(2)` subsystem.

use alloc::{boxed::Box, sync::Arc, vec::Vec};
use spin::Mutex;

use crate::{
    uapi::errno::{Errno, Result},
    fs::{
        poll::PollEvents,
        vfs::{File, FileOps, INode, INodeAttr, Mode, NodeOps, OpenFlags},
    },
};

// Linux epoll control operations
pub const EPOLL_CTL_ADD: i32 = 1;
pub const EPOLL_CTL_DEL: i32 = 2;
pub const EPOLL_CTL_MOD: i32 = 3;

// Linux epoll event masks
pub const EPOLLIN: u32 = 0x0001;
pub const EPOLLPRI: u32 = 0x0002;
pub const EPOLLOUT: u32 = 0x0004;
pub const EPOLLERR: u32 = 0x0008;
pub const EPOLLHUP: u32 = 0x0010;
pub const EPOLLNVAL: u32 = 0x0020;
pub const EPOLLRDNORM: u32 = 0x0040;
pub const EPOLLRDBAND: u32 = 0x0080;
pub const EPOLLWRNORM: u32 = 0x0100;
pub const EPOLLWRBAND: u32 = 0x0200;
pub const EPOLLMSG: u32 = 0x0400;
pub const EPOLLRDHUP: u32 = 0x2000;
pub const EPOLLEXCLUSIVE: u32 = 1 << 28;
pub const EPOLLWAKEUP: u32 = 1 << 29;
pub const EPOLLONESHOT: u32 = 1 << 30;
pub const EPOLLET: u32 = 1 << 31;

// epoll_create1 flags
pub const EPOLL_CLOEXEC: i32 = 0o2000000;

/// Linux `struct epoll_event` layout on x86-64 (12 bytes packed).
#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EpollEvent {
    pub events: u32,
    pub data: u64,
}

/// An entry in the epoll interest list.
pub struct EpollItem {
    pub fd: i32,
    pub file: Arc<File>,
    pub events: u32,
    pub data: u64,
    pub disabled: bool,
    pub last_poll: u32,
}

/// The core epoll instance maintaining an interest list of registered descriptors.
pub struct EpollInstance {
    items: Mutex<Vec<EpollItem>>,
}

impl EpollInstance {
    /// Creates a new, empty epoll instance.
    pub fn new() -> Self {
        Self {
            items: Mutex::new(Vec::new()),
        }
    }

    /// Registers a new file descriptor in the interest list.
    pub fn add(&self, fd: i32, file: Arc<File>, events: u32, data: u64) -> Result<()> {
        let mut items = self.items.lock();
        if items.iter().any(|item| item.fd == fd) {
            return Err(Errno::EEXIST);
        }
        items.push(EpollItem {
            fd,
            file,
            events,
            data,
            disabled: false,
            last_poll: 0,
        });
        crate::fs::poll::notify();
        Ok(())
    }

    /// Modifies the settings associated with a target file descriptor.
    pub fn modify(&self, fd: i32, events: u32, data: u64) -> Result<()> {
        let mut items = self.items.lock();
        if let Some(item) = items.iter_mut().find(|item| item.fd == fd) {
            item.events = events;
            item.data = data;
            item.disabled = false;
            item.last_poll = 0;
            crate::fs::poll::notify();
            Ok(())
        } else {
            Err(Errno::ENOENT)
        }
    }

    /// Removes a file descriptor from the interest list.
    pub fn delete(&self, fd: i32) -> Result<()> {
        let mut items = self.items.lock();
        let len_before = items.len();
        items.retain(|item| item.fd != fd);
        if items.len() < len_before {
            Ok(())
        } else {
            Err(Errno::ENOENT)
        }
    }

    /// Scans the interest list for ready events, writing them into `out`.
    ///
    /// Supports level-triggered, edge-triggered (`EPOLLET`), and `EPOLLONESHOT`.
    pub fn scan_ready(&self, out: &mut [EpollEvent]) -> usize {
        let mut items = self.items.lock();
        let mut count = 0;

        for item in items.iter_mut() {
            if item.disabled {
                continue;
            }

            let poll_res = item.file.poll().bits() as u32;
            let wanted = item.events | EPOLLERR | EPOLLHUP;
            let matched = poll_res & wanted;

            if (item.events & EPOLLET) != 0 {
                // Edge-triggered: report only on transition to ready
                if matched != 0 && matched != item.last_poll {
                    item.last_poll = matched;
                    if (item.events & EPOLLONESHOT) != 0 {
                        item.disabled = true;
                    }
                    out[count] = EpollEvent {
                        events: matched,
                        data: item.data,
                    };
                    count += 1;
                    if count >= out.len() {
                        break;
                    }
                } else if matched == 0 {
                    item.last_poll = 0;
                }
            } else {
                // Level-triggered: report as long as condition persists
                item.last_poll = matched;
                if matched != 0 {
                    if (item.events & EPOLLONESHOT) != 0 {
                        item.disabled = true;
                    }
                    out[count] = EpollEvent {
                        events: matched,
                        data: item.data,
                    };
                    count += 1;
                    if count >= out.len() {
                        break;
                    }
                }
            }
        }

        count
    }

    /// Checks if any monitored item is currently ready.
    pub fn is_ready(&self) -> bool {
        let items = self.items.lock();
        for item in items.iter() {
            if item.disabled {
                continue;
            }
            let poll_res = item.file.poll().bits() as u32;
            let wanted = item.events | EPOLLERR | EPOLLHUP;
            if (poll_res & wanted) != 0 {
                return true;
            }
        }
        false
    }
}

impl Default for EpollInstance {
    fn default() -> Self {
        Self::new()
    }
}

/// File operations for open epoll instances.
pub struct EpollFileOps {
    pub instance: Arc<EpollInstance>,
}

impl FileOps for EpollFileOps {
    fn read(&self, _offset: u64, _buf: &mut [u8]) -> Result<usize> {
        crate::return_errno!(EINVAL, "cannot read from epoll file descriptor")
    }

    fn write(&self, _offset: u64, _buf: &[u8]) -> Result<usize> {
        crate::return_errno!(EINVAL, "cannot write to epoll file descriptor")
    }

    fn poll(&self) -> PollEvents {
        if self.instance.is_ready() {
            PollEvents::IN | PollEvents::RDNORM
        } else {
            PollEvents::empty()
        }
    }

    fn as_epoll(&self) -> Option<Arc<EpollInstance>> {
        Some(self.instance.clone())
    }
}

pub struct EpollNodeOps;

impl NodeOps for EpollNodeOps {
    fn open(&self, _flags: OpenFlags) -> Result<Box<dyn FileOps>> {
        crate::return_errno!(EOPNOTSUPP, "cannot directly open epoll inode")
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

/// Creates a new anonymous `File` wrapping an [`EpollInstance`].
pub fn create_epoll_file() -> Arc<File> {
    let instance = Arc::new(EpollInstance::new());
    let inode = Arc::new(INode::new(
        Box::new(EpollNodeOps),
        Mode::FIFO | Mode::RUSR | Mode::WUSR,
        901,
    ));
    Arc::new(File::new(
        Box::new(EpollFileOps { instance }),
        inode,
        OpenFlags::READ | OpenFlags::WRITE,
        false,
    ))
}

#[cfg(ktest)]
mod tests {
    use super::*;
    use crate::fs::pipe::create_pipe;
    use ostd::prelude::ktest;

    #[ktest]
    fn epoll_basic_add_wait_and_del() {
        let ep = EpollInstance::new();
        let (rx, tx) = create_pipe(false);

        // Add rx end watching for EPOLLIN
        ep.add(10, rx.clone(), EPOLLIN, 0x1234_5678)
            .expect("epoll add");

        let mut events = [EpollEvent::default(); 4];
        // Initially no data, scan_ready returns 0
        assert_eq!(ep.scan_ready(&mut events), 0);

        // Write data into pipe
        tx.write(b"alpine_epoll_data").expect("write pipe");

        // Now scan_ready should return 1 event with data
        let n = ep.scan_ready(&mut events);
        assert_eq!(n, 1);
        let ev0_data = events[0].data;
        let ev0_mask = events[0].events;
        assert_eq!(ev0_data, 0x1234_5678);
        assert!((ev0_mask & EPOLLIN) != 0);

        // Delete from epoll
        ep.delete(10).expect("epoll delete");
        assert_eq!(ep.scan_ready(&mut events), 0);
    }

    #[ktest]
    fn epoll_edge_triggered_and_oneshot() {
        let ep = EpollInstance::new();
        let (rx, tx) = create_pipe(false);

        // Add with EPOLLET (Edge Triggered)
        ep.add(20, rx.clone(), EPOLLIN | EPOLLET, 0xfeed_cafe)
            .expect("epoll add ET");

        tx.write(b"data1").expect("write");
        let mut events = [EpollEvent::default(); 2];

        // First scan returns 1 event
        assert_eq!(ep.scan_ready(&mut events), 1);
        let ev0_data = events[0].data;
        assert_eq!(ev0_data, 0xfeed_cafe);

        // Second scan without state transition returns 0 (edge-triggered)
        assert_eq!(ep.scan_ready(&mut events), 0);

        // Modify to EPOLLONESHOT
        ep.modify(20, EPOLLIN | EPOLLONESHOT, 0xdead_beef)
            .expect("epoll mod oneshot");

        // First scan returns event
        assert_eq!(ep.scan_ready(&mut events), 1);
        let ev0_data = events[0].data;
        assert_eq!(ev0_data, 0xdead_beef);

        // Subsequent scan returns 0 because oneshot disarmed
        assert_eq!(ep.scan_ready(&mut events), 0);
    }
}
