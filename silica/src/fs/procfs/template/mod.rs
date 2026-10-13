// SPDX-License-Identifier: GPL-2.0

//! Reusable ProcFS Inode Templates and Table-Driven Directory Helpers.

pub mod dir;
pub mod file;
pub mod sym;

pub use dir::{ProcDir, ProcDirFileOps, ProcDirNodeOps, ProcDirOps, StaticDirEntry};
pub use file::{
    CallbackProcFileOps, DynamicProcFileNodeOps, DynamicProcFileOps, ProcFile, ProcFileNodeOps,
    ProcFileOps,
};
pub use sym::{ProcSym, ProcSymNodeOps, ProcSymOps};
