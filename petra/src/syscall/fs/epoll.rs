// SPDX-License-Identifier: GPL-2.0

//! System calls for epoll: `epoll_create`, `epoll_create1`, `epoll_ctl`, `epoll_wait`, `epoll_pwait`.

use ostd::{arch::cpu::context::UserContext, task::Task};

use crate::{
    fs::{
        epoll::{
            EPOLL_CLOEXEC, EPOLL_CTL_ADD, EPOLL_CTL_DEL, EPOLL_CTL_MOD, EpollEvent,
            create_epoll_file,
        },
        fd::FdFlags,
    },
    proc::thread::Thread,
    syscall::fs::utils::UserPtr,
    uapi::errno::{Errno, Result},
};

/// System call `epoll_create(int size)` (Syscall #213).
pub fn sys_epoll_create(size: i32, ctx: &mut UserContext) -> Result<usize> {
    if size <= 0 {
        return Err(Errno::EINVAL);
    }
    sys_epoll_create1(0, ctx)
}

/// System call `epoll_create1(int flags)` (Syscall #291).
pub fn sys_epoll_create1(flags: i32, _ctx: &mut UserContext) -> Result<usize> {
    if (flags & !EPOLL_CLOEXEC) != 0 {
        return Err(Errno::EINVAL);
    }

    let file = create_epoll_file();
    let mut fd_flags = FdFlags::empty();
    if (flags & EPOLL_CLOEXEC) != 0 {
        fd_flags |= FdFlags::FD_CLOEXEC;
    }

    let proc = Thread::proc_or_esrch()?;
    let fd = proc.fd_table.lock().alloc_fd(file, fd_flags)?;
    Ok(fd as usize)
}

/// System call `epoll_ctl(int epfd, int op, int fd, struct epoll_event *event)` (Syscall #233).
pub fn sys_epoll_ctl(
    epfd: i32,
    op: i32,
    fd: i32,
    event_ptr: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    if epfd == fd {
        return Err(Errno::EINVAL);
    }

    let proc = Thread::proc_or_esrch()?;
    let ep_file = proc.file(epfd)?;
    let epoll = ep_file.as_epoll().ok_or(Errno::EINVAL)?;

    match op {
        EPOLL_CTL_DEL => {
            epoll.delete(fd)?;
            Ok(0)
        }
        EPOLL_CTL_ADD => {
            if event_ptr == 0 {
                return Err(Errno::EFAULT);
            }
            let target_file = proc.file(fd)?;
            let vmar = proc.vmspace();
            let mut raw = [0u8; EPOLL_EVENT_SIZE];
            UserPtr::new(event_ptr).read_bytes(&vmar, &mut raw)?;
            let events = u32::from_ne_bytes(raw[0..4].try_into().unwrap());
            let data = u64::from_ne_bytes(raw[4..12].try_into().unwrap());
            epoll.add(fd, target_file, events, data)?;
            Ok(0)
        }
        EPOLL_CTL_MOD => {
            if event_ptr == 0 {
                return Err(Errno::EFAULT);
            }
            let vmar = proc.vmspace();
            let mut raw = [0u8; EPOLL_EVENT_SIZE];
            UserPtr::new(event_ptr).read_bytes(&vmar, &mut raw)?;
            let events = u32::from_ne_bytes(raw[0..4].try_into().unwrap());
            let data = u64::from_ne_bytes(raw[4..12].try_into().unwrap());
            epoll.modify(fd, events, data)?;
            Ok(0)
        }
        _ => Err(Errno::EINVAL),
    }
}

const EPOLL_EVENT_SIZE: usize = 12;
const EPOLL_MAX_EVENTS_LIMIT: i32 = 1024;
const TIMESPEC_RAW_SIZE: usize = 16;
const SIGSET_SIZE_BYTES: usize = 8;

fn write_epoll_events(
    events_ptr: usize,
    ready_events: &[EpollEvent],
    ready_count: usize,
    vmar: &alloc::sync::Arc<crate::vm::Vmar>,
) -> Result<()> {
    for (index, event) in ready_events.iter().take(ready_count).enumerate() {
        let byte_offset = index * EPOLL_EVENT_SIZE;
        let mut raw_bytes = [0u8; EPOLL_EVENT_SIZE];
        raw_bytes[0..4].copy_from_slice(&event.events.to_ne_bytes());
        raw_bytes[4..12].copy_from_slice(&event.data.to_ne_bytes());
        UserPtr::new(events_ptr + byte_offset).write_bytes(vmar, &raw_bytes)?;
    }
    Ok(())
}

