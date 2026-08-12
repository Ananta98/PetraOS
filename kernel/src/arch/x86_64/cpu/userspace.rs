use core::arch::asm;

/// User Code Segment Selector with RPL = 3
pub const USER_CS: u64 = 0x1B;
/// User Data Segment Selector with RPL = 3
pub const USER_DS: u64 = 0x23;

/// Default virtual memory base address for user process executable code
pub const USER_CODE_VBASE: u64 = 0x0000_0000_0040_0000;
/// Default top address for user process stack
pub const USER_STACK_VTOP: u64 = 0x0000_0000_7000_0000;
/// Default user stack size (16 KiB)
/// Default machine code payload for user mode testing.
///
/// Executes sys_write (1) via int 0x80, sys_yield (24) via int 0x80,
/// and sys_exit (60) via int 0x80.
pub static DEFAULT_USER_PAYLOAD: &[u8] = &[
    // 1. mov rax, 1 (SYS_WRITE)
    0x48, 0xc7, 0xc0, 0x01, 0x00, 0x00, 0x00,
    // 2. mov rdi, 1 (stdout)
    0x48, 0xc7, 0xc7, 0x01, 0x00, 0x00, 0x00,
    // 3. lea rsi, [rip + 0x24] (address of string payload below)
    0x48, 0x8d, 0x35, 0x24, 0x00, 0x00, 0x00,
    // 4. mov rdx, 34 (string length)
    0x48, 0xc7, 0xc2, 0x22, 0x00, 0x00, 0x00,
    // 5. int 0x80 (trigger syscall dispatch)
    0xcd, 0x80,
    // 6. mov rax, 24 (SYS_YIELD)
    0x48, 0xc7, 0xc0, 0x18, 0x00, 0x00, 0x00,
    // 7. int 0x80
    0xcd, 0x80,
    // 8. mov rax, 60 (SYS_EXIT)
    0x48, 0xc7, 0xc0, 0x3c, 0x00, 0x00, 0x00,
    // 9. mov rdi, 0 (status 0)
    0x48, 0xc7, 0xc7, 0x00, 0x00, 0x00, 0x00,
    // 10. int 0x80
    0xcd, 0x80,
    // 11. jmp . (infinite loop safety fallback)
    0xeb, 0xfe,
    // String payload (34 bytes): "Hello from Userspace (init_proc)!\n"
    b'H', b'e', b'l', b'l', b'o', b' ', b'f', b'r', b'o', b'm', b' ',
    b'U', b's', b'e', b'r', b's', b'p', b'a', b'c', b'e', b' ',
    b'(', b'i', b'n', b'i', b't', b'_', b'p', b'r', b'o', b'c', b')', b'!', b'\n',
];

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
    log::info!(
        "[Userspace Jump] Transitioning CPU to Ring 3: RIP={:#x}, RSP={:#x}, Kernel RSP0={:#x}, CR3={:#x}",
        entry_point,
        stack_pointer,
        kernel_rsp0,
        cr3
    );

    // Configure TSS RSP0 for user mode interrupts and syscalls
    super::tss::set_rsp0(kernel_rsp0);

    // SAFETY: Switch stack to valid kernel_rsp0, load user CR3, set segment registers, push iretq frame, and execute iretq.
    unsafe {
        asm!(
            "mov rsp, {kstack_top}",
            "mov cr3, {cr3}",
            "mov ds, {user_ds:e}",
            "mov es, {user_ds:e}",
            "mov fs, {user_ds:e}",
            "mov gs, {user_ds:e}",
            "push {user_ds}",
            "push {rsp}",
            "push {rflags}",
            "push {user_cs}",
            "push {rip}",
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


