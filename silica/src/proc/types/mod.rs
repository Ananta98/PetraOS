// SPDX-License-Identifier: GPL-2.0

//! Foundational process and thread types, identifiers, and global table.

pub mod pid;
pub mod proc;
pub mod table;

pub use self::{
    pid::{PID_INIT, PID_MAX, Pid, PidAllocator, TID_MAX, Tid, alloc_tid},
    proc::{COMM_LEN, ExitStatus, Proc, ProcInner, ProcState, comm_from_name},
    table::{
        PID_ALLOCATOR, PROCTREE_LOCK, ProcessTable, ProcessTree, allproc_count, allproc_find,
        allproc_for_each, allproc_insert, allproc_iter, allproc_remove, pgrp_for_each,
        pgrp_members, tree_find_reaper, tree_reparent_children,
    },
};
