use crate::fs::vfs::{DirEntry, FileOps, Result as VfsResult, SeekFrom};
use crate::proc::process::Process;
use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::any::Any;
use ostd::Error;
use ostd::sync::SpinLock;

/// Linux epoll event representation (`struct epoll_event`).
#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default)]
pub struct EpollEvent {
    pub events: u32,
    pub data: u64,
}

pub const EPOLLIN: u32 = 0x001;
pub const EPOLLPRI: u32 = 0x002;
pub const EPOLLOUT: u32 = 0x004;
pub const EPOLLERR: u32 = 0x008;
pub const EPOLLHUP: u32 = 0x010;
pub const EPOLLRDHUP: u32 = 0x2000;
pub const EPOLLET: u32 = 0x80000000;
pub const EPOLLONESHOT: u32 = 0x40000000;

pub const EPOLL_CTL_ADD: i32 = 1;
pub const EPOLL_CTL_DEL: i32 = 2;
pub const EPOLL_CTL_MOD: i32 = 3;

#[derive(Clone)]
pub struct EpollItem {
    pub target_fd: i32,
    pub event: EpollEvent,
}

/// An epoll virtual file instance managing monitored target file descriptors.
#[derive(Clone)]
pub struct EpollFile {
    pub items: Arc<SpinLock<BTreeMap<i32, EpollItem>>>,
    pub flags: u32,
}

impl EpollFile {
    pub fn new(flags: u32) -> Self {
        Self {
            items: Arc::new(SpinLock::new(BTreeMap::new())),
            flags,
        }
    }

    pub fn ctl(&self, op: i32, target_fd: i32, event: Option<EpollEvent>) -> Result<(), Error> {
        let mut items = self.items.lock();
        match op {
            EPOLL_CTL_ADD => {
                let evt = event.ok_or(Error::InvalidArgs)?;
                if items.contains_key(&target_fd) {
                    return Err(Error::InvalidArgs);
                }
                items.insert(
                    target_fd,
                    EpollItem {
                        target_fd,
                        event: evt,
                    },
                );
                Ok(())
            }
            EPOLL_CTL_DEL => {
                if items.remove(&target_fd).is_some() {
                    Ok(())
                } else {
                    Err(Error::InvalidArgs)
                }
            }
            EPOLL_CTL_MOD => {
                let evt = event.ok_or(Error::InvalidArgs)?;
                if let Some(item) = items.get_mut(&target_fd) {
                    item.event = evt;
                    Ok(())
                } else {
                    Err(Error::InvalidArgs)
                }
            }
            _ => Err(Error::InvalidArgs),
        }
    }

    pub fn poll_ready(&self) -> Vec<EpollEvent> {
        let items = self.items.lock();
        let proc = Process::current();
        let fd_table = proc.fd_table.lock();
        let mut ready_events = Vec::new();

        for (&fd, item) in items.iter() {
            if let Ok(entry) = fd_table.get_fd(fd) {
                let open_file = entry.open_file.lock();
                let mut revents = 0u32;

                if let Some(socket_file) = open_file
                    .file_ops
                    .as_any()
                    .and_then(|any| any.downcast_ref::<crate::syscall::net::SocketFile>())
                {
                    let mut stack_guard = crate::net::NET_STACK.lock();
                    if let Some(stack) = stack_guard.as_mut() {
                        let handle = *socket_file.handle.lock();
                        if socket_file.socket_type == 1 {
                            let tcp = stack.sockets.get_mut::<smoltcp::socket::tcp::Socket>(handle);
                            if tcp.can_recv() {
                                revents |= EPOLLIN;
                            }
                            if tcp.can_send() {
                                revents |= EPOLLOUT;
                            }
                        } else if socket_file.socket_type == 2 {
                            let udp = stack.sockets.get_mut::<smoltcp::socket::udp::Socket>(handle);
                            if udp.can_recv() {
                                revents |= EPOLLIN;
                            }
                            if udp.can_send() {
                                revents |= EPOLLOUT;
                            }
                        }
                    }
                } else {
                    // Regular files and pipes are ready for read and write
                    revents |= EPOLLIN | EPOLLOUT;
                }

                let matched = item.event.events & revents;
                if matched != 0 {
                    ready_events.push(EpollEvent {
                        events: matched,
                        data: item.event.data,
                    });
                }
            }
        }

        ready_events
    }
}

impl FileOps for EpollFile {
    fn read(&mut self, _buf: &mut [u8], _offset: &mut usize) -> VfsResult<usize> {
        Err(Error::InvalidArgs)
    }

    fn write(&mut self, _buf: &[u8], _offset: &mut usize) -> VfsResult<usize> {
        Err(Error::InvalidArgs)
    }

    fn seek(&mut self, _pos: SeekFrom, _offset: &mut usize) -> VfsResult<usize> {
        Err(Error::InvalidArgs)
    }

    fn readdir(&mut self) -> VfsResult<Vec<DirEntry>> {
        Err(Error::InvalidArgs)
    }

    fn as_any(&self) -> Option<&dyn Any> {
        Some(self)
    }
}
