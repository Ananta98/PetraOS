// SPDX-License-Identifier: GPL-2.0

//! Core Linux KPI kernel primitives: synchronization, tasks, timers, memory, and refcounts.

pub mod atomic;
pub mod mm;
pub mod refcount;
pub mod sync;
pub mod task;
pub mod time;

pub use atomic::{
    atomic64_t, atomic_t, change_bit, clear_bit, set_bit, test_and_change_bit, test_and_clear_bit,
    test_and_set_bit, test_bit, AtomicBool, AtomicI32, AtomicI64, AtomicUsize, Ordering,
};
pub use mm::{kcalloc, kmalloc, kzalloc, vfree, vmalloc, Gfp};
pub use refcount::{KRef, KWeak};
pub use sync::{KCompletion, KMutex, RootSpinLock, SpinLock};
pub use task::{schedule_delayed_work_fn, schedule_work, schedule_work_fn};
pub use time::{arm, jiffies, ktime_get_ns, mdelay, msleep, udelay, usleep_range, KTimer};
