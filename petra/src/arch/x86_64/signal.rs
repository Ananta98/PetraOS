// SPDX-License-Identifier: GPL-2.0

//! x86-64 signal frame (`rt_sigframe`), frame construction, and `rt_sigreturn(2)` restoration.
//!
//! Layout (`560` bytes, 16-byte aligned): `pretcode` (8 B) -> `ucontext` (424 B) -> `siginfo_t` (128 B).
//!
//! `OSTD`'s `UserContext` only carries general-purpose registers and `rflags`, so segment
//! selectors, `REG_ORIG_RAX` (preventing `SA_RESTART`), and FPU state are not captured or
//! restored; `IA32_FS_BASE` is tracked separately on [`crate::proc::thread::Thread::fs_base`].

use alloc::sync::Arc;
use core::fmt;

use ostd::{arch::cpu::context::UserContext, mm::PAGE_SIZE, user::UserContextApi};

use crate::{
    uapi::{
        errno::{Errno, Result},
        signal::{SIGSET_WORDS, SigInfo, SigSet, SigStack, Signal, code},
    },
    utils::userspace::UserPtr,
    vm::{Vmar, flags::MmapFlags, perms::VmPerms},
};

/// Syscall number for `rt_sigreturn(2)`.
pub const NR_RT_SIGRETURN: usize = 15;

/// Fallback `rt_sigreturn(2)` trampoline used when `rt_sigaction(2)` omits `SA_RESTORER`.
///
/// The kernel's vDSO (`crate::vdso`) only exports timekeeping symbols and does not provide
/// a signal restorer, so this 9-byte stub is mapped into each process at [`TRAMPOLINE_ADDR`].
pub const TRAMPOLINE: [u8; 9] = [
    0xb8, 0x0f, 0x00, 0x00, 0x00, // mov eax, 15 (SYS_rt_sigreturn)
    0x0f, 0x05,                   // syscall
    0x0f, 0x0b,                   // ud2 (trap if rt_sigreturn returns)
];

/// Fixed user-space address for [`TRAMPOLINE`], placed below the user stack region.
pub const TRAMPOLINE_ADDR: usize = 0x0000_7fff_0000_0000;

// Ring-3 selectors matching `ostd/src/arch/x86/trap/gdt.rs`.
const USER_CS: usize = 0x2b;
const USER_SS: usize = 0x23;
const CSGSFS: usize = (USER_CS << 16) | USER_SS;

/// Register indices in [`MContext::gregs`] (`pt_regs` order starting at `R8`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum GReg {
    R8 = 0,
    R9 = 1,
    R10 = 2,
    R11 = 3,
    R12 = 4,
    R13 = 5,
    R14 = 6,
    R15 = 7,
    Rdi = 8,
    Rsi = 9,
    Rbp = 10,
    Rbx = 11,
    Rdx = 12,
    Rax = 13,
    Rcx = 14,
    Rsp = 15,
    Rip = 16,
    Efl = 17,
    CsGsFs = 18,
    Err = 19,
    TrapNo = 20,
    OldMask = 21,
    Cr2 = 22,
}

pub const NGREG: usize = 23;

/// Interrupted machine context (`mcontext_t`, 256 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct MContext {
    pub gregs: [u64; NGREG],
    /// Always null: `ostd` saves no FPU state per task (`UC_FP_XSTATE` stays clear).
    pub fpregs: u64,
    pub reserved: [u64; 8],
}

impl MContext {
    pub const LEN: usize = size_of::<MContext>();

