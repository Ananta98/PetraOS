use crate::fs::epoll::{EPOLL_CTL_DEL, EpollEvent, EpollFile};
use crate::fs::fd_table::FileDescriptor;
use crate::proc::process::Process;
use crate::syscall::time::monotonic_ns;
use crate::syscall::{SyscallResult, to_continue_i32};
use crate::vm::vma::VmaManager;
use alloc::boxed::Box;
use ostd::Error;
use ostd::arch::cpu::context::UserContext;

const EPOLL_EVENT_SIZE: usize = 12;

fn epoll_event_from_bytes(buf: &[u8; EPOLL_EVENT_SIZE]) -> EpollEvent {
    let events = u32::from_ne_bytes([buf[0], buf[1], buf[2], buf[3]]);
    let data = u64::from_ne_bytes([
        buf[4], buf[5], buf[6], buf[7], buf[8], buf[9], buf[10], buf[11],
    ]);
    EpollEvent { events, data }
}

/// **`epoll_create(int size)`** — SYS 213
pub fn syscall_epoll_create(
    arg0: usize,
    _arg1: usize,
    _arg2: usize,
    _arg3: usize,
    _arg4: usize,
    _arg5: usize,
    vm: &VmaManager,
    context: &mut UserContext,
) -> SyscallResult {
    let size = arg0 as i32;
    if size <= 0 {
        return to_continue_i32(Err(Error::InvalidArgs));
    }
    syscall_epoll_create1(0, 0, 0, 0, 0, 0, vm, context)
}

/// **`epoll_create1(int flags)`** — SYS 291
pub fn syscall_epoll_create1(
    arg0: usize,
    _arg1: usize,
    _arg2: usize,
    _arg3: usize,
    _arg4: usize,
    _arg5: usize,
    _vm: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    let flags = arg0 as u32;
    let proc = Process::current();
    let mut fd_table = proc.fd_table.lock();

    let epoll_file = EpollFile::new(flags);
    let fd_entry = FileDescriptor::new(Box::new(epoll_file), flags);

    match fd_table.alloc_fd(0) {
        Ok(fd) => {
            fd_table.insert(fd, fd_entry);
            to_continue_i32(Ok(fd))
        }
        Err(err) => to_continue_i32(Err(err)),
    }
}

/// **`epoll_ctl(int epfd, int op, int fd, struct epoll_event *event)`** — SYS 233
pub fn syscall_epoll_ctl(
    arg0: usize,
    arg1: usize,
    arg2: usize,
    arg3: usize,
    _arg4: usize,
    _arg5: usize,
    vm: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    let epfd = arg0 as i32;
    let op = arg1 as i32;
    let target_fd = arg2 as i32;
    let event_ptr = arg3;

    let proc = Process::current();
    let fd_table = proc.fd_table.lock();

    let ep_entry = match fd_table.get_fd(epfd) {
        Ok(e) => e,
        Err(err) => return to_continue_i32(Err(err)),
    };

    let open_file = ep_entry.open_file.lock();
    let epoll_file = match open_file
        .file_ops
        .as_any()
        .and_then(|any| any.downcast_ref::<EpollFile>())
    {
        Some(ef) => ef.clone(),
        None => return to_continue_i32(Err(Error::InvalidArgs)),
    };
    drop(open_file);
    drop(fd_table);

    let event = if op != EPOLL_CTL_DEL && event_ptr != 0 {
        let mut buf = [0u8; EPOLL_EVENT_SIZE];
        if vm.copy_from_user(event_ptr, &mut buf).is_err() {
            return to_continue_i32(Err(Error::InvalidArgs));
        }
        Some(epoll_event_from_bytes(&buf))
    } else {
        None
    };

    match epoll_file.ctl(op, target_fd, event) {
        Ok(()) => to_continue_i32(Ok(0)),
        Err(err) => to_continue_i32(Err(err)),
    }
}

/// **`epoll_wait(int epfd, struct epoll_event *events, int maxevents, int timeout)`** — SYS 232
pub fn syscall_epoll_wait(
    arg0: usize,
    arg1: usize,
    arg2: usize,
    arg3: usize,
    _arg4: usize,
    _arg5: usize,
    vm: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    let epfd = arg0 as i32;
    let events_ptr = arg1;
    let maxevents = arg2 as i32;
    let timeout = arg3 as i32;

    if maxevents <= 0 || events_ptr == 0 {
        return to_continue_i32(Err(Error::InvalidArgs));
    }

    let proc = Process::current();
    let epoll_file = {
        let fd_table = proc.fd_table.lock();
        let ep_entry = match fd_table.get_fd(epfd) {
            Ok(e) => e,
            Err(err) => return to_continue_i32(Err(err)),
        };

        let open_file = ep_entry.open_file.lock();
        match open_file
            .file_ops
            .as_any()
            .and_then(|any| any.downcast_ref::<EpollFile>())
        {
            Some(ef) => ef.clone(),
            None => return to_continue_i32(Err(Error::InvalidArgs)),
        }
    };

    let start_ns = monotonic_ns();
    let timeout_ns = if timeout > 0 {
        start_ns.saturating_add(timeout as u64 * 1_000_000)
    } else {
        0
    };

    loop {
        let ready = epoll_file.poll_ready();
        if !ready.is_empty() {
            let count = core::cmp::min(ready.len(), maxevents as usize);
            let mut bytes = alloc::vec![0u8; count * EPOLL_EVENT_SIZE];
            for (i, evt) in ready.iter().take(count).enumerate() {
                let chunk = &mut bytes[i * EPOLL_EVENT_SIZE..(i + 1) * EPOLL_EVENT_SIZE];
                chunk[0..4].copy_from_slice(&evt.events.to_ne_bytes());
                chunk[4..12].copy_from_slice(&evt.data.to_ne_bytes());
            }

            if vm.copy_to_user(events_ptr, &bytes).is_ok() {
                return to_continue_i32(Ok(count as i32));
            } else {
                return to_continue_i32(Err(Error::InvalidArgs));
            }
        }

        if timeout == 0 {
            return to_continue_i32(Ok(0));
        }

        if timeout > 0 {
            if monotonic_ns() >= timeout_ns {
                return to_continue_i32(Ok(0));
            }
        }

        ostd::task::Task::yield_now();
    }
}

/// **`epoll_pwait(int epfd, struct epoll_event *events, int maxevents, int timeout, const sigset_t *sigmask)`** — SYS 281
pub fn syscall_epoll_pwait(
    arg0: usize,
    arg1: usize,
    arg2: usize,
    arg3: usize,
    arg4: usize,
    arg5: usize,
    vm: &VmaManager,
    context: &mut UserContext,
) -> SyscallResult {
    syscall_epoll_wait(arg0, arg1, arg2, arg3, arg4, arg5, vm, context)
}
