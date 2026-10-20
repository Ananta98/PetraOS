// SPDX-License-Identifier: GPL-2.0

//! Reusable ProcFS Inode Templates and Table-Driven Directory Helpers.
//!
//! Everything in procfs is built from the three node kinds defined here:
//!
//! - [`dir`]: directories, with table-driven static entry resolution,
//! - [`file`]: content view files ([`file::ProcFileOps`]),
//! - [`sym`]: dynamically-resolving symbolic links.
//!
//! Subsystem code implements a small content trait per node; the adapters
//! ([`ProcDirNodeOps`], [`ProcFileNodeOps`], [`ProcSymNodeOps`]) provide the
//! uniform VFS plumbing, following the template module of the Asterinas
//! procfs reference.

pub mod dir;
pub mod file;
pub mod sym;

pub use dir::{
    ProcDir, ProcDirFileOps, ProcDirNodeOps, ProcDirOps, StaticDirEntry, dir_attr, dir_node,
    list_static_entries, lookup_static_entry,
};
pub use file::{
    ProcFile, ProcFileNodeOps, ProcFileOps, StaticFileOps, file_node, rw_file_node,
    static_file_node,
};
pub use sym::{ProcSym, ProcSymNodeOps, ProcSymOps, sym_node};