    /// Captures the interrupted register state, clearing DF and TF for handler entry.
    pub fn capture(ctx: &UserContext) -> Self {
        let regs = ctx.general_regs();
        let mut gregs = [0u64; NGREG];
        gregs[GReg::R8 as usize] = regs.r8 as u64;
        gregs[GReg::R9 as usize] = regs.r9 as u64;
        gregs[GReg::R10 as usize] = regs.r10 as u64;
        gregs[GReg::R11 as usize] = regs.r11 as u64;
        gregs[GReg::R12 as usize] = regs.r12 as u64;
        gregs[GReg::R13 as usize] = regs.r13 as u64;
        gregs[GReg::R14 as usize] = regs.r14 as u64;
        gregs[GReg::R15 as usize] = regs.r15 as u64;
        gregs[GReg::Rdi as usize] = regs.rdi as u64;
        gregs[GReg::Rsi as usize] = regs.rsi as u64;
        gregs[GReg::Rbp as usize] = regs.rbp as u64;
        gregs[GReg::Rbx as usize] = regs.rbx as u64;
        gregs[GReg::Rdx as usize] = regs.rdx as u64;
        gregs[GReg::Rax as usize] = regs.rax as u64;
        gregs[GReg::Rcx as usize] = regs.rcx as u64;
        gregs[GReg::Rsp as usize] = regs.rsp as u64;
        gregs[GReg::Rip as usize] = regs.rip as u64;
        gregs[GReg::Efl as usize] = (ctx.rflags() & !(X86_EFLAGS_DF | X86_EFLAGS_TF)) as u64;
        gregs[GReg::CsGsFs as usize] = CSGSFS as u64;

        Self {
            gregs,
            fpregs: 0,
            reserved: [0; 8],
        }
    }
}

const X86_EFLAGS_DF: usize = 1 << 10;
const X86_EFLAGS_TF: usize = 1 << 8;

/// Signal user context (`ucontext_t`, 424 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct UContext {
    pub uc_flags: u64,
    pub uc_link: u64,
    pub uc_stack: SigStack,
    pub uc_mcontext: MContext,
    /// Signal mask active before delivery, restored by `rt_sigreturn(2)`.
    pub uc_sigmask: crate::uapi::signal::SigSet,
}

impl UContext {
    pub const LEN: usize = size_of::<UContext>();
}

/// Signal frame (`rt_sigframe`, 560 bytes) pushed onto the user stack.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct SigFrame {
    /// Restorer address returned to by the handler (`sa_restorer` or [`TRAMPOLINE_ADDR`]).
    pub pretcode: u64,
    pub uc: UContext,
    pub info: SigInfo,
}

impl SigFrame {
    pub const LEN: usize = size_of::<SigFrame>();

    /// Builds a frame for `ctx` with `saved_mask` and restorer `pretcode`.
    pub fn new(ctx: &UserContext, info: SigInfo, saved_mask: &SigSet, pretcode: u64) -> Self {
        let uc = UContext {
            uc_flags: 0,
            uc_link: 0,
            uc_stack: SigStack::default(),
            uc_mcontext: MContext::capture(ctx),
            uc_sigmask: *saved_mask,
        };

        Self {
            pretcode,
            uc,
            info,
        }
    }

    /// The address the handler sees as its third argument.
    pub fn ucontext_addr(frame_addr: usize) -> usize {
        frame_addr + size_of::<u64>()
    }

    /// The address the handler sees as its second argument.
    pub fn siginfo_addr(frame_addr: usize) -> usize {
        frame_addr + size_of::<u64>() + UContext::LEN
    }
}

/// Reason signal frame construction failed.
#[derive(Clone, Copy, Debug)]
pub enum FrameError {
    NoStack { needed: usize },
    Unwritable,
}

impl fmt::Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoStack { needed } => write!(f, "no room for a {needed}-byte signal frame"),
            Self::Unwritable => f.write_str("the signal frame could not be written"),
        }
    }
}

impl From<FrameError> for Errno {
    fn from(err: FrameError) -> Self {
        match err {
            // Linux returns EAGAIN when an alternate stack cannot hold the frame.
            FrameError::NoStack { .. } => Errno::EAGAIN,
            FrameError::Unwritable => Errno::EFAULT,
        }
    }
}

/// Builds and writes a signal frame on `altstack` (when `on_stack` and eligible) or the current stack.
pub fn build_frame(
    vmar: &Arc<Vmar>,
    ctx: &UserContext,
    info: &SigInfo,
    saved_mask: &SigSet,
    altstack: &SigStack,
    on_stack: bool,
    pretcode: usize,
) -> Result<usize, FrameError> {
    let sp = ctx.stack_pointer();
    let frame = SigFrame::new(ctx, *info, saved_mask, pretcode as u64);

    if on_stack && altstack.enabled() && !altstack.contains(sp) {
        build_on_altstack(vmar, altstack, &frame)
    } else {
        build_on_current_stack(vmar, sp, &frame)
    }
}

