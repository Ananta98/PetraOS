// SPDX-License-Identifier: GPL-2.0

//! System calls for synchronous I/O multiplexing: `poll`, `ppoll`, `select`, `pselect6`.

use ostd::{arch::cpu::context::UserContext, task::Task};

use crate::{
    api::errno::{Errno, Result},
    fs::poll::{PollEvents, PollFd},
    proc::thread::{Thread, ThreadFlags},
    syscall::fs::utils::UserPtr,
    vm::vmar::Vmar,
};

pub const FD_SETSIZE: usize = 1024;

/// Linux `fd_set` for `select(2)`.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct FdSet {
    pub fds_bits: [u64; FD_SETSIZE / 64],
}

impl FdSet {
    pub fn is_set(&self, fd: usize) -> bool {
        if fd >= FD_SETSIZE {
            return false;
        }
        (self.fds_bits[fd / 64] & (1u64 << (fd % 64))) != 0
    }

    pub fn set(&mut self, fd: usize) {
        if fd < FD_SETSIZE {
            self.fds_bits[fd / 64] |= 1u64 << (fd % 64);
        }
    }
}

fn read_fd_set(vmar: &Vmar, ptr: usize) -> Result<FdSet> {
    let mut set = FdSet::default();
    let byte_len = core::mem::size_of::<FdSet>();
    let mut raw = alloc::vec![0u8; byte_len];
    UserPtr::new(ptr).read_bytes(vmar, &mut raw)?;
    for (i, chunk) in raw.chunks_exact(8).enumerate() {
        set.fds_bits[i] = u64::from_ne_bytes(chunk.try_into().unwrap());
    }
    Ok(set)
}

fn write_fd_set(vmar: &Vmar, ptr: usize, set: &FdSet) -> Result<()> {
    let byte_len = core::mem::size_of::<FdSet>();
    let mut raw = alloc::vec![0u8; byte_len];
    for (i, &word) in set.fds_bits.iter().enumerate() {
        raw[i * 8..(i + 1) * 8].copy_from_slice(&word.to_ne_bytes());
    }
    UserPtr::new(ptr).write_bytes(vmar, &raw)
}

