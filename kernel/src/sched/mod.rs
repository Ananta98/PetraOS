//! Scheduler subsystem.
//!
//! Exposes the CFS run queue, the Real-Time run queue, the per-CPU policy
//! engine, and the global multi-CPU scheduler.

pub mod cfs;
pub mod policy;
pub mod realtime;
pub mod sched_thread;
pub mod scheduler;

pub use policy::PerCpuScheduler;
pub use sched_thread::{SchedPolicy, SchedThread, ThreadId};
pub use scheduler::{GlobalScheduler, MAX_CPUS};