fn execute_epoll_wait(
    epfd: i32,
    events_ptr: usize,
    maxevents: i32,
    deadline: Option<crate::sched::Deadline>,
) -> Result<usize> {
    if maxevents <= 0 || maxevents > EPOLL_MAX_EVENTS_LIMIT {
        return Err(Errno::EINVAL);
    }
    if events_ptr == 0 {
        return Err(Errno::EFAULT);
    }

    let proc = Thread::proc_or_esrch()?;
    let ep_file = proc.file(epfd)?;
    let epoll = ep_file.as_epoll().ok_or(Errno::EINVAL)?;
    let vmar = proc.vmspace();

    let max_capacity = maxevents as usize;
    let mut ready_events = alloc::vec![EpollEvent::default(); max_capacity];

    loop {
        let ready_count = epoll.scan_ready(&mut ready_events);
        if ready_count > 0 {
            write_epoll_events(events_ptr, &ready_events, ready_count, &vmar)?;
            return Ok(ready_count);
        }

        if deadline.as_ref().is_some_and(|d| d.expired()) {
            return Ok(0);
        }

        if Thread::should_interrupt() {
            return Err(Errno::EINTR);
        }

        Task::yield_now();
    }
}

/// System call `epoll_wait(int epfd, struct epoll_event *events, int maxevents, int timeout)` (Syscall #232).
pub fn sys_epoll_wait(
    epfd: i32,
    events_ptr: usize,
    maxevents: i32,
    timeout_ms: i32,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let deadline = crate::sched::Deadline::from_timeout_ms(timeout_ms);
    execute_epoll_wait(epfd, events_ptr, maxevents, deadline)
}

/// System call `epoll_pwait(int epfd, struct epoll_event *events, int maxevents, int timeout, const sigset_t *sigmask, size_t sigsetsize)` (Syscall #281).
pub fn sys_epoll_pwait(
    epfd: i32,
    events_ptr: usize,
    maxevents: i32,
    timeout_ms: i32,
    sigmask_ptr: usize,
    sigsetsize: usize,
    ctx: &mut UserContext,
) -> Result<usize> {
    if sigmask_ptr != 0 && sigsetsize != SIGSET_SIZE_BYTES {
        return Err(Errno::EINVAL);
    }
    sys_epoll_wait(epfd, events_ptr, maxevents, timeout_ms, ctx)
}

fn parse_timespec_deadline(
    timeout_ts_ptr: usize,
    vmar: &alloc::sync::Arc<crate::vm::Vmar>,
) -> Result<Option<crate::sched::Deadline>> {
    if timeout_ts_ptr == 0 {
        return Ok(None);
    }
    let mut timespec_raw = [0u8; TIMESPEC_RAW_SIZE];
    UserPtr::new(timeout_ts_ptr).read_bytes(vmar, &mut timespec_raw)?;
    let timeout_spec = crate::time::Timespec::from_bytes(&timespec_raw).ok_or(Errno::EINVAL)?;
    if !timeout_spec.is_valid() {
        return Err(Errno::EINVAL);
    }
    let total_nanos = timeout_spec.total_nanos().ok_or(Errno::EINVAL)?;
    Ok(Some(crate::sched::Deadline::from_timeout_ns(total_nanos)))
}

/// System call `epoll_pwait2(int epfd, struct epoll_event *events, int maxevents, const struct timespec *timeout, const sigset_t *sigmask, size_t sigsetsize)` (Syscall #441).
pub fn sys_epoll_pwait2(
    epfd: i32,
    events_ptr: usize,
    maxevents: i32,
    timeout_ts_ptr: usize,
    sigmask_ptr: usize,
    sigsetsize: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    if sigmask_ptr != 0 && sigsetsize != SIGSET_SIZE_BYTES {
        return Err(Errno::EINVAL);
    }

    let proc = Thread::proc_or_esrch()?;
    let vmar = proc.vmspace();
    let deadline = parse_timespec_deadline(timeout_ts_ptr, &vmar)?;

    execute_epoll_wait(epfd, events_ptr, maxevents, deadline)
}
