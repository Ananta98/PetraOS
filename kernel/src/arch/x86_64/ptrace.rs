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

    /// Size of `UserRegsStruct` representation in bytes.
    pub const SIZE: usize = 216;

    /// Converts `UserRegsStruct` into a 216-byte array in safe Rust.
    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut bytes = [0u8; Self::SIZE];
        let fields = [
            self.r15,
            self.r14,
            self.r13,
            self.r12,
            self.rbp,
            self.rbx,
            self.r11,
            self.r10,
            self.r9,
            self.r8,
            self.rax,
            self.rcx,
            self.rdx,
            self.rsi,
            self.rdi,
            self.orig_rax,
            self.rip,
            self.cs,
            self.eflags,
            self.rsp,
            self.ss,
            self.fs_base,
            self.gs_base,
            self.ds,
            self.es,
            self.fs,
            self.gs,
        ];
        for (i, &val) in fields.iter().enumerate() {
            bytes[i * 8..(i + 1) * 8].copy_from_slice(&val.to_ne_bytes());
        }
        bytes
    }

    /// Constructs `UserRegsStruct` from a 216-byte array in safe Rust.
    pub fn from_bytes(bytes: &[u8; Self::SIZE]) -> Self {
        let mut vals = [0u64; 27];
        for i in 0..27 {
            let chunk: [u8; 8] = bytes[i * 8..(i + 1) * 8].try_into().unwrap();
            vals[i] = u64::from_ne_bytes(chunk);
        }
        Self {
            r15: vals[0],
            r14: vals[1],
            r13: vals[2],
            r12: vals[3],
            rbp: vals[4],
            rbx: vals[5],
            r11: vals[6],
            r10: vals[7],
            r9: vals[8],
            r8: vals[9],
            rax: vals[10],
            rcx: vals[11],
            rdx: vals[12],
            rsi: vals[13],
            rdi: vals[14],
            orig_rax: vals[15],
            rip: vals[16],
            cs: vals[17],
            eflags: vals[18],
            rsp: vals[19],
            ss: vals[20],
            fs_base: vals[21],
            gs_base: vals[22],
            ds: vals[23],
            es: vals[24],
            fs: vals[25],
            gs: vals[26],
        }
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

/// Writes a single user register by offset index to `UserContext` in Safe Rust.
pub fn poke_user_reg(ctx: &mut UserContext, reg_idx: usize, val: u64) -> bool {
    let val_usize = val as usize;
    match reg_idx {
        0 => ctx.set_r15(val_usize),
        1 => ctx.set_r14(val_usize),
        2 => ctx.set_r13(val_usize),
        3 => ctx.set_r12(val_usize),
        4 => ctx.set_rbp(val_usize),
        5 => ctx.set_rbx(val_usize),
        6 => ctx.set_r11(val_usize),
        7 => ctx.set_r10(val_usize),
        8 => ctx.set_r9(val_usize),
        9 => ctx.set_r8(val_usize),
        10 => ctx.set_rax(val_usize),
        11 => ctx.set_rcx(val_usize),
        12 => ctx.set_rdx(val_usize),
        13 => ctx.set_rsi(val_usize),
        14 => ctx.set_rdi(val_usize),
        15 => ctx.set_rax(val_usize),
        16 => ctx.set_rip(val_usize),
        18 => ctx.set_rflags(val_usize),
        19 => ctx.set_rsp(val_usize),
        _ => return false,
    }
    true
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

        let bytes = regs.to_bytes();
        let regs_from_bytes = UserRegsStruct::from_bytes(&bytes);
        assert_eq!(regs, regs_from_bytes);
    }

    #[ktest]
    fn test_peek_and_poke_user_reg() {
        let mut ctx = UserContext::default();
        ctx.set_rax(0x42);
        assert_eq!(peek_user_reg(&ctx, 10), Some(0x42));
        assert_eq!(peek_user_reg(&ctx, 999), None);

        assert!(poke_user_reg(&mut ctx, 10, 0x99));
        assert_eq!(ctx.rax(), 0x99);
        assert!(!poke_user_reg(&mut ctx, 999, 0x99));
    }
}
