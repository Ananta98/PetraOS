// SPDX-License-Identifier: GPL-2.0

//! System calls for epoll: `epoll_create`, `epoll_create1`, `epoll_ctl`, `epoll_wait`, `epoll_pwait`.

use ostd::{arch::cpu::context::UserContext, task::Task};

use crate::{
    api::errno::{Errno, Result},
    fs::{
        epoll::{
            create_epoll_file, EpollEvent, EPOLL_CLOEXEC, EPOLL_CTL_ADD, EPOLL_CTL_DEL,
            EPOLL_CTL_MOD,
        },
        fd::FdFlags,
    },
    proc::thread::{Thread, ThreadFlags},
    syscall::fs::utils::{read_user_bytes, write_user_bytes},
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

    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
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

    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let ep_file = proc.fd_table.lock().get(epfd)?;
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
            let target_file = proc.fd_table.lock().get(fd)?;
            let vmar = proc.vmspace();
            let mut raw = [0u8; 12];
            read_user_bytes(&vmar, event_ptr, &mut raw)?;
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
            let mut raw = [0u8; 12];
            read_user_bytes(&vmar, event_ptr, &mut raw)?;
            let events = u32::from_ne_bytes(raw[0..4].try_into().unwrap());
            let data = u64::from_ne_bytes(raw[4..12].try_into().unwrap());
            epoll.modify(fd, events, data)?;
            Ok(0)
        }
        _ => Err(Errno::EINVAL),
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
    if maxevents <= 0 || maxevents > 1024 {
        return Err(Errno::EINVAL);
    }
    if events_ptr == 0 {
        return Err(Errno::EFAULT);
    }

    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let ep_file = proc.fd_table.lock().get(epfd)?;
    let epoll = ep_file.as_epoll().ok_or(Errno::EINVAL)?;
    let vmar = proc.vmspace();

    let max_n = maxevents as usize;
    let mut ready_evs = alloc::vec![EpollEvent::default(); max_n];

    let start_tick = crate::sched::now();
    let has_timeout = timeout_ms >= 0;
    let dur_ticks = if timeout_ms > 0 {
        (timeout_ms as u64 * crate::sched::TIMER_HZ).div_ceil(1000)
    } else {
        0
    };

    loop {
        let ready = epoll.scan_ready(&mut ready_evs);
        if ready > 0 {
            for (i, ev) in ready_evs.iter().take(ready).enumerate() {
                let off = i * 12;
                let mut raw = [0u8; 12];
                raw[0..4].copy_from_slice(&ev.events.to_ne_bytes());
                raw[4..12].copy_from_slice(&ev.data.to_ne_bytes());
                write_user_bytes(&vmar, events_ptr + off, &raw)?;
            }
            return Ok(ready);
        }

        if has_timeout
            && (timeout_ms == 0 || crate::sched::now().saturating_sub(start_tick) >= dur_ticks)
        {
            return Ok(0);
        }

        if let Some(td) = Thread::current() {
            if td.inner.lock().flags.contains(ThreadFlags::TDF_ASTPENDING) {
                return Err(Errno::EINTR);
            }
        }

        Task::yield_now();
    }
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
    if sigmask_ptr != 0 && sigsetsize != 8 {
        return Err(Errno::EINVAL);
    }
    sys_epoll_wait(epfd, events_ptr, maxevents, timeout_ms, ctx)
}
