// SPDX-License-Identifier: GPL-2.0

//! `/proc/sys/kernel` parameters.
//!
//! Synthetic sysctl files. Read-only reports are constant files; `hostname`
//! is the one writable parameter, holding the single piece of kernel state
//! user space may change here (also surfaced by `uname(2)` through
//! [`get_hostname`]).

use alloc::{string::String, sync::Arc, vec::Vec};
use core::str;
use spin::RwLock;

use crate::{
    fs::{
        procfs::template::{ProcDirOps, ProcFileOps, rw_file_node, static_file_node},
        vfs::INode,
    },
    uapi::errno::Result,
};

/// Inode numbers of `/proc/sys/kernel` parameters (inside the 2000.. block,
/// see `super`).
const PROC_SYS_KERNEL_INO_HOSTNAME: usize = 2010;
const PROC_SYS_KERNEL_INO_OSRELEASE: usize = 2011;
const PROC_SYS_KERNEL_INO_OSTYPE: usize = 2012;
const PROC_SYS_KERNEL_INO_VERSION: usize = 2013;
const PROC_SYS_KERNEL_INO_PID_MAX: usize = 2014;

/// Current hostname, `None` until user space sets one explicitly.
static HOSTNAME: RwLock<Option<String>> = RwLock::new(None);

/// Retrieves the current system hostname (used by `uname(2)`).
pub fn get_hostname() -> String {
    HOSTNAME
        .read()
        .clone()
        .unwrap_or_else(|| String::from("petraos"))
}

/// Sets the system hostname.
fn set_hostname(name: String) {
    *HOSTNAME.write() = Some(name);
}

/// Read-write parameter file for `/proc/sys/kernel/hostname`.
struct HostnameFileOps;

impl ProcFileOps for HostnameFileOps {
    fn generate_content(&self) -> Vec<u8> {
        let mut s = get_hostname();
        s.push('\n');
        s.into_bytes()
    }

    fn write_content(&self, data: &[u8]) -> Result<()> {
        // Mirror Linux behavior: accept a trailing newline, ignore noise.
        if let Ok(text) = str::from_utf8(data) {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                set_hostname(String::from(trimmed));
            }
        }
        Ok(())
    }
}

/// Directory node operations for `/proc/sys/kernel`.
pub struct ProcSysKernelDirOps;

impl ProcDirOps for ProcSysKernelDirOps {
    fn lookup_child(&self, name: &str) -> Result<Arc<INode>> {
        match name {
            "hostname" => Ok(rw_file_node(HostnameFileOps, PROC_SYS_KERNEL_INO_HOSTNAME)),
            "osrelease" => Ok(static_file_node(
                b"6.6.0-silica\n",
                PROC_SYS_KERNEL_INO_OSRELEASE,
            )),
            "ostype" => Ok(static_file_node(b"Linux\n", PROC_SYS_KERNEL_INO_OSTYPE)),
            "version" => Ok(static_file_node(
                b"#1 PREEMPT Mon Oct 12 2026\n",
                PROC_SYS_KERNEL_INO_VERSION,
            )),
            "pid_max" => Ok(static_file_node(b"32768\n", PROC_SYS_KERNEL_INO_PID_MAX)),
            _ => crate::return_errno!(ENOENT, "unknown entry in /proc/sys/kernel"),
        }
    }

    fn list_entries(&self) -> Vec<String> {
        ["hostname", "osrelease", "ostype", "version", "pid_max"]
            .iter()
            .map(|s| String::from(*s))
            .collect()
    }
}
