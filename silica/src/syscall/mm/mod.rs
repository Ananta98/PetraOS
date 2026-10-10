// SPDX-License-Identifier: GPL-2.0

pub mod brk;
pub mod madvise;
pub mod mmap;
pub mod mprotect;
pub mod msync;
pub mod munmap;

pub use brk::*;
pub use madvise::*;
pub use mmap::*;
pub use mprotect::*;
pub use msync::*;
pub use munmap::*;
