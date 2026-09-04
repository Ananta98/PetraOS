//! Architecture-specific scheduler and context switching routines for x86_64.
//!
//! Submodules provide:
//! - [`context`]: Thread execution state (`ThreadContext`) and stack initialization.
//! - [`switch`]: Low-level assembly context switching (`switch_context`, `switch_context_to`, `arch_switch_context`).

pub mod context;
pub mod switch;

pub use context::ThreadContext;
pub use switch::arch_switch_context;