fn build_on_altstack(
    vmar: &Arc<Vmar>,
    altstack: &SigStack,
    frame: &SigFrame,
) -> Result<usize, FrameError> {
    let top = altstack.ss_sp + altstack.ss_size;
    let addr = match align_frame_below(top) {
        Some(addr) if addr >= altstack.ss_sp => addr,
        _ => {
            return Err(FrameError::NoStack {
                needed: SigFrame::LEN,
            });
        }
    };
    write_frame(vmar, addr, frame)?;
    Ok(addr)
}

fn build_on_current_stack(
    vmar: &Arc<Vmar>,
    sp: usize,
    frame: &SigFrame,
) -> Result<usize, FrameError> {
    let addr = match align_frame_below(sp) {
        Some(addr) => addr,
        None => {
            return Err(FrameError::NoStack {
                needed: SigFrame::LEN,
            });
        }
    };
    // Keep the frame above the null/guard page so overflow faults on the next push.
    if addr < PAGE_SIZE {
        return Err(FrameError::NoStack {
            needed: SigFrame::LEN,
        });
    }
    write_frame(vmar, addr, frame)?;
    Ok(addr)
}

fn write_frame(vmar: &Arc<Vmar>, addr: usize, frame: &SigFrame) -> Result<(), FrameError> {
    let _ = vmar.populate_range(&(addr..addr + SigFrame::LEN));
    vmar.activate();

    UserPtr::new(addr)
        .write_bytes(vmar, &frame.to_bytes())
        .map_err(|_| FrameError::Unwritable)
}

/// Maps the fallback `rt_sigreturn` trampoline page into `vmar` as read-execute.
pub fn install_trampoline(vmar: &Arc<Vmar>) -> Result<()> {
    let aligned = TRAMPOLINE_ADDR & !(PAGE_SIZE - 1);

    // `mprotect` can only narrow permissions, so map RWX briefly to write the stub then drop WRITE.
    vmar.mmap_anonymous(
        aligned,
        PAGE_SIZE,
        VmPerms::READ | VmPerms::WRITE | VmPerms::EXEC,
        MmapFlags::PRIVATE | MmapFlags::FIXED,
    )?;
    vmar.populate_range(&(aligned..aligned + PAGE_SIZE))?;
    vmar.activate();

    UserPtr::new(TRAMPOLINE_ADDR).write_bytes(vmar, &TRAMPOLINE[..])?;

    vmar.mprotect(VmPerms::READ | VmPerms::EXEC, aligned..aligned + PAGE_SIZE)?;
    Ok(())
}

/// Reads an untrusted [`SigFrame`] from user space for `rt_sigreturn(2)`.
pub fn read_frame(vmar: &Arc<Vmar>, addr: usize) -> Result<SigFrame> {
    let mut buf = [0u8; SigFrame::LEN];
    UserPtr::new(addr).read_bytes(vmar, &mut buf)?;
    Ok(SigFrame::from_bytes(&buf))
}

/// Byte offsets inside the serialized `rt_sigframe`.
mod offset {
    use super::{MContext, NGREG, UContext};
    use core::mem::size_of;

    pub const PRETCODE: usize = 0;
    pub const UC: usize = PRETCODE + size_of::<u64>();

    pub const UC_FLAGS: usize = UC;
    pub const UC_LINK: usize = UC_FLAGS + size_of::<u64>();
    pub const UC_STACK_SP: usize = UC_LINK + size_of::<u64>();
    pub const UC_STACK_FLAGS: usize = UC_STACK_SP + size_of::<u64>();
    // 4 bytes of alignment padding sit between 32-bit `ss_flags` and 64-bit `ss_size`.
    pub const UC_STACK_SIZE: usize = UC_STACK_FLAGS + size_of::<u32>() * 2;

    pub const UC_MCONTEXT: usize = UC_STACK_SIZE;
    pub const GREGS: usize = UC + UC_MCONTEXT;
    pub const FPREGS: usize = GREGS + NGREG * size_of::<u64>();

    pub const INFO: usize = UC + size_of::<UContext>();
    pub const UC_SIGMASK: usize = UC + UC_MCONTEXT + size_of::<MContext>();
    pub const INFO_SI_ADDR: usize = INFO + 16;
    pub const INFO_SI_PID: usize = INFO + 24;
    pub const INFO_SI_UID: usize = INFO + 28;
    pub const INFO_SI_STATUS: usize = INFO + 32;
}

use offset::*;

