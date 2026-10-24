pub mod cpuinfo;
pub mod power;
pub mod signal;

pub fn init() {
    power::init();
}

/// Returns whether the current context is safe to perform blocking or IPI waits.
///
/// Returns true only if execution is in task context (L0).
#[inline]
pub fn can_wait_ipi() -> bool {
    matches!(
        ostd::irq::InterruptLevel::current(),
        ostd::irq::InterruptLevel::L0
    )
}
