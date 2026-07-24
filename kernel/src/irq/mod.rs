//! IRQ management subsystem (hardware interrupts, soft IRQs, and tasklets).

pub mod hard;
pub mod soft;
pub mod tasklet;

#[cfg(target_arch = "x86_64")]
pub use hard::map_isa_irq;
pub use hard::{IrqGuard, IrqHandler, IrqRegistration, disable_local};
pub use soft::{SoftIrqVector, do_softirq, open_softirq, raise_softirq, softirq_pending};
pub use tasklet::{Tasklet, schedule_tasklet};

/// Initialise the IRQ subsystem (registers tasklet softirq handler).
pub fn init() {
    soft::open_softirq(SoftIrqVector::Tasklet, tasklet::run_tasklets);
}