/// Encodes a `SigInfo` structure into 128-byte Linux `siginfo_t` user-space layout.
pub fn encode_siginfo(info: &SigInfo) -> [u8; 128] {
    let mut buf = [0u8; 128];
    buf[0..4].copy_from_slice(&info.si_signo.to_le_bytes());
    buf[4..8].copy_from_slice(&info.si_errno.to_le_bytes());
    buf[8..12].copy_from_slice(&info.si_code.to_le_bytes());
    if info.si_code == code::TIMER {
        buf[16..20].copy_from_slice(&(info.si_addr as u32).to_le_bytes());
        buf[20..24].copy_from_slice(&0u32.to_le_bytes());
        buf[24..32].copy_from_slice(&info.si_value.to_le_bytes());
    } else {
        buf[16..24].copy_from_slice(&info.si_addr.to_le_bytes());
        buf[24..28].copy_from_slice(&info.si_pid.to_le_bytes());
        buf[28..32].copy_from_slice(&info.si_uid.to_le_bytes());
        buf[32..36].copy_from_slice(&info.si_status.to_le_bytes());
    }
    buf
}

impl SigFrame {
    /// Serializes the frame into the user-space `rt_sigframe` byte layout.
    pub fn to_bytes(&self) -> [u8; Self::LEN] {
        let mut buf = [0u8; Self::LEN];

        buf[PRETCODE..PRETCODE + 8].copy_from_slice(&self.pretcode.to_le_bytes());
        buf[UC_FLAGS..UC_FLAGS + 8].copy_from_slice(&self.uc.uc_flags.to_le_bytes());
        buf[UC_LINK..UC_LINK + 8].copy_from_slice(&self.uc.uc_link.to_le_bytes());
        buf[UC_STACK_SP..UC_STACK_SP + 8]
            .copy_from_slice(&(self.uc.uc_stack.ss_sp as u64).to_le_bytes());
        buf[UC_STACK_FLAGS..UC_STACK_FLAGS + 4]
            .copy_from_slice(&self.uc.uc_stack.ss_flags.to_le_bytes());
        buf[UC_STACK_SIZE..UC_STACK_SIZE + 8]
            .copy_from_slice(&(self.uc.uc_stack.ss_size as u64).to_le_bytes());
        for (i, &g) in self.uc.uc_mcontext.gregs.iter().enumerate() {
            let at = GREGS + i * 8;
            buf[at..at + 8].copy_from_slice(&g.to_le_bytes());
        }
        buf[FPREGS..FPREGS + 8].copy_from_slice(&self.uc.uc_mcontext.fpregs.to_le_bytes());
        buf[UC_SIGMASK..UC_SIGMASK + SIGSET_WORDS * 8]
            .copy_from_slice(&encode_sigset(&self.uc.uc_sigmask));

        buf[INFO..INFO + 128].copy_from_slice(&encode_siginfo(&self.info));

        buf
    }

    /// Deserializes a `SigFrame` from user-space bytes.
    pub fn from_bytes(buf: &[u8; Self::LEN]) -> Self {
        let mut gregs = [0u64; NGREG];
        for (i, slot) in gregs.iter_mut().enumerate() {
            let at = GREGS + i * 8;
            *slot = read_u64(&buf[at..at + 8]);
        }

        let si_code = read_u32(&buf[INFO + 8..INFO + 12]) as i32;
        let (si_addr, si_pid, si_uid, si_status, si_value) =
            if si_code == code::TIMER {
                (
                    read_u32(&buf[INFO + 16..INFO + 20]) as u64,
                    0,
                    0,
                    0,
                    read_u64(&buf[INFO + 24..INFO + 32]),
                )
            } else {
                (
                    read_u64(&buf[INFO_SI_ADDR..INFO_SI_ADDR + 8]),
                    read_u32(&buf[INFO_SI_PID..INFO_SI_PID + 4]) as i32,
                    read_u32(&buf[INFO_SI_UID..INFO_SI_UID + 4]),
                    read_u32(&buf[INFO_SI_STATUS..INFO_SI_STATUS + 4]) as i32,
                    0,
                )
            };

        Self {
            pretcode: read_u64(&buf[PRETCODE..PRETCODE + 8]),
            uc: UContext {
                uc_flags: read_u64(&buf[UC_FLAGS..UC_FLAGS + 8]),
                uc_link: read_u64(&buf[UC_LINK..UC_LINK + 8]),
                uc_stack: SigStack {
                    ss_sp: read_u64(&buf[UC_STACK_SP..UC_STACK_SP + 8]) as usize,
                    ss_flags: read_u32(&buf[UC_STACK_FLAGS..UC_STACK_FLAGS + 4]),
                    _pad: 0,
                    ss_size: read_u64(&buf[UC_STACK_SIZE..UC_STACK_SIZE + 8]) as usize,
                },
                uc_mcontext: MContext {
                    gregs,
                    fpregs: read_u64(&buf[FPREGS..FPREGS + 8]),
                    reserved: [0; 8],
                },
                uc_sigmask: decode_sigset(&buf[UC_SIGMASK..UC_SIGMASK + SIGSET_WORDS * 8]),
            },
            info: SigInfo {
                si_signo: read_u32(&buf[INFO..INFO + 4]) as i32,
                si_errno: read_u32(&buf[INFO + 4..INFO + 8]) as i32,
                si_code,
                _pad0: 0,
                si_addr,
                si_pid,
                si_uid,
                si_status,
                _pad1: 0,
                si_utime: 0,
                si_stime: 0,
                si_value,
                _pad2: [0; 8],
            },
        }
    }
}

