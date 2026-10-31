// SPDX-License-Identifier: GPL-2.0

//! Asynchronous I/O framework: `io_uring(7)`.
//!
//! Provides the core data structures and VFS file operations for `io_uring`,
//! supporting submission queue (SQ) batch processing, completion queue (CQ)
//! delivery, non-blocking requests, and shared memory ring buffers.

use alloc::{boxed::Box, collections::VecDeque, sync::Arc};
use ostd::sync::WaitQueue;
use spin::Mutex;

use crate::{
    fs::{
        poll::PollEvents,
        vfs::{File, FileOps, INode, INodeAttr, Mode, NodeOps, OpenFlags},
    },
    uapi::errno::Result,
    vm::backing::{Backing, SharedPages},
};

pub const IORING_SETUP_IOPOLL: u32 = 1 << 0;
pub const IORING_SETUP_SQPOLL: u32 = 1 << 1;
pub const IORING_SETUP_CQSIZE: u32 = 1 << 3;
pub const IORING_SETUP_CLAMP: u32 = 1 << 4;

pub const IORING_FEAT_SINGLE_MMAP: u32 = 1 << 0;
pub const IORING_FEAT_NODROP: u32 = 1 << 1;
pub const IORING_FEAT_SUBMIT_STABLE: u32 = 1 << 2;
pub const IORING_FEAT_FAST_POLL: u32 = 1 << 5;

pub const IORING_ENTER_GETEVENTS: u32 = 1 << 0;
pub const IORING_ENTER_SQ_WAKEUP: u32 = 1 << 1;

pub const IORING_OP_NOP: u8 = 0;
pub const IORING_OP_READV: u8 = 1;
pub const IORING_OP_WRITEV: u8 = 2;
pub const IORING_OP_FSYNC: u8 = 3;
pub const IORING_OP_POLL_ADD: u8 = 6;
pub const IORING_OP_POLL_REMOVE: u8 = 7;
pub const IORING_OP_CLOSE: u8 = 19;
pub const IORING_OP_READ: u8 = 22;
pub const IORING_OP_WRITE: u8 = 23;

pub const IORING_OFF_SQ_RING: u64 = 0;
pub const IORING_OFF_CQ_RING: u64 = 0x800_0000;
pub const IORING_OFF_SQES: u64 = 0x1000_0000;

/// Linux `struct io_uring_sqe` (64 bytes).
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct IoUringSqe {
    pub opcode: u8,
    pub flags: u8,
    pub ioprio: u16,
    pub fd: i32,
    pub off: u64,
    pub addr: u64,
    pub len: u32,
    pub rw_flags: u32,
    pub user_data: u64,
    pub pad: [u8; 24],
}

/// Linux `struct io_uring_cqe` (16 bytes).
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct IoUringCqe {
    pub user_data: u64,
    pub res: i32,
    pub flags: u32,
}

/// Linux `struct io_sqring_offsets`.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct IoSqringOffsets {
    pub head: u32,
    pub tail: u32,
    pub ring_mask: u32,
    pub ring_entries: u32,
    pub flags: u32,
    pub dropped: u32,
    pub array: u32,
    pub resv1: u32,
    pub user_addr: u64,
}

/// Linux `struct io_cqring_offsets`.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct IoCqringOffsets {
    pub head: u32,
    pub tail: u32,
    pub ring_mask: u32,
    pub ring_entries: u32,
    pub overflow: u32,
    pub cqes: u32,
    pub flags: u32,
    pub resv1: u32,
    pub user_addr: u64,
}

/// Linux `struct io_uring_params` (120 bytes).
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct IoUringParams {
    pub sq_entries: u32,
    pub cq_entries: u32,
    pub flags: u32,
    pub sq_thread_cpu: u32,
    pub sq_thread_idle: u32,
    pub features: u32,
    pub wq_fd: u32,
    pub resv: [u32; 3],
    pub sq_off: IoSqringOffsets,
    pub cq_off: IoCqringOffsets,
}

