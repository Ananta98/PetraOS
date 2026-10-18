// SPDX-License-Identifier: GPL-2.0

//! Linux-compatible staged kernel initcalls.

use alloc::vec::Vec;
use core::fmt;
use spin::Mutex;

use crate::api::errno::Result;

/// Initcall execution levels, matching Linux kernel boot stages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum InitcallLevel {
    /// Pure / Early initialization (level 0)
    Early = 0,
    /// Core kernel subsystems (level 1)
    Core = 1,
    /// Post-core initialization (level 2)
    PostCore = 2,
    /// Architecture-specific setup (level 3)
    Arch = 3,
    /// Kernel subsystems: VFS, IPC, network stack (level 4)
    Subsys = 4,
    /// Filesystems: ext2, devfs, initramfs (level 5)
    Fs = 5,
    /// Device drivers: PCI, NVMe, Block devices (level 6)
    Device = 6,
    /// Late initialization before userspace (level 7)
    Late = 7,
}

impl fmt::Display for InitcallLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Early => write!(f, "0 (early)"),
            Self::Core => write!(f, "1 (core)"),
            Self::PostCore => write!(f, "2 (postcore)"),
            Self::Arch => write!(f, "3 (arch)"),
            Self::Subsys => write!(f, "4 (subsys)"),
            Self::Fs => write!(f, "5 (fs)"),
            Self::Device => write!(f, "6 (device)"),
            Self::Late => write!(f, "7 (late)"),
        }
    }
}

pub type InitcallFn = fn() -> Result<()>;

/// An entry in the kernel's initcall dispatch table.
#[derive(Clone, Copy)]
pub struct InitcallEntry {
    pub level: InitcallLevel,
    pub name: &'static str,
    pub func: InitcallFn,
}

/// Global registry of kernel initcalls.
static INITCALL_LIST: Mutex<Vec<InitcallEntry>> = Mutex::new(Vec::new());

/// Register a function into the kernel initcall queue.
pub fn register_initcall(level: InitcallLevel, name: &'static str, func: InitcallFn) {
    let mut list = INITCALL_LIST.lock();
    list.push(InitcallEntry { level, name, func });
}

/// Execute all registered initcalls in priority order (levels 0 through 7).
///
/// This is one-shot: the pending list is drained, so initcalls registered
/// after boot (e.g. by loadable modules) do not run until the next boot.
pub fn do_initcalls() -> Result<()> {
    let mut entries = {
        let mut list = INITCALL_LIST.lock();
        core::mem::take(&mut *list)
    };

    // Stable sort by initcall level so earlier levels execute first while
    // preserving registration order within a level.
    entries.sort_by_key(|e| e.level);

    ostd::info!(
        "Executing kernel initcalls ({} registered)...",
        entries.len()
    );

    let mut current_level = None;
    for entry in &entries {
        if current_level != Some(entry.level) {
            current_level = Some(entry.level);
            ostd::info!("initcall: entering level {}", entry.level);
        }

        match (entry.func)() {
            Ok(()) => {
                ostd::info!("initcall: {}() ok", entry.name);
            }
            Err(err) => {
                ostd::error!("initcall: {}() failed with error {:?}", entry.name, err);
                return Err(err);
            }
        }
    }

    ostd::info!("All kernel initcalls completed successfully");
    Ok(())
}
