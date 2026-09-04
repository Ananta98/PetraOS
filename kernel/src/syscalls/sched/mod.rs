//! Scheduler system calls subsystem for PetraOS.
//!
//! Provides implementations for standard POSIX / Linux scheduling system calls:
//! - Policy: `sched_getscheduler`, `sched_setscheduler`, `sched_get_priority_min`, `sched_get_priority_max`
//! - Parameters: `sched_getparam`, `sched_setparam`
//! - CPU Affinity: `sched_getaffinity`, `sched_setaffinity`
//! - Attributes: `sched_getattr`, `sched_setattr`

pub mod affinity;
pub mod attr;
pub mod param;
pub mod policy;
pub mod types;

pub use affinity::{sys_sched_getaffinity, sys_sched_setaffinity};
pub use attr::{sys_sched_getattr, sys_sched_setattr};
pub use param::{sys_sched_getparam, sys_sched_setparam};
pub use policy::{
    sys_sched_get_priority_max, sys_sched_get_priority_min, sys_sched_getscheduler,
    sys_sched_setscheduler,
};
pub use types::*;
