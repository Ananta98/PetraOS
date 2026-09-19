//! Architecture-Specific Paging Subsystem for x86_64.
//!
//! Provides runtime 4-level and 5-level (LA57) paging detection, TLB invalidation,
//! and the `ArchPageTable` hardware mapper.

pub mod flush;
pub mod helpers;
pub mod table;

pub use helpers::{active_cr3, ensure_mapped, map_mmio, read_cr2};
pub use table::ArchPageTable;
