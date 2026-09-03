use core::arch::asm;

/// User Code Segment Selector with RPL = 3
pub const USER_CS: u64 = 0x1B;
/// User Data Segment Selector with RPL = 3
pub const USER_DS: u64 = 0x23;

/// Default virtual memory base address for user process executable code
pub const USER_CODE_VBASE: u64 = 0x0000_0000_0040_0000;
/// Default top address for user process stack
pub const USER_STACK_VTOP: u64 = 0x0000_0000_7000_0000;
/// Default virtual memory base address for user process heap (brk)
pub const USER_HEAP_VBASE: u64 = 0x0000_0000_1000_0000;
/// Default virtual memory base address for user mmap region
pub const USER_MMAP_VBASE: u64 = 0x0000_7000_0000_0000;
/// Default user stack size (16 KiB)

/// Transition CPU privilege level from Ring 0 (Kernel) to Ring 3 (User Mode).
///
/// Loads the user page table root (CR3), sets user data segment registers, pushes the `iretq`
/// stack frame (SS, RSP, RFLAGS, CS, RIP), and executes `iretq`.
///
/// # Safety
/// The caller must ensure that `entry_point` and `stack_pointer` point to valid user virtual memory
/// pages mapped in the given `cr3` address space with `MapFlags::USER`, and `kernel_rsp0` points to
/// a valid, 16-byte aligned kernel stack top for TSS RSP0 context handling.
pub unsafe fn jump_to_userspace(
    entry_point: u64,
    stack_pointer: u64,
    kernel_rsp0: u64,
    cr3: u64,
) -> ! {
    // Configure TSS RSP0 for user mode interrupts and syscalls
    super::tss::set_rsp0(kernel_rsp0);

    // SAFETY: Switch stack to valid kernel_rsp0, load user CR3, push iretq frame, set segment registers, and execute iretq.
    unsafe {
        asm!(
            "mov rsp, {kstack_top}",
            "mov cr3, {cr3}",
            "push {user_ds}",
            "push {rsp}",
            "push {rflags}",
            "push {user_cs}",
            "push {rip}",
            "mov ds, {user_ds:e}",
            "mov es, {user_ds:e}",
            "mov fs, {user_ds:e}",
            "mov gs, {user_ds:e}",
            "iretq",
            kstack_top = in(reg) kernel_rsp0,
            cr3 = in(reg) cr3,
            user_ds = in(reg) USER_DS,
            rsp = in(reg) stack_pointer,
            rflags = in(reg) 0x202u64,
            user_cs = in(reg) USER_CS,
            rip = in(reg) entry_point,
            options(noreturn)
        );
    }
}
