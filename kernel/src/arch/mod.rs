pub trait CpuArch {
    /// Disable interrupts on the current core and return the previous state.
    fn disable_interrupts() -> bool;

    /// Enable interrupts on the current core.
    fn enable_interrupts();

    /// Halt the CPU until the next interrupt.
    fn halt();

    /// Initialize architecture-specific tables (GDT, IDT/IVT, Page Tables).
    fn init_hardware();

    /// Get the current logical CPU core ID.
    fn cpu_id() -> u32;

    /// Initialize a thread's stack with an initial frame and return the new stack pointer.
    fn init_stack(stack: &mut [u8], entry: extern "C" fn(*mut u8), arg: *mut u8) -> u64;

    /// Save the current CPU context and switch to another stack pointer.
    ///
    /// # Safety
    /// Must be called with interrupts disabled or in a safe state.
    unsafe fn switch_context(prev_rsp_ptr: *mut u64, next_rsp: u64);

    /// Switch to a thread's stack pointer without saving the current context.
    ///
    /// # Safety
    /// Must be called in a safe state.
    unsafe fn switch_context_to(next_rsp: u64) -> !;
}

#[cfg(target_arch = "x86_64")]
pub mod x86_64;

#[cfg(target_arch = "x86_64")]
pub use x86_64::X86_64 as ArchImpl;

#[cfg(target_arch = "x86_64")]
pub use x86_64::syscall::SyscallFrame;
