// SPDX-License-Identifier: GPL-2.0

//! Timer notification file descriptors (`timerfd_create(2)`, `timerfd_settime(2)`, `timerfd_gettime(2)`).
//!
//! Delivers timer expiration notifications as file descriptor events for polling,
//! epoll multiplexing, and synchronous reading.

use alloc::{boxed::Box, sync::Arc};
use core::time::Duration;
use ostd::sync::WaitQueue;
use spin::Mutex;

use crate::{
    fs::{
        poll::PollEvents,
        vfs::{File, FileOps, INode, INodeAttr, Mode, NodeOps, OpenFlags},
    },
    proc::thread::Thread,
    time::{Itimerspec, Timespec, posix_timer::get_clock_now},
    uapi::{
        errno::{Errno, Result},
        fd::{
            NANOS_PER_SEC, TFD_NONBLOCK, TFD_TIMER_ABSTIME, TIMERFD_PAYLOAD_SIZE,
        },
    },
};

pub use crate::uapi::fd::{
    ITIMERSPEC_RAW_SIZE, TFD_CLOEXEC, TFD_CREATE_FLAGS_MASK, TFD_SETTIME_FLAGS_MASK,
    TFD_TIMER_CANCEL_ON_SET,
};

const TIMERFD_INODE_NUMBER: usize = 904;

pub struct TimerFdInner {
    pub interval_ns: u64,
    pub next_deadline_ns: u64,
    pub expirations: u64,
    pub armed: bool,
    pub generation: u64,
    pub ktimer: Option<crate::kpi::time::KTimer>,
}

impl TimerFdInner {
    fn cancel_active_timer(&mut self) {
        if let Some(active_ktimer) = self.ktimer.take() {
            active_ktimer.cancel();
        }
    }

    fn disarm(&mut self) {
        self.cancel_active_timer();
        self.armed = false;
        self.interval_ns = 0;
        self.next_deadline_ns = 0;
        self.expirations = 0;
    }

    fn arm(&mut self, interval_ns: u64, deadline_ns: u64) {
        self.cancel_active_timer();
        self.armed = true;
        self.interval_ns = interval_ns;
        self.next_deadline_ns = deadline_ns;
        self.expirations = 0;
    }

    fn advance_expirations(&mut self, now_ns: u64) {
        if !self.armed || now_ns < self.next_deadline_ns {
            return;
        }

        let interval_ns = self.interval_ns;
        if interval_ns == 0 {
            self.expirations = self.expirations.saturating_add(1);
            self.armed = false;
            self.next_deadline_ns = 0;
            self.cancel_active_timer();
            return;
        }

        let elapsed_ns = now_ns.saturating_sub(self.next_deadline_ns);
        let expired_periods = 1 + (elapsed_ns / interval_ns);
        self.expirations = self.expirations.saturating_add(expired_periods);
        self.next_deadline_ns = self
            .next_deadline_ns
            .saturating_add(expired_periods * interval_ns);
    }
}

/// Internal state of a timerfd descriptor.
pub struct TimerFd {
    pub clockid: i32,
    pub inner: Mutex<TimerFdInner>,
    pub wait_queue: WaitQueue,
}

impl TimerFd {
    pub fn new(clockid: i32) -> Self {
        Self {
            clockid,
            inner: Mutex::new(TimerFdInner {
                interval_ns: 0,
                next_deadline_ns: 0,
                expirations: 0,
                armed: false,
                generation: 0,
                ktimer: None,
            }),
            wait_queue: WaitQueue::new(),
        }
    }

    fn check_and_update_expirations(&self) {
        let now_ns = get_clock_now(self.clockid);
        self.inner.lock().advance_expirations(now_ns);
    }

    fn schedule_next_expire_work(timer: &Arc<TimerFd>, generation: u64, delay_ns: u64) {
        let timer_clone = Arc::clone(timer);
        let ktimer = crate::kpi::task::schedule_delayed_work_fn(
            move || {
                Self::on_expire(timer_clone, generation);
            },
            Duration::from_nanos(delay_ns),
        );
        timer.inner.lock().ktimer = Some(ktimer);
    }

    pub fn on_expire(timer: Arc<TimerFd>, generation: u64) {
        let mut inner = timer.inner.lock();
        if inner.generation != generation || !inner.armed {
            return;
        }

        inner.expirations = inner.expirations.saturating_add(1);
        let interval_ns = inner.interval_ns;

        if interval_ns > 0 {
            let now_ns = get_clock_now(timer.clockid);
            inner.next_deadline_ns = now_ns.saturating_add(interval_ns);
        } else {
            inner.armed = false;
            inner.next_deadline_ns = 0;
            inner.ktimer = None;
        }
        drop(inner);

        timer.wait_queue.wake_all();
        crate::fs::poll::notify();

        if interval_ns > 0 {
            Self::schedule_next_expire_work(&timer, generation, interval_ns);
        }
    }

