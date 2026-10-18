// SPDX-License-Identifier: GPL-2.0

//! `/proc/<pid>/comm` generator.

use alloc::{format, sync::Arc, vec::Vec};

use crate::proc::Proc;

pub fn generate_proc_comm(proc: &Arc<Proc>) -> Vec<u8> {
    format!("{}\n", proc.comm_name()).into_bytes()
}
