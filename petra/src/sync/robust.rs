// SPDX-License-Identifier: GPL-2.0

//! Robust futex list tracking and exit handling (Linux `set_robust_list` / `get_robust_list`).

use alloc::sync::Arc;
use core::mem::size_of;

use crate::{
    proc::Proc,
    sync::futex::{FUTEX_OWNER_DIED, FUTEX_TID_MASK, FUTEX_WAITERS, futex_wake, read_futex_val},
    utils::UserPtr,
};

/// Linux `struct robust_list_head` on x86_64 (24 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct RobustListHead {
    /// Pointer to the head of the circular linked list (`struct robust_list *`).
    pub list: usize,
    /// Offset in bytes from the list node to the 32-bit futex word.
    pub futex_offset: isize,
    /// Pointer to the node being registered/unregistered.
    pub list_op_pending: usize,
}

pub const ROBUST_LIST_HEAD_LEN: usize = size_of::<RobustListHead>(); // 24 bytes

/// Walks the robust futex list for an exiting thread, setting `FUTEX_OWNER_DIED`
/// on each held futex and waking waiters.
pub fn exit_robust_list(proc: &Arc<Proc>, tid: u32, head_ptr: usize) {
    if head_ptr == 0 {
        return;
    }
    let vmar = proc.vmspace();

    // 1. Read the robust list head from user memory
    let mut head_bytes = [0u8; ROBUST_LIST_HEAD_LEN];
    if UserPtr::new(head_ptr).read_bytes(&vmar, &mut head_bytes).is_err() {
        return;
    }

    let list_next = usize::from_ne_bytes(head_bytes[0..8].try_into().unwrap());
    let futex_offset = isize::from_ne_bytes(head_bytes[8..16].try_into().unwrap());
    let list_op_pending = usize::from_ne_bytes(head_bytes[16..24].try_into().unwrap());

    // Helper to inspect a futex node and mark it OWNER_DIED if owned by `tid`
    let handle_futex_entry = |node_ptr: usize| {
        if node_ptr == 0 {
            return;
        }
        let futex_addr = (node_ptr as isize).wrapping_add(futex_offset) as usize;
        if futex_addr == 0 || !futex_addr.is_multiple_of(4) {
            return;
        }

        if let Ok(cur_val) = read_futex_val(&vmar, futex_addr) {
            // Check if this futex is owned by the dying thread
            if (cur_val & FUTEX_TID_MASK) == tid {
                let new_val = (cur_val & FUTEX_WAITERS) | FUTEX_OWNER_DIED;
                let _ = UserPtr::new(futex_addr).write_bytes(&vmar, &new_val.to_ne_bytes());
                // Wake up a waiter so they receive EOWNERDEAD
                futex_wake(&vmar, futex_addr, 1);
            }
        }
    };

    // 2. Handle pending operation entry if set
    if list_op_pending != 0 {
        handle_futex_entry(list_op_pending);
    }

    // 3. Walk the circular linked list up to safety limit (1024 nodes)
    let mut curr = list_next;
    let mut count = 0;
    while curr != 0 && curr != head_ptr && count < 1024 {
        handle_futex_entry(curr);

        // Read next pointer from struct robust_list { void *next; }
        let mut next_bytes = [0u8; size_of::<usize>()];
        if UserPtr::new(curr).read_bytes(&vmar, &mut next_bytes).is_err() {
            break;
        }
        let next_ptr = usize::from_ne_bytes(next_bytes);
        if next_ptr == curr {
            break; // Self-loop guard
        }
        curr = next_ptr;
        count += 1;
    }
}
