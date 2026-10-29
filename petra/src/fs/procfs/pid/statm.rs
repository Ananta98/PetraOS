// SPDX-License-Identifier: GPL-2.0

//! `/proc/<pid>/statm` — memory usage and page statistics.
//!
//! Reproduces the Linux seven-field layout: `total resident shared text lib
//! data dirty` pages, with a page acknowledged as 4096 bytes.

use alloc::{format, vec::Vec};

use crate::{
    fs::procfs::template::ProcFileOps,
    proc::{Pid, allproc_find},
};

/// Page size assumed by the `/proc/<pid>/statm` ABI (matches Linux x86_64).
const STATM_PAGE_SIZE: usize = 4096;

/// Content operations of the per-process statm file.
pub struct PidStatmFileOps {
    pub pid: Pid,
}

impl ProcFileOps for PidStatmFileOps {
    fn generate_content(&self) -> Vec<u8> {
        let Some(proc) = allproc_find(self.pid) else {
            return Vec::new();
        };

        let total_pages = proc.vmspace().total_mapped_size() / STATM_PAGE_SIZE;
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
}
