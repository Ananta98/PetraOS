// SPDX-License-Identifier: GPL-2.0

//! System calls for timerfd: `timerfd_create`, `timerfd_settime`, `timerfd_gettime`.

use ostd::arch::cpu::context::UserContext;

use crate::{
    fs::{
        create_timerfd_file,
        fd::FdFlags,
        timer_fd::{TimerFd, is_supported_clock},
    },
    proc::thread::Thread,
    syscall::fs::utils::UserPtr,
    time::Itimerspec,
    uapi::{
        errno::{Errno, Result},
        fd::{ITIMERSPEC_RAW_SIZE, TFD_CLOEXEC, TFD_CREATE_FLAGS_MASK, TFD_SETTIME_FLAGS_MASK},
    },
};

/// System call `timerfd_create(int clockid, int flags)` (Syscall #283).
pub fn sys_timerfd_create(clockid: i32, flags: i32, _ctx: &mut UserContext) -> Result<usize> {
    if !is_supported_clock(clockid) || (flags & !TFD_CREATE_FLAGS_MASK) != 0 {
        return Err(Errno::EINVAL);
    }

    let file = create_timerfd_file(clockid, flags);
    let mut descriptor_flags = FdFlags::empty();
    if (flags & TFD_CLOEXEC) != 0 {
        descriptor_flags |= FdFlags::FD_CLOEXEC;
    }

    let proc = Thread::proc_or_esrch()?;
    let new_fd = proc.fd_table.lock().alloc_fd(file, descriptor_flags)?;
    Ok(new_fd as usize)
}

/// System call `timerfd_settime(int fd, int flags, const struct itimerspec *new_value, struct itimerspec *old_value)` (Syscall #286).
pub fn sys_timerfd_settime(
    fd: i32,
    flags: i32,
    new_value_ptr: usize,
    old_value_ptr: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    if (flags & !TFD_SETTIME_FLAGS_MASK) != 0 || new_value_ptr == 0 {
        return Err(Errno::EINVAL);
    }

    let proc = Thread::proc_or_esrch()?;
    let target_file = proc.file(fd)?;
    let timerfd = target_file.as_timerfd().ok_or(Errno::EINVAL)?;
    let vmar = proc.vmspace();

    let mut spec_raw = [0u8; ITIMERSPEC_RAW_SIZE];
    UserPtr::new(new_value_ptr).read_bytes(&vmar, &mut spec_raw)?;
    let new_spec = Itimerspec::from_bytes(&spec_raw).ok_or(Errno::EINVAL)?;
    if !new_spec.it_value.is_valid() || !new_spec.it_interval.is_valid() {
        return Err(Errno::EINVAL);
    }

    let previous_spec = TimerFd::set_time(&timerfd, flags, &new_spec)?;

    if old_value_ptr != 0 {
        UserPtr::new(old_value_ptr).write_bytes(&vmar, &previous_spec.to_bytes())?;
    }

    Ok(0)
}

/// System call `timerfd_gettime(int fd, struct itimerspec *curr_value)` (Syscall #287).
pub fn sys_timerfd_gettime(
    fd: i32,
    curr_value_ptr: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    if curr_value_ptr == 0 {
        return Err(Errno::EFAULT);
    }

    let proc = Thread::proc_or_esrch()?;
    let target_file = proc.file(fd)?;
    let timerfd = target_file.as_timerfd().ok_or(Errno::EINVAL)?;
    let vmar = proc.vmspace();

    let current_spec = timerfd.get_time();
    UserPtr::new(curr_value_ptr).write_bytes(&vmar, &current_spec.to_bytes())?;
    Ok(0)
}
