// SPDX-License-Identifier: GPL-2.0

//! Virtual File System (VFS) abstraction layer.

pub mod dcache;
pub mod file;
pub mod fs;
pub mod inode;
pub mod mount;
pub mod ops;
pub mod path;
pub mod perms;

pub use dcache::*;
pub use file::*;
pub use fs::*;
pub use inode::*;
pub use mount::{Mount, PathNode, VFS_ROOT, collect_all_mounts, root, set_root, sync_all};
pub use ops::*;
pub use path::{LookupFlags, lookup, open, open_at};
pub use perms::*;