fn encode_sigset(set: &SigSet) -> [u8; SIGSET_WORDS * 8] {
    let mut buf = [0u8; SIGSET_WORDS * 8];
    for (i, &word) in set.0.iter().enumerate() {
        buf[i * 8..(i + 1) * 8].copy_from_slice(&word.to_le_bytes());
    }
    buf
}

fn decode_sigset(buf: &[u8]) -> SigSet {
    let mut words = [0u64; SIGSET_WORDS];
    for (i, slot) in words.iter_mut().enumerate() {
        *slot = u64::from_le_bytes(buf[i * 8..(i + 1) * 8].try_into().expect("eight bytes"));
    }
    SigSet(words)
}

fn read_u64(bytes: &[u8]) -> u64 {
    u64::from_le_bytes(bytes.try_into().expect("slice is eight bytes"))
}

fn read_u32(bytes: &[u8]) -> u32 {
    u32::from_le_bytes(bytes.try_into().expect("slice is four bytes"))
}

/// Validated register state extracted from a user-supplied [`SigFrame`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Restore {
    pub rax: u64,
    pub rbx: u64,
    pub rcx: u64,
    pub rdx: u64,
    pub rsi: u64,
    pub rdi: u64,
    pub rbp: u64,
    pub rsp: u64,
    pub r8: u64,
    pub r9: u64,
    pub r10: u64,
    pub r11: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
    pub rip: u64,
    pub rflags: u64,
}

impl Restore {
    /// Validates untrusted frame pointers (`rip` canonical & executable, `rsp` canonical) and extracts registers.
    pub fn from_frame(frame: &SigFrame, vmar: &Arc<Vmar>) -> Result<Self> {
        let g = &frame.uc.uc_mcontext.gregs;
        let rip = g[GReg::Rip as usize];
        let rsp = g[GReg::Rsp as usize];

        if !is_canonical(rip) {
            ostd::error!("Restore::from_frame: non-canonical RIP {:#x}", rip);
            crate::return_errno!(EFAULT, "the restored instruction pointer is not canonical");
        }
        if !is_canonical(rsp) {
            ostd::error!("Restore::from_frame: non-canonical RSP {:#x}", rsp);
            crate::return_errno!(EFAULT, "the restored stack pointer is not canonical");
        }
        if !is_executable(vmar, rip as usize) {
            ostd::error!("Restore::from_frame: non-executable RIP {:#x}", rip);
            crate::return_errno!(EFAULT, "the restored instruction pointer is not executable");
        }

        Ok(Self {
            rax: g[GReg::Rax as usize],
            rbx: g[GReg::Rbx as usize],
            rcx: g[GReg::Rcx as usize],
            rdx: g[GReg::Rdx as usize],
            rsi: g[GReg::Rsi as usize],
            rdi: g[GReg::Rdi as usize],
            rbp: g[GReg::Rbp as usize],
            rsp,
            r8: g[GReg::R8 as usize],
            r9: g[GReg::R9 as usize],
            r10: g[GReg::R10 as usize],
            r11: g[GReg::R11 as usize],
            r12: g[GReg::R12 as usize],
            r13: g[GReg::R13 as usize],
            r14: g[GReg::R14 as usize],
            r15: g[GReg::R15 as usize],
            rip,
            rflags: g[GReg::Efl as usize],
        })
    }

