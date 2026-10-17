// SPDX-License-Identifier: GPL-2.0

//! Process management subsystem.
//!
//! Provides the process control block ([`Proc`]), lifecycle state machine,
//! address space bindings, and hierarchy relationships.

pub mod init;
pub mod job;
pub mod kthread;
pub mod lifecycle;
pub mod signal;
pub mod thread;
pub mod types;

pub use self::{
    job::*,
    lifecycle::*,
    types::*,
};

use alloc::sync::Arc;
use ostd::sync::SpinLock;

use crate::{
    api::{
        cred::Ucred,
        errno::Result,
        limit::Plimit,
        signal::SigActs,
    },
    fs::fd::Filedesc,
    vm::vmar::Vmar,
};

/// Creates the initial user process (`init`, PID 1) with a given address space.
pub fn create_init_process_with_vmar(vmar: Arc<Vmar>) -> Result<Arc<Proc>> {
    let pid = types::PID_INIT;
    types::PID_ALLOCATOR.lock().reserve(pid)?;

    let fd_table = Arc::new(SpinLock::new(Filedesc::with_stdio()));
    let cred = Arc::new(Ucred::root());
    let limit = Arc::new(Plimit::default_limits());
    let sigacts = Arc::new(SpinLock::new(SigActs::new()));

    let proc = Proc::new(pid, None, vmar, fd_table, cred, limit, sigacts, "init");
    if let Ok(root_node) = crate::fs::root() {
        proc.set_cwd(root_node);
    }

    types::allproc_insert(Arc::clone(&proc));
    Ok(proc)
}

/// Creates the main thread for `proc`.
pub fn create_main_thread(
    proc: &Arc<Proc>,
    user_ctx: ostd::arch::cpu::context::UserContext,
) -> Result<Arc<crate::proc::thread::Thread>> {
    lifecycle::clone::create_user_thread(proc, "init", user_ctx, 0)
}
