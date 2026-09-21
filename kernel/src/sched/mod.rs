//! Scheduler Subsystem for PetraOS.
//!
//! Re-exports modules, runqueues, policies, and the scheduler interface.

pub mod fair;
pub mod nice;
pub mod policy;
pub mod realtime;
pub mod runqueue;
pub mod scheduler;

pub use fair::{BASE_SLICE_NS, EevdfEntity, EevdfScheduler};
pub use nice::{MAX_NICE, MIN_NICE, NICE_0_WEIGHT, Nice, nice_to_weight};
pub use policy::{
    DEFAULT_RR_QUANTUM_NS, MAX_RT_PRIO, MIN_RT_PRIO, RT_PRIO_COUNT, RtPriority, SchedPolicy,
};
pub use realtime::RtRunQueue;
pub use runqueue::RunQueue;
pub use scheduler::{
    SCHEDULER, Scheduler, add_thread, current_thread, current_thread_on_cpu, init, pick_next,
    remove_thread, schedule, set_current_thread, set_current_thread_on_cpu, tick,
    yield_current,
};
