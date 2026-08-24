//! User-Space Memory Access Subsystem for PetraOS.
//!
//! Provides OOP abstractions (`UserPtr<T>` and `UserCStr`) for type-safe,
//! bound-checked memory operations between kernel space and Ring 3 user space.

pub mod user_cstr;
pub mod user_ptr;

pub use user_cstr::UserCStr;
pub use user_ptr::UserPtr;

/// Maximum virtual address allowed for user space pointers (Ring 3 canonical boundary).
pub const USER_SPACE_MAX_ADDR: u64 = 0x0000_7FFF_FFFF_FFFF;
