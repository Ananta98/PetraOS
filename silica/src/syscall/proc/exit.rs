// SPDX-License-Identifier: GPL-2.0

use crate::{
    proc::{
        ExitStatus,
        lifecycle::{exit_thread, exit1},
        thread::Thread,
    },
    uapi::errno::Result,
};
use ostd::arch::cpu::context::UserContext;

pub fn sys_exit(code: i32, _ctx: &mut UserContext) -> Result<usize> {
    let td = Thread::current().expect("must be in thread context");
    exit_thread(&td, code);
    Ok(0)
}

pub fn sys_exit_group(code: i32, _ctx: &mut UserContext) -> Result<usize> {
    let td = Thread::current().expect("must be in thread context");
    exit1(&td, ExitStatus::Exited(code));
    Ok(0)
}