    /// Applies the restored registers to `ctx`.
    pub fn apply(&self, ctx: &mut UserContext) {
        let regs = ctx.general_regs_mut();
        regs.rax = self.rax as usize;
        regs.rbx = self.rbx as usize;
        regs.rcx = self.rcx as usize;
        regs.rdx = self.rdx as usize;
        regs.rsi = self.rsi as usize;
        regs.rdi = self.rdi as usize;
        regs.rbp = self.rbp as usize;
        regs.rsp = self.rsp as usize;
        regs.r8 = self.r8 as usize;
        regs.r9 = self.r9 as usize;
        regs.r10 = self.r10 as usize;
        regs.r11 = self.r11 as usize;
        regs.r12 = self.r12 as usize;
        regs.r13 = self.r13 as usize;
        regs.r14 = self.r14 as usize;
        regs.r15 = self.r15 as usize;
        regs.rip = self.rip as usize;
        ctx.set_rflags(self.rflags as usize);
    }
}

/// Points the register file at a handler.
#[derive(Clone, Copy, Debug)]
pub struct Redirect {
    pub rip: usize,
    pub rsp: usize,
    pub rdi: usize,
    pub rsi: usize,
    pub rdx: usize,
}

impl Redirect {
    // Handler returns via `ret`, so RSP starts at `frame_addr` where `pretcode` resides.
    fn stack_top(frame_addr: usize) -> usize {
        frame_addr
    }

    /// Three-argument handler entry: `(int signo, siginfo_t *info, ucontext_t *uc)`.
    pub fn with_siginfo(handler: usize, sig: Signal, frame_addr: usize) -> Self {
        Self {
            rip: handler,
            rsp: Self::stack_top(frame_addr),
            rdi: sig.as_u32() as usize,
            rsi: SigFrame::siginfo_addr(frame_addr),
            rdx: SigFrame::ucontext_addr(frame_addr),
        }
    }

    /// One-argument handler entry: `(int signo)`.
    pub fn plain(handler: usize, sig: Signal, frame_addr: usize) -> Self {
        Self {
            rip: handler,
            rsp: Self::stack_top(frame_addr),
            rdi: sig.as_u32() as usize,
            rsi: 0,
            rdx: 0,
        }
    }

    /// Configures `ctx` to enter the signal handler.
    pub fn apply(&self, ctx: &mut UserContext) {
        let regs = ctx.general_regs_mut();
        regs.rip = self.rip;
        regs.rsp = self.rsp;
        // Zero RAX (ABI return/varargs) and clear TF to avoid single-stepping handler entry.
        regs.rax = 0;
        regs.rdi = self.rdi;
        regs.rsi = self.rsi;
        regs.rdx = self.rdx;
        ctx.set_rflags(ctx.rflags() & !(1 << 8));
    }
}

/// Returns the highest address `<= limit - SigFrame::LEN` aligned to `16n + 8` (function-entry RSP),
/// so the handler's `ret` pops `pretcode` cleanly.
fn align_frame_below(limit: usize) -> Option<usize> {
    let mut block = limit & !0xf;
    // Use `block` directly if `limit` is already at `+8`, otherwise step down one 16-byte block.
    if limit - block != 8 {
        block = block.checked_sub(16)?;
    }
    let addr = block.checked_sub(SigFrame::LEN)?.checked_add(8)?;
    debug_assert_eq!(
        addr & 0xf,
        8,
        "a signal frame must start eight bytes into a block"
    );
    debug_assert!(
        addr + SigFrame::LEN <= limit,
        "the frame must fit below the limit"
    );
    Some(addr)
}

/// Whether `addr` lies in a mapping that user space may execute.
fn is_executable(vmar: &Arc<Vmar>, addr: usize) -> bool {
    vmar.mappings_in(addr..addr + 1)
        .iter()
        .any(|m| m.perms().contains(VmPerms::EXEC) && m.start() <= addr && addr < m.end())
}

/// Whether an address has the x86-64 canonical form.
fn is_canonical(addr: u64) -> bool {
    let high = addr >> 48;
    high == 0 || high == 0xffff
}
