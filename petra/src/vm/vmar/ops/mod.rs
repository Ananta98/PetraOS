// SPDX-License-Identifier: GPL-2.0

//! System call operations implemented on [`Vmar`](super::Vmar).

pub mod fork;
pub mod mincore;
pub mod mlock;
pub mod mmap;
pub mod mprotect;
pub mod mremap;
pub mod msync;
pub mod munmap;
