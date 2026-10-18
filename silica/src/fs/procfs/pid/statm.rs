// SPDX-License-Identifier: GPL-2.0

//! `/proc/<pid>/statm` generator.

use alloc::{format, sync::Arc, vec::Vec};

use crate::proc::Proc;

pub fn generate_proc_statm(proc: &Arc<Proc>) -> Vec<u8> {
    let total_pages = proc.vmspace().total_mapped_size() / 4096;
    let resident_pages = total_pages;
    let shared_pages = 0;
    let text_pages = 1;
    let data_pages = total_pages.saturating_sub(text_pages);
    format!(
        "{} {} {} {} 0 {} 0\n",
        total_pages, resident_pages, shared_pages, text_pages, data_pages
    )
    .into_bytes()
}
