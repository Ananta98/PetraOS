// SPDX-License-Identifier: GPL-2.0

//! Mouse device emulation (`/dev/input/mice`, `/dev/input/mouseN`).
//!
//! Emulates standard 3-byte PS/2 mouse packets for consumers that do not read
//! raw evdev events (legacy Xorg, GPM, SDL). It registers as an [`InputListener`],
//! accumulates relative motion and button transitions across all input devices,
//! and outputs standard PS/2 packets upon `SYN_REPORT`.

use alloc::collections::VecDeque;
use ostd::sync::{LocalIrqDisabled, SpinLock, WaitQueue, Waiter};

use crate::{
    drivers::input::{InputDevice, InputListener, register_listener},
    fs::poll::PollEvents,
    uapi::{
        errno::{Errno, Result},
        input::{InputEvent, ev, key, rel},
    },
};

const MICE_QUEUE_CAP: usize = 256;

static MICE_ACCUMULATOR: SpinLock<MouseAccumulator, LocalIrqDisabled> =
    SpinLock::new(MouseAccumulator::new());

struct MouseAccumulator {
    dx: i32,
    dy: i32,
    buttons: u8,
    queue: VecDeque<u8>,
    waitq: WaitQueue,
}

impl MouseAccumulator {
    const fn new() -> Self {
        Self {
            dx: 0,
            dy: 0,
            buttons: 0,
            queue: VecDeque::new(),
            waitq: WaitQueue::new(),
        }
    }

    fn push_packet(&mut self) {
        if self.dx == 0 && self.dy == 0 && !self.queue.is_empty() {
            return;
        }

        // Clamp deltas to signed 8-bit range
        let clamped_x = self.dx.clamp(-127, 127);
        let clamped_y = self.dy.clamp(-127, 127);

        let mut b0 = 0x08 | (self.buttons & 0x07);
        if clamped_x < 0 {
            b0 |= 0x10;
        }
        // PS/2 reports Y positive-up; evdev REL_Y is positive-down
        let ps2_y = -clamped_y;
        if ps2_y < 0 {
            b0 |= 0x20;
        }

        if self.queue.len() + 3 <= MICE_QUEUE_CAP {
            self.queue.push_back(b0);
            self.queue.push_back(clamped_x as u8);
            self.queue.push_back(ps2_y as u8);
        }

        self.dx = 0;
        self.dy = 0;
        if crate::arch::can_wait_ipi() {
            self.waitq.wake_all();
        } else if crate::kpi::task::is_initialized() {
            crate::kpi::task::schedule_work_fn(|| {
                let acc = MICE_ACCUMULATOR.lock();
                acc.waitq.wake_all();
            });
        }
        crate::fs::poll::notify();
    }
}

struct MiceListener;

impl InputListener for MiceListener {
    fn event(&self, _device: &InputDevice, event: &InputEvent) {
        let mut acc = MICE_ACCUMULATOR.lock();
        match event.type_ {
            ev::REL => match event.code {
                rel::X => acc.dx += event.value,
                rel::Y => acc.dy += event.value,
                _ => {}
            },
            ev::KEY => {
                let bit = match event.code {
                    key::BTN_LEFT => 0x01,
                    key::BTN_RIGHT => 0x02,
                    key::BTN_MIDDLE => 0x04,
                    _ => 0,
                };
                if bit != 0 {
                    if event.value != 0 {
                        acc.buttons |= bit;
                    } else {
                        acc.buttons &= !bit;
                    }
                }
            }
            ev::SYN if event.code == ev::SYN_REPORT => {
                acc.push_packet();
            }
            _ => {}
        }
    }
}

static MICE_LISTENER: MiceListener = MiceListener;

/// Initializes the `mousedev` listener.
pub fn init() {
    register_listener(&MICE_LISTENER);
}

/// Reads standard 3-byte packets from the mice accumulator.
pub fn read_mice(buf: &mut [u8], nonblock: bool) -> Result<usize> {
    if buf.is_empty() {
        return Ok(0);
    }

    let read_avail = |buf: &mut [u8]| -> usize {
        let mut acc = MICE_ACCUMULATOR.lock();
        let mut count = 0;
        while count < buf.len() {
            if let Some(b) = acc.queue.pop_front() {
                buf[count] = b;
                count += 1;
            } else {
                break;
            }
        }
        count
    };

    let n = read_avail(buf);
    if n > 0 || nonblock {
        if n == 0 && nonblock {
            return Err(Errno::EAGAIN);
        }
        return Ok(n);
    }

    let (waiter, _) = Waiter::new_pair();
    let cond = || -> Option<usize> {
        {
            let acc = MICE_ACCUMULATOR.lock();
            acc.waitq.enqueue(waiter.waker());
        }
        let n = read_avail(buf);
        if n > 0 {
            Some(n)
        } else {
            None
        }
    };

    let cancel = || {
        if crate::proc::thread::Thread::should_interrupt() {
            Err(Errno::EINTR)
        } else {
            Ok(())
        }
    };

    waiter.wait_until_or_cancelled(cond, cancel)
}

/// Poll readiness for `/dev/input/mice`.
pub fn poll_mice() -> PollEvents {
    let acc = MICE_ACCUMULATOR.lock();
    if !acc.queue.is_empty() {
        PollEvents::IN | PollEvents::RDNORM
    } else {
        PollEvents::empty()
    }
}