/// System call `poll(struct pollfd *fds, nfds_t nfds, int timeout)` (Syscall #7).
pub fn sys_poll(
    fds_ptr: usize,
    nfds: usize,
    timeout_ms: i32,
    _ctx: &mut UserContext,
) -> Result<usize> {
    if nfds > 1024 {
        return Err(Errno::EINVAL);
    }
    if nfds == 0 {
        if timeout_ms > 0 {
            let start_tick = crate::sched::now();
            let dur_ticks = (timeout_ms as u64 * crate::sched::TIMER_HZ).div_ceil(1000);
            while crate::sched::now().saturating_sub(start_tick) < dur_ticks {
                Task::yield_now();
            }
        }
        return Ok(0);
    }

    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let vmar = proc.vmspace();

    let byte_len = nfds * core::mem::size_of::<PollFd>();
    let mut raw = alloc::vec![0u8; byte_len];
    UserPtr::new(fds_ptr).read_bytes(&vmar, &mut raw)?;

    let mut fds = alloc::vec![PollFd::default(); nfds];
    for (i, pfd) in fds.iter_mut().enumerate() {
        let off = i * 8;
        pfd.fd = i32::from_ne_bytes(raw[off..off + 4].try_into().unwrap());
        pfd.events = i16::from_ne_bytes(raw[off + 4..off + 6].try_into().unwrap());
        pfd.revents = i16::from_ne_bytes(raw[off + 6..off + 8].try_into().unwrap());
    }

    let start_tick = crate::sched::now();
    let has_timeout = timeout_ms >= 0;
    let dur_ticks = if timeout_ms > 0 {
        (timeout_ms as u64 * crate::sched::TIMER_HZ).div_ceil(1000)
    } else {
        0
    };

    loop {
        let mut ready_count = 0;
        {
            let table = proc.fd_table.lock();
            for pfd in fds.iter_mut() {
                pfd.revents = 0;
                if pfd.fd < 0 {
                    continue;
                }
                let revents = match table.get(pfd.fd) {
                    Err(_) => PollEvents::NVAL,
                    Ok(file) => {
                        let want = PollEvents::from_bits_truncate(pfd.events as u16) | PollEvents::ALWAYS;
                        file.poll() & want
                    }
                };
                pfd.revents = revents.bits() as i16;
                if !revents.is_empty() {
                    ready_count += 1;
                }
            }
        }

        if ready_count > 0 {
            for (i, pfd) in fds.iter().enumerate() {
                let off = i * 8 + 6;
                let revents_bytes = pfd.revents.to_ne_bytes();
                UserPtr::new(fds_ptr + off).write_bytes(&vmar, &revents_bytes)?;
            }
            return Ok(ready_count);
        }

        if has_timeout
            && (timeout_ms == 0 || crate::sched::now().saturating_sub(start_tick) >= dur_ticks)
        {
            for (i, pfd) in fds.iter().enumerate() {
                let off = i * 8 + 6;
                let revents_bytes = pfd.revents.to_ne_bytes();
                UserPtr::new(fds_ptr + off).write_bytes(&vmar, &revents_bytes)?;
            }
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

/// System call `ppoll(struct pollfd *fds, nfds_t nfds, const struct timespec *tmo_p, const sigset_t *sigmask, size_t sigsetsize)` (Syscall #271).
pub fn sys_ppoll(
    fds_ptr: usize,
    nfds: usize,
    tmo_ptr: usize,
    _sigmask_ptr: usize,
    _sigsetsize: usize,
    ctx: &mut UserContext,
) -> Result<usize> {
    let timeout_ms = if tmo_ptr == 0 {
        -1
    } else {
        let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
        let vmar = proc.vmspace();
        let mut raw = [0u8; 16];
        UserPtr::new(tmo_ptr).read_bytes(&vmar, &mut raw)?;
        let sec = i64::from_ne_bytes(raw[0..8].try_into().unwrap());
        let nsec = i64::from_ne_bytes(raw[8..16].try_into().unwrap());
        if sec < 0 || !(0..1_000_000_000).contains(&nsec) {
            return Err(Errno::EINVAL);
        }
        (sec.saturating_mul(1000)).saturating_add(nsec / 1_000_000) as i32
    };

    sys_poll(fds_ptr, nfds, timeout_ms, ctx)
}

fn do_select(
    nfds: i32,
    readfds_ptr: usize,
    writefds_ptr: usize,
    exceptfds_ptr: usize,
    timeout_ms: i32,
) -> Result<usize> {
    if nfds < 0 || nfds as usize > FD_SETSIZE {
        return Err(Errno::EINVAL);
    }
    if nfds == 0 {
        if timeout_ms > 0 {
            let start_tick = crate::sched::now();
            let dur_ticks = (timeout_ms as u64 * crate::sched::TIMER_HZ).div_ceil(1000);
            while crate::sched::now().saturating_sub(start_tick) < dur_ticks {
                Task::yield_now();
            }
        }
        return Ok(0);
    }

    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let vmar = proc.vmspace();

    let rfds = if readfds_ptr != 0 {
        Some(read_fd_set(&vmar, readfds_ptr)?)
    } else {
        None
    };
    let wfds = if writefds_ptr != 0 {
        Some(read_fd_set(&vmar, writefds_ptr)?)
    } else {
        None
    };
    let efds = if exceptfds_ptr != 0 {
        Some(read_fd_set(&vmar, exceptfds_ptr)?)
    } else {
        None
    };

    let start_tick = crate::sched::now();
    let has_timeout = timeout_ms >= 0;
    let dur_ticks = if timeout_ms > 0 {
        (timeout_ms as u64 * crate::sched::TIMER_HZ).div_ceil(1000)
    } else {
        0
    };

    loop {
        let mut ready_count = 0;
        let mut out_rfds = FdSet::default();
        let mut out_wfds = FdSet::default();
        let mut out_efds = FdSet::default();

        {
            let table = proc.fd_table.lock();
            for fd in 0..nfds as usize {
                let want_r = rfds.as_ref().is_some_and(|s| s.is_set(fd));
                let want_w = wfds.as_ref().is_some_and(|s| s.is_set(fd));
                let want_e = efds.as_ref().is_some_and(|s| s.is_set(fd));

                if !want_r && !want_w && !want_e {
                    continue;
                }

                if let Ok(file) = table.get(fd as i32) {
                    let poll_res = file.poll();
                    if want_r && (poll_res & (PollEvents::IN | PollEvents::HUP | PollEvents::ERR)).bits() != 0 {
                        out_rfds.set(fd);
                        ready_count += 1;
                    }
                    if want_w && (poll_res & (PollEvents::OUT | PollEvents::ERR)).bits() != 0 {
                        out_wfds.set(fd);
                        ready_count += 1;
                    }
                    if want_e && (poll_res & PollEvents::PRI).bits() != 0 {
                        out_efds.set(fd);
                        ready_count += 1;
                    }
                }
            }
        }

        if ready_count > 0 {
            if readfds_ptr != 0 {
                write_fd_set(&vmar, readfds_ptr, &out_rfds)?;
            }
            if writefds_ptr != 0 {
                write_fd_set(&vmar, writefds_ptr, &out_wfds)?;
            }
            if exceptfds_ptr != 0 {
                write_fd_set(&vmar, exceptfds_ptr, &out_efds)?;
            }
            return Ok(ready_count);
        }

        if has_timeout
            && (timeout_ms == 0 || crate::sched::now().saturating_sub(start_tick) >= dur_ticks)
        {
            if readfds_ptr != 0 {
                write_fd_set(&vmar, readfds_ptr, &out_rfds)?;
            }
            if writefds_ptr != 0 {
                write_fd_set(&vmar, writefds_ptr, &out_wfds)?;
            }
            if exceptfds_ptr != 0 {
                write_fd_set(&vmar, exceptfds_ptr, &out_efds)?;
            }
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

/// System call `select(int nfds, fd_set *readfds, fd_set *writefds, fd_set *exceptfds, struct timeval *timeout)` (Syscall #23).
pub fn sys_select(
    nfds: i32,
    readfds_ptr: usize,
    writefds_ptr: usize,
    exceptfds_ptr: usize,
    timeout_ptr: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let timeout_ms = if timeout_ptr == 0 {
        -1
    } else {
        let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
        let vmar = proc.vmspace();
        let mut raw = [0u8; 16];
        UserPtr::new(timeout_ptr).read_bytes(&vmar, &mut raw)?;
        let sec = i64::from_ne_bytes(raw[0..8].try_into().unwrap());
        let usec = i64::from_ne_bytes(raw[8..16].try_into().unwrap());
        if sec < 0 || usec < 0 {
            return Err(Errno::EINVAL);
        }
        (sec.saturating_mul(1000)).saturating_add(usec / 1000) as i32
    };

    do_select(nfds, readfds_ptr, writefds_ptr, exceptfds_ptr, timeout_ms)
}

/// System call `pselect6(int nfds, fd_set *readfds, fd_set *writefds, fd_set *exceptfds, const struct timespec *timeout, const void *sigmask)` (Syscall #270).
pub fn sys_pselect6(
    nfds: i32,
    readfds_ptr: usize,
    writefds_ptr: usize,
    exceptfds_ptr: usize,
    timeout_ptr: usize,
    _sigmask_ptr: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let timeout_ms = if timeout_ptr == 0 {
        -1
    } else {
        let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
        let vmar = proc.vmspace();
        let mut raw = [0u8; 16];
        UserPtr::new(timeout_ptr).read_bytes(&vmar, &mut raw)?;
        let sec = i64::from_ne_bytes(raw[0..8].try_into().unwrap());
        let nsec = i64::from_ne_bytes(raw[8..16].try_into().unwrap());
        if sec < 0 || !(0..1_000_000_000).contains(&nsec) {
            return Err(Errno::EINVAL);
        }
        (sec.saturating_mul(1000)).saturating_add(nsec / 1_000_000) as i32
    };

    do_select(nfds, readfds_ptr, writefds_ptr, exceptfds_ptr, timeout_ms)
}
