pub trait CpuArch {
    /// Disable interrupts on the current core and return the previous state.
    fn disable_interrupts() -> bool;

    /// Enable interrupts on the current core.
    fn enable_interrupts();

    /// Halt the CPU until the next interrupt.
    fn halt();

    /// Initialize architecture-specific tables (GDT, IDT/IVT, Page Tables).
    fn init_hardware();
}

#[cfg(target_arch = "x86_64")]
pub mod x86_64;

#[cfg(target_arch = "x86_64")]
pub use x86_64::X86_64 as ArchImpl;
