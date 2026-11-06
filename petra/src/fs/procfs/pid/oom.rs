// SPDX-License-Identifier: GPL-2.0

//! `/proc/<pid>/oom_score_adj` and `/proc/<pid>/oom_score`.
//!
//! Provides the userspace control and observation interface for the OOM
//! killer badness heuristic. Values range from -1000 (OOM_SCORE_ADJ_MIN,
//! completely immune to OOM killing) to 1000 (OOM_SCORE_ADJ_MAX, always
//! killed first).

use alloc::{format, vec::Vec};
use core::str;

use crate::{
    fs::{procfs::template::ProcFileOps, vfs::Mode},
    proc::{Pid, allproc_find, thread::Thread},
    uapi::errno::{Errno, Result},
};

/// Minimum adjustment score (OOM disabled for process).
pub const OOM_SCORE_ADJ_MIN: i16 = -1000;
/// Maximum adjustment score (process targeted first).
pub const OOM_SCORE_ADJ_MAX: i16 = 1000;
/// Capability bit 24 (CAP_SYS_RESOURCE).
const CAP_SYS_RESOURCE: u64 = 1 << 24;

/// `/proc/<pid>/oom_score_adj` file operations.
pub struct PidOomScoreAdjFileOps {
    pub pid: Pid,
}

impl ProcFileOps for PidOomScoreAdjFileOps {
    fn generate_content(&self) -> Vec<u8> {
        let Some(proc) = allproc_find(self.pid) else {
            return Vec::new();
        };
        format!("{}\n", proc.oom_score_adj()).into_bytes()
    }

    fn write_content(&self, data: &[u8]) -> Result<()> {
        let text = str::from_utf8(data).map_err(|_| Errno::EINVAL)?;
        let val = text.trim().parse::<i16>().map_err(|_| Errno::EINVAL)?;

        if !(OOM_SCORE_ADJ_MIN..=OOM_SCORE_ADJ_MAX).contains(&val) {
            crate::return_errno!(EINVAL, "oom_score_adj out of range [-1000, 1000]");
        }

        let proc = allproc_find(self.pid).ok_or(Errno::ESRCH)?;

        // In Linux, lowering oom_score_adj requires CAP_SYS_RESOURCE or root.
        if let Some(caller) = Thread::current_proc() {
            let current = proc.oom_score_adj();
            if val < current {
                let is_root = caller.cred().cr_uid.is_root();
                let has_cap = (caller.inner.lock().cap_effective & CAP_SYS_RESOURCE) != 0;
                if !is_root && !has_cap {
                    crate::return_errno!(EACCES, "CAP_SYS_RESOURCE required to lower oom_score_adj");
                }
            }
        }

        proc.set_oom_score_adj(val);
        Ok(())
    }

    fn file_mode(&self, _writable: bool) -> Mode {
        // Linux exposes /proc/[pid]/oom_score_adj as 0666.
        Mode::FILE | Mode::from_bits_truncate(0o666)
    }
}

/// `/proc/<pid>/oom_score` file operations.
pub struct PidOomScoreFileOps {
    pub pid: Pid,
}

impl ProcFileOps for PidOomScoreFileOps {
    fn generate_content(&self) -> Vec<u8> {
        let Some(proc) = allproc_find(self.pid) else {
            return Vec::new();
        };
        // Return badness score clamped to >= 0.
        let score = proc.oom_score_adj().max(0);
        format!("{}\n", score).into_bytes()
    }
}

