//! Signal frame, sigcontext, and ucontext handling for user space signal delivery.

use crate::arch::ptrace::UserRegsStruct;
use crate::vm::vma::VmaManager;
use alloc::vec::Vec;
use ostd::Error;
use ostd::arch::cpu::context::UserContext;

/// Standard Linux x86_64 `sigcontext` structure.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SigContext {
    pub r8: u64,
    pub r9: u64,
    pub r10: u64,
    pub r11: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
    pub rdi: u64,
    pub rsi: u64,
    pub rbp: u64,
    pub rbx: u64,
    pub rdx: u64,
    pub rax: u64,
    pub rcx: u64,
    pub rsp: u64,
    pub rip: u64,
    pub eflags: u64,
    pub cs: u16,
    pub gs: u16,
    pub fs: u16,
    pub ss: u16,
    pub err: u64,
    pub trapno: u64,
    pub oldmask: u64,
    pub cr2: u64,
}

/// Standard Linux x86_64 `ucontext_t` structure.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UContext {
    pub flags: u64,
    pub link: u64,
    pub stack_sp: u64,
    pub stack_flags: i32,
    pub stack_size: usize,
    pub mcontext: SigContext,
    pub sigmask: u64,
}

/// User stack frame layout pushed for signal handler execution.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SignalFrame {
    /// Address of `sys_rt_sigreturn` trampoline or restorer function.
    pub pretend_return_addr: u64,
    /// Preserved user context and signal mask.
    pub uc: UContext,
}

impl SigContext {
    /// Creates a `SigContext` from current `UserContext` and saved signal mask.
    pub fn from_user_context(ctx: &UserContext, mask: u64) -> Self {
        let regs = UserRegsStruct::from_user_context(ctx);
        Self {
            r8: regs.r8,
            r9: regs.r9,
            r10: regs.r10,
            r11: regs.r11,
            r12: regs.r12,
            r13: regs.r13,
            r14: regs.r14,
            r15: regs.r15,
            rdi: regs.rdi,
            rsi: regs.rsi,
            rbp: regs.rbp,
            rbx: regs.rbx,
            rdx: regs.rdx,
            rax: regs.rax,
            rcx: regs.rcx,
            rsp: regs.rsp,
            rip: regs.rip,
            eflags: regs.eflags,
            cs: regs.cs as u16,
            gs: regs.gs as u16,
            fs: regs.fs as u16,
            ss: regs.ss as u16,
            err: 0,
            trapno: 0,
            oldmask: mask,
            cr2: 0,
        }
    }

    /// Restores `UserContext` from this `SigContext`.
    pub fn apply_to_user_context(&self, ctx: &mut UserContext) {
        ctx.set_r8(self.r8 as usize);
        ctx.set_r9(self.r9 as usize);
        ctx.set_r10(self.r10 as usize);
        ctx.set_r11(self.r11 as usize);
        ctx.set_r12(self.r12 as usize);
        ctx.set_r13(self.r13 as usize);
        ctx.set_r14(self.r14 as usize);
        ctx.set_r15(self.r15 as usize);
        ctx.set_rdi(self.rdi as usize);
        ctx.set_rsi(self.rsi as usize);
        ctx.set_rbp(self.rbp as usize);
        ctx.set_rbx(self.rbx as usize);
        ctx.set_rdx(self.rdx as usize);
        ctx.set_rax(self.rax as usize);
        ctx.set_rcx(self.rcx as usize);
        ctx.set_rsp(self.rsp as usize);
        ctx.set_rip(self.rip as usize);
        ctx.set_rflags(self.eflags as usize);
    }
}

/// Prepares and copies a `SignalFrame` onto the user stack, updating `context`
/// to begin execution of `handler`.
pub fn setup_signal_frame(
    vm: &VmaManager,
    context: &mut UserContext,
    sig: i32,
    handler: usize,
    restorer: Option<usize>,
    mask: u64,
) -> Result<usize, Error> {
    let old_sp = context.rsp();
    let frame_size = core::mem::size_of::<SignalFrame>();
    let new_sp = (old_sp - frame_size) & !0xF; // Align stack to 16 bytes

    let sigctx = SigContext::from_user_context(context, mask);

    let mut frame_bytes = Vec::with_capacity(frame_size);
    let mut push_u64 = |val: u64| {
        frame_bytes.extend_from_slice(&val.to_le_bytes());
    };

    // 1. pretend_return_addr
    push_u64(restorer.unwrap_or(0) as u64);
    // 2. uc.flags, uc.link, uc.stack_sp, uc.stack_flags/stack_size
    push_u64(0);
    push_u64(0);
    push_u64(old_sp as u64);
    push_u64(0); // stack_flags and stack_size combined in u64 slots

    // 3. uc.mcontext (SigContext)
    push_u64(sigctx.r8);
    push_u64(sigctx.r9);
    push_u64(sigctx.r10);
    push_u64(sigctx.r11);
    push_u64(sigctx.r12);
    push_u64(sigctx.r13);
    push_u64(sigctx.r14);
    push_u64(sigctx.r15);
    push_u64(sigctx.rdi);
    push_u64(sigctx.rsi);
    push_u64(sigctx.rbp);
    push_u64(sigctx.rbx);
    push_u64(sigctx.rdx);
    push_u64(sigctx.rax);
    push_u64(sigctx.rcx);
    push_u64(sigctx.rsp);
    push_u64(sigctx.rip);
    push_u64(sigctx.eflags);
    push_u64(
        sigctx.cs as u64
            | ((sigctx.gs as u64) << 16)
            | ((sigctx.fs as u64) << 32)
            | ((sigctx.ss as u64) << 48),
    );
    push_u64(sigctx.err);
    push_u64(sigctx.trapno);
    push_u64(sigctx.oldmask);
    push_u64(sigctx.cr2);

    // 4. uc.sigmask
    push_u64(mask);

    vm.copy_to_user(new_sp, &frame_bytes)?;

    // Set up register arguments for signal handler function
    context.set_rdi(sig as usize);
    context.set_rip(handler);
    context.set_rsp(new_sp);

    Ok(new_sp)
}

#[cfg(ktest)]
mod tests {
    use super::*;
    use ostd::prelude::ktest;

    #[ktest]
    fn test_sigcontext_conversion() {
        let mut ctx = UserContext::default();
        ctx.set_rdi(9);
        ctx.set_rip(0x401000);

        let sc = SigContext::from_user_context(&ctx, 0x123);
        assert_eq!(sc.rdi, 9);
        assert_eq!(sc.rip, 0x401000);
        assert_eq!(sc.oldmask, 0x123);

        let mut restored_ctx = UserContext::default();
        sc.apply_to_user_context(&mut restored_ctx);
        assert_eq!(restored_ctx.rdi(), 9);
        assert_eq!(restored_ctx.rip(), 0x401000);
    }
}
