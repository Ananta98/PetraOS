// SPDX-License-Identifier: GPL-2.0

//! `pause(2)` system call (Syscall #34 on x86_64).

use ostd::{arch::cpu::context::UserContext, task::Task};

use crate::{
    proc::thread::Thread,
    uapi::errno::{Errno, Result},
};

/// `pause(2)` - suspends execution until a signal is delivered.
pub fn sys_pause(_ctx: &mut UserContext) -> Result<usize> {
    loop {
        if Thread::should_interrupt() {
            return Err(Errno::EINTR);
        }
        Task::yield_now();
    }
}
