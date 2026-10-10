// SPDX-License-Identifier: GPL-2.0

//! `pause(2)` system call (Syscall #34 on x86_64).

use ostd::{arch::cpu::context::UserContext, task::Task};

use crate::{
    api::errno::{Errno, Result},
    proc::thread::{Thread, ThreadFlags},
};

/// `pause(2)` - suspends execution until a signal is delivered.
pub fn sys_pause(_ctx: &mut UserContext) -> Result<usize> {
    loop {
        if let Some(td) = Thread::current()
            && td.inner.lock().flags.contains(ThreadFlags::TDF_ASTPENDING)
        {
            return Err(Errno::EINTR);
        }
        Task::yield_now();
    }
}
