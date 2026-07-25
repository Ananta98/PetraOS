//! Process register inspection, manipulation, and ptrace debugging structures.

use ostd::arch::cpu::context::UserContext;

/// Linux x86_64 `user_regs_struct` matching System V AMD64 ABI layout.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UserRegsStruct {
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub rbp: u64,
    pub rbx: u64,
    pub r11: u64,
    pub r10: u64,
    pub r9: u64,
    pub r8: u64,
    pub rax: u64,
    pub rcx: u64,
    pub rdx: u64,
    pub rsi: u64,
    pub rdi: u64,
    pub orig_rax: u64,
    pub rip: u64,
    pub cs: u64,
    pub eflags: u64,
    pub rsp: u64,
    pub ss: u64,
    pub fs_base: u64,
    pub gs_base: u64,
    pub ds: u64,
    pub es: u64,
    pub fs: u64,
    pub gs: u64,
}

impl UserRegsStruct {
    /// Extracts register state from a `UserContext`.
    pub fn from_user_context(ctx: &UserContext) -> Self {
        Self {
            r15: ctx.r15() as u64,
            r14: ctx.r14() as u64,
            r13: ctx.r13() as u64,
            r12: ctx.r12() as u64,
            rbp: ctx.rbp() as u64,
            rbx: ctx.rbx() as u64,
            r11: ctx.r11() as u64,
            r10: ctx.r10() as u64,
            r9: ctx.r9() as u64,
            r8: ctx.r8() as u64,
            rax: ctx.rax() as u64,
            rcx: ctx.rcx() as u64,
            rdx: ctx.rdx() as u64,
            rsi: ctx.rsi() as u64,
            rdi: ctx.rdi() as u64,
            orig_rax: ctx.rax() as u64,
            rip: ctx.rip() as u64,
            cs: 0x33,
            eflags: ctx.rflags() as u64,
            rsp: ctx.rsp() as u64,
            ss: 0x2b,
            fs_base: 0,
            gs_base: 0,
            ds: 0x2b,
            es: 0x2b,
            fs: 0,
            gs: 0,
        }
    }

    /// Applies register modifications back to a `UserContext`.
    pub fn apply_to_user_context(&self, ctx: &mut UserContext) {
        ctx.set_r15(self.r15 as usize);
        ctx.set_r14(self.r14 as usize);
        ctx.set_r13(self.r13 as usize);
        ctx.set_r12(self.r12 as usize);
        ctx.set_rbp(self.rbp as usize);
        ctx.set_rbx(self.rbx as usize);
        ctx.set_r11(self.r11 as usize);
        ctx.set_r10(self.r10 as usize);
        ctx.set_r9(self.r9 as usize);
        ctx.set_r8(self.r8 as usize);
        ctx.set_rax(self.rax as usize);
        ctx.set_rcx(self.rcx as usize);
        ctx.set_rdx(self.rdx as usize);
        ctx.set_rsi(self.rsi as usize);
        ctx.set_rdi(self.rdi as usize);
        ctx.set_rip(self.rip as usize);
        ctx.set_rflags(self.eflags as usize);
        ctx.set_rsp(self.rsp as usize);
    }
}

/// Reads a single user register by offset index from `UserContext` in Safe Rust.
pub fn peek_user_reg(ctx: &UserContext, reg_idx: usize) -> Option<u64> {
    match reg_idx {
        0 => Some(ctx.r15() as u64),
        1 => Some(ctx.r14() as u64),
        2 => Some(ctx.r13() as u64),
        3 => Some(ctx.r12() as u64),
        4 => Some(ctx.rbp() as u64),
        5 => Some(ctx.rbx() as u64),
        6 => Some(ctx.r11() as u64),
        7 => Some(ctx.r10() as u64),
        8 => Some(ctx.r9() as u64),
        9 => Some(ctx.r8() as u64),
        10 => Some(ctx.rax() as u64),
        11 => Some(ctx.rcx() as u64),
        12 => Some(ctx.rdx() as u64),
        13 => Some(ctx.rsi() as u64),
        14 => Some(ctx.rdi() as u64),
        15 => Some(ctx.rax() as u64),
        16 => Some(ctx.rip() as u64),
        17 => Some(0x33),
        18 => Some(ctx.rflags() as u64),
        19 => Some(ctx.rsp() as u64),
        20 => Some(0x2b),
        _ => None,
    }
}

#[cfg(ktest)]
mod tests {
    use super::*;
    use ostd::prelude::ktest;

    #[ktest]
    fn test_user_regs_conversion() {
        let mut ctx = UserContext::default();
        ctx.set_rax(0x1234_5678);
        ctx.set_rip(0x4000_0000);
        ctx.set_rsp(0x7FFF_0000);

        let regs = UserRegsStruct::from_user_context(&ctx);
        assert_eq!(regs.rax, 0x1234_5678);
        assert_eq!(regs.rip, 0x4000_0000);
        assert_eq!(regs.rsp, 0x7FFF_0000);

        let mut ctx2 = UserContext::default();
        regs.apply_to_user_context(&mut ctx2);
        assert_eq!(ctx2.rax(), 0x1234_5678);
        assert_eq!(ctx2.rip(), 0x4000_0000);
        assert_eq!(ctx2.rsp(), 0x7FFF_0000);
    }

    #[ktest]
    fn test_peek_user_reg() {
        let mut ctx = UserContext::default();
        ctx.set_rax(0x42);
        assert_eq!(peek_user_reg(&ctx, 10), Some(0x42));
        assert_eq!(peek_user_reg(&ctx, 999), None);
    }
}