    pub fn set_time(
        timer: &Arc<TimerFd>,
        flags: i32,
        new_value: &Itimerspec,
    ) -> Result<Itimerspec> {
        let old_value = timer.get_time();
        let is_abstime = (flags & TFD_TIMER_ABSTIME) != 0;

        let interval_ns = new_value.it_interval.total_nanos().unwrap_or(0);
        let value_ns = new_value.it_value.total_nanos().unwrap_or(0);

        let mut inner = timer.inner.lock();
        inner.generation = inner.generation.wrapping_add(1);
        let current_gen = inner.generation;

        if value_ns == 0 {
            inner.disarm();
            return Ok(old_value);
        }

        let now_ns = get_clock_now(timer.clockid);
        let deadline_ns = if is_abstime {
            value_ns
        } else {
            now_ns.saturating_add(value_ns)
        };

        inner.arm(interval_ns, deadline_ns);
        drop(inner);

        let delay_ns = deadline_ns.saturating_sub(now_ns).max(1);
        Self::schedule_next_expire_work(timer, current_gen, delay_ns);

        Ok(old_value)
    }

    pub fn get_time(&self) -> Itimerspec {
        self.check_and_update_expirations();
        let inner = self.inner.lock();

        let interval = nanoseconds_to_timespec(inner.interval_ns);
        if !inner.armed {
            return Itimerspec::new(interval, Timespec::ZERO);
        }

        let now_ns = get_clock_now(self.clockid);
        let remaining_ns = inner.next_deadline_ns.saturating_sub(now_ns);
        let value = nanoseconds_to_timespec(remaining_ns);

        Itimerspec::new(interval, value)
    }

    pub fn read_expirations(&self, dest_buffer: &mut [u8], nonblocking: bool) -> Result<usize> {
        let mut try_read = || -> Option<Result<usize>> {
            self.check_and_update_expirations();
            let mut inner = self.inner.lock();
            let count = inner.expirations;
            if count > 0 {
                inner.expirations = 0;
                drop(inner);
                dest_buffer[..TIMERFD_PAYLOAD_SIZE].copy_from_slice(&count.to_ne_bytes());
                return Some(Ok(TIMERFD_PAYLOAD_SIZE));
            }
            drop(inner);

            if Thread::should_interrupt() {
                return Some(Err(Errno::EINTR));
            }
            None
        };

        if let Some(result) = try_read() {
            return result;
        }
        if nonblocking {
            return Err(Errno::EAGAIN);
        }
        self.wait_queue.wait_until(try_read)
    }

    pub fn poll_events(&self) -> PollEvents {
        self.check_and_update_expirations();
        let inner = self.inner.lock();
        if inner.expirations > 0 {
            PollEvents::IN | PollEvents::RDNORM
        } else {
            PollEvents::empty()
        }
    }
}

fn nanoseconds_to_timespec(nanos: u64) -> Timespec {
    Timespec {
        tv_sec: (nanos / NANOS_PER_SEC) as i64,
        tv_nsec: (nanos % NANOS_PER_SEC) as i64,
    }
}

pub struct TimerFdFileOps {
    instance: Arc<TimerFd>,
    nonblocking: bool,
}

impl FileOps for TimerFdFileOps {
    fn read(&self, _offset: u64, buf: &mut [u8]) -> Result<usize> {
        if buf.len() < TIMERFD_PAYLOAD_SIZE {
            return Err(Errno::EINVAL);
        }
        self.instance.read_expirations(buf, self.nonblocking)
    }

    fn write(&self, _offset: u64, _buf: &[u8]) -> Result<usize> {
        crate::return_errno!(EINVAL, "cannot write to timerfd descriptor")
    }

    fn poll(&self) -> PollEvents {
        self.instance.poll_events()
    }

    fn as_timerfd(&self) -> Option<Arc<TimerFd>> {
        Some(Arc::clone(&self.instance))
    }
}

pub struct TimerFdNodeOps;

impl NodeOps for TimerFdNodeOps {
    fn open(&self, _flags: OpenFlags) -> Result<Box<dyn FileOps>> {
        crate::return_errno!(EOPNOTSUPP, "cannot directly open timerfd inode")
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

/// Creates a new anonymous `File` wrapping a [`TimerFd`].
pub fn create_timerfd_file(clockid: i32, flags: i32) -> Arc<File> {
    let nonblocking = (flags & TFD_NONBLOCK) != 0;
    let instance = Arc::new(TimerFd::new(clockid));

    let inode = Arc::new(INode::new(
        Box::new(TimerFdNodeOps),
        Mode::FIFO | Mode::RUSR | Mode::WUSR,
        TIMERFD_INODE_NUMBER,
    ));

    let mut open_flags = OpenFlags::READ;
    if nonblocking {
        open_flags |= OpenFlags::NONBLOCK;
    }

    Arc::new(File::new(
        Box::new(TimerFdFileOps {
            instance,
            nonblocking,
        }),
        inode,
        open_flags,
        false,
    ))
}

pub fn is_supported_clock(clockid: i32) -> bool {
    matches!(
        clockid,
        crate::time::CLOCK_REALTIME
            | crate::time::CLOCK_MONOTONIC
            | crate::time::CLOCK_BOOTTIME
            | crate::time::CLOCK_REALTIME_ALARM
            | crate::time::CLOCK_BOOTTIME_ALARM
    )
}