/// Core instance representing an open `io_uring` context.
pub struct IoUringInstance {
    pub sq_entries: u32,
    pub cq_entries: u32,
    pub flags: u32,
    pub shared_pages: Arc<SharedPages>,
    pub cqes: Mutex<VecDeque<IoUringCqe>>,
    pub wait_queue: WaitQueue,
}

impl IoUringInstance {
    pub fn new(entries: u32, flags: u32) -> Result<Self> {
        let sq_entries = entries.next_power_of_two().max(1).min(4096);
        let cq_entries = (sq_entries * 2).min(8192);

        // Allocate a 64 KiB shared page backing for rings and buffers
        let shared_pages = SharedPages::new(65536)?;

        Ok(Self {
            sq_entries,
            cq_entries,
            flags,
            shared_pages,
            cqes: Mutex::new(VecDeque::new()),
            wait_queue: WaitQueue::new(),
        })
    }

    /// Appends a completion event to the completion queue.
    pub fn push_cqe(&self, cqe: IoUringCqe) {
        let mut cqes = self.cqes.lock();
        if cqes.len() < self.cq_entries as usize {
            cqes.push_back(cqe);
        }
        drop(cqes);
        self.wait_queue.wake_all();
        crate::fs::poll::notify();
    }

    /// Checks if any completed CQE is ready.
    pub fn has_completions(&self) -> bool {
        !self.cqes.lock().is_empty()
    }
}

pub struct IoUringFileOps {
    pub instance: Arc<IoUringInstance>,
}

impl FileOps for IoUringFileOps {
    fn read(&self, _offset: u64, _buf: &mut [u8]) -> Result<usize> {
        crate::return_errno!(EINVAL, "cannot read directly from io_uring descriptor")
    }

    fn write(&self, _offset: u64, _buf: &[u8]) -> Result<usize> {
        crate::return_errno!(EINVAL, "cannot write directly to io_uring descriptor")
    }

    fn poll(&self) -> PollEvents {
        let mut ev = PollEvents::OUT | PollEvents::WRNORM;
        if self.instance.has_completions() {
            ev |= PollEvents::IN | PollEvents::RDNORM;
        }
        ev
    }

    fn as_io_uring(&self) -> Option<Arc<IoUringInstance>> {
        Some(self.instance.clone())
    }

    fn mmap(&self, _offset: u64, _len: usize) -> Result<()> {
        Ok(())
    }

    fn mmap_backing(&self, _offset: u64, _len: usize) -> Result<Option<Arc<dyn Backing>>> {
        Ok(Some(self.instance.shared_pages.clone() as Arc<dyn Backing>))
    }
}

pub struct IoUringNodeOps;

impl NodeOps for IoUringNodeOps {
    fn open(&self, _flags: OpenFlags) -> Result<Box<dyn FileOps>> {
        crate::return_errno!(EOPNOTSUPP, "cannot directly open io_uring inode")
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

pub fn create_io_uring_file(entries: u32, flags: u32) -> Result<Arc<File>> {
    let instance = Arc::new(IoUringInstance::new(entries, flags)?);
    let inode = Arc::new(INode::new(
        Box::new(IoUringNodeOps),
        Mode::FIFO | Mode::RUSR | Mode::WUSR,
        902,
    ));
    Ok(Arc::new(File::new(
        Box::new(IoUringFileOps { instance }),
        inode,
        OpenFlags::READ | OpenFlags::WRITE,
        false,
    )))
}

#[cfg(ktest)]
mod tests {
    use super::*;
    use ostd::prelude::ktest;

    #[ktest]
    fn io_uring_instance_setup_and_cqe() {
        let instance = IoUringInstance::new(32, 0).expect("io_uring new");
        assert_eq!(instance.sq_entries, 32);
        assert_eq!(instance.cq_entries, 64);
        assert!(!instance.has_completions());

        instance.push_cqe(IoUringCqe {
            user_data: 0x1122_3344,
            res: 42,
            flags: 0,
        });

        assert!(instance.has_completions());
        let cqe = instance.cqes.lock().pop_front().unwrap();
        assert_eq!(cqe.user_data, 0x1122_3344);
        assert_eq!(cqe.res, 42);
    }
}
