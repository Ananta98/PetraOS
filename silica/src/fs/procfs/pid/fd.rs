// SPDX-License-Identifier: GPL-2.0

//! `/proc/<pid>/fd/` — open file descriptors.
//!
//! Each numeric child resolves to a symlink describing the open file: the
//! absolute path when known, otherwise a Linux-style synthetic name
//! (`/dev/console`, `pipe:[ino]`, `socket:[ino]`).

use alloc::{format, string::String, sync::Arc, vec::Vec};

use crate::{
    api::errno::{Errno, Result},
    fs::procfs::template::{ProcDirOps, ProcSymOps, sym_node},
    fs::vfs::INode,
    proc::{Pid, allproc_find},
};

/// Tag selecting the `fd` subdirectory of a pid (into [`super::pid_ino`]).
pub const TAG_FD_DIR: u16 = 0x0010;

/// Base tag of descriptor-entry inodes; the low 12 bits carry the fd number
/// itself.
const TAG_FD_BASE: u16 = 0x1000;

/// `/proc/<pid>/fd` directory node operations.
pub struct ProcPidFdDirOps {
    pid: Pid,
}

impl ProcPidFdDirOps {
    /// Creates a directory handler for `pid`.
    pub const fn new(pid: Pid) -> Self {
        Self { pid }
    }
}

impl ProcDirOps for ProcPidFdDirOps {
    fn lookup_child(&self, name: &str) -> Result<Arc<INode>> {
        let fd: i32 = name.parse().map_err(|_| Errno::ENOENT)?;
        let proc = allproc_find(self.pid).ok_or(Errno::ENOENT)?;
        let _ = proc.file(fd)?;

        let fd_tag = TAG_FD_BASE | (fd as u16 & 0x0FFF);
        Ok(sym_node(
            ProcFdSymlinkOps { pid: self.pid, fd },
            super::pid_ino(self.pid, fd_tag),
        ))
    }

    fn list_entries(&self) -> Vec<String> {
        let Some(proc) = allproc_find(self.pid) else {
            return Vec::new();
        };
        let fd_table = proc.fd_table.lock();
        fd_table
            .files
            .iter()
            .enumerate()
            .filter(|(_, slot)| slot.is_some())
            .map(|(idx, _)| alloc::format!("{}", idx))
            .collect()
    }
}

/// Symlink operations describing an open descriptor of `pid`.
pub struct ProcFdSymlinkOps {
    pub pid: Pid,
    pub fd: i32,
}

impl ProcSymOps for ProcFdSymlinkOps {
    fn readlink(&self) -> Result<String> {
        let proc = allproc_find(self.pid).ok_or(Errno::ENOENT)?;
        let file = proc.file(self.fd)?;

        if let Some(s) = file
            .abs_path
            .as_deref()
            .and_then(|p| core::str::from_utf8(p).ok())
        {
            return Ok(String::from(s));
        }

        // Fallbacks for pseudo-files and standard streams
        if file.inode.is_chr() {
            Ok(String::from("/dev/console"))
        } else if file.inode.is_fifo() {
            Ok(format!("pipe:[{}]", file.inode.ino()))
        } else if file.inode.is_socket() {
            Ok(format!("socket:[{}]", file.inode.ino()))
        } else {
            Ok(String::from("/dev/null"))
        }
    }
}
