use crate::arch::syscall::syscall::SyscallFrame;
use crate::ipc::signal::{SigAction, SigSet};
use core::mem::size_of;

/// Saved signal execution context on the user stack (matches x86_64 POSIX layout).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SigContext {
    pub r8: u64,
    pub r9: u64,
    pub r10: u64,
    pub r11: u64,
    pub rbx: u64,
    pub rbp: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
    pub rdi: u64,
    pub rsi: u64,
    pub rdx: u64,
    pub rcx: u64,
    pub rax: u64,
    pub rip: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub cs: u64,
    pub ss: u64,
    pub oldmask: u64,
}

/// Signal frame constructed on the user stack prior to handler entry.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SigFrame {
    /// Return address pointing to restorer function or sigreturn trampoline
    pub pretcode: u64,
    /// Saved signal context
    pub uc: SigContext,
}

/// Setup user stack frame for invoking a signal handler.
///
/// # Safety
/// Writes `SigFrame` onto the process user stack.
pub unsafe fn setup_signal_frame(
    frame: &mut SyscallFrame,
    sig: u8,
    action: &SigAction,
    old_mask: SigSet,
) -> Result<(), &'static str> {
    // 1. Align stack pointer to 16 bytes and reserve space for SigFrame
    let mut user_rsp = frame.rsp;
    user_rsp = (user_rsp - size_of::<SigFrame>() as u64) & !0xF;

    let restorer = if action.restorer != 0 {
        action.restorer as u64
    } else {
        0 // Restorer fallback address or trampoline
    };

    let sig_context = SigContext {
        r8: frame.r8,
        r9: frame.r9,
        r10: frame.r10,
        r11: frame.r11,
        rbx: 0,
        rbp: frame.rbp,
        r12: 0,
        r13: 0,
        r14: 0,
        r15: 0,
        rdi: frame.rdi,
        rsi: frame.rsi,
        rdx: frame.rdx,
        rcx: frame.rcx,
        rax: frame.rax,
        rip: frame.rip,
        rflags: frame.rflags,
        rsp: frame.rsp,
        cs: frame.cs,
        ss: frame.ss,
        oldmask: old_mask,
    };

    let sig_frame = SigFrame {
        pretcode: restorer,
        uc: sig_context,
    };

    if !crate::syscalls::is_user_ptr_valid(user_rsp, size_of::<SigFrame>()) {
        return Err("Invalid user stack pointer for signal frame");
    }

    // SAFETY: Copy SigFrame directly to user stack space
    let frame_ptr = user_rsp as *mut SigFrame;
    unsafe {
        core::ptr::write_volatile(frame_ptr, sig_frame);
    }

    // 2. Redirect execution context to signal handler
    frame.rsp = user_rsp;
    frame.rip = action.handler as u64;
    frame.rdi = sig as u64;
    frame.rsi = 0; // siginfo pointer (0 if not SA_SIGINFO)
    frame.rdx = (user_rsp + 8) as u64; // address of SigContext

    Ok(())
}

/// Restore user stack frame and CPU registers during `sys_sigreturn`.
///
/// # Safety
/// Reads `SigFrame` from user stack pointer in `SyscallFrame`.
pub unsafe fn restore_signal_frame(frame: &mut SyscallFrame) -> Result<SigSet, &'static str> {
    let user_rsp = frame.rsp;
    if !crate::syscalls::is_user_ptr_valid(user_rsp, size_of::<SigFrame>()) {
        return Err("Invalid user stack pointer for sigreturn");
    }
    let frame_ptr = user_rsp as *const SigFrame;

    // SAFETY: Read SigFrame from current user stack pointer
    let sig_frame = unsafe { core::ptr::read_volatile(frame_ptr) };
    let uc = sig_frame.uc;

    frame.r8 = uc.r8;
    frame.r9 = uc.r9;
    frame.r10 = uc.r10;
    frame.r11 = uc.r11;
    frame.rbp = uc.rbp;
    frame.rdi = uc.rdi;
    frame.rsi = uc.rsi;
    frame.rdx = uc.rdx;
    frame.rcx = uc.rcx;
    frame.rax = uc.rax;
    frame.rip = uc.rip;
    frame.rflags = uc.rflags;
    frame.rsp = uc.rsp;
    frame.cs = uc.cs;
    frame.ss = uc.ss;

    Ok(uc.oldmask)
}
