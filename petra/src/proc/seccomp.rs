// SPDX-License-Identifier: GPL-2.0

//! Secure Computing (`seccomp`) subsystem.
//!
//! Implements strict and cBPF-filtered system call sandboxing according to
//! Linux seccomp ABI specifications.

use alloc::{sync::Arc, vec::Vec};
use ostd::arch::cpu::context::UserContext;

use crate::{
    proc::{
        ExitStatus, Thread,
        lifecycle::exit1,
    },
    uapi::{
        errno::{Errno, Result},
        seccomp::*,
        signal::{SigInfo, Signal},
    },
};

/// Maximum number of instructions allowed in a classic BPF filter program.
pub const BPF_MAXINSNS: usize = 4096;

/// Operational mode of the seccomp subsystem for a thread.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SeccompMode {
    /// Seccomp is inactive; all system calls proceed normally.
    #[default]
    Disabled = 0,
    /// Strict mode: only `read`, `write`, `_exit`, and `rt_sigreturn` are permitted.
    Strict = 1,
    /// Filter mode: system calls are evaluated against attached cBPF programs.
    Filter = 2,
}

/// Action precedence rank used when evaluating multiple stacked seccomp filters.
///
/// Higher rank indicates greater restriction and takes priority.
#[inline]
const fn action_rank(action: u32) -> u32 {
    match action {
        SECCOMP_RET_KILL_PROCESS => 8,
        SECCOMP_RET_KILL_THREAD => 7,
        SECCOMP_RET_TRAP => 6,
        SECCOMP_RET_ERRNO => 5,
        SECCOMP_RET_USER_NOTIF => 4,
        SECCOMP_RET_TRACE => 3,
        SECCOMP_RET_LOG => 2,
        SECCOMP_RET_ALLOW => 1,
        _ => 8, // Unknown actions default to highest restriction (kill).
    }
}

/// Outcome of evaluating seccomp filters against a system call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeccompOutcome {
    /// Allow the system call to proceed.
    Allow,
    /// Deliver `SIGSYS` (`SYS_SECCOMP`) to the thread and set return to `-ENOSYS`.
    Trap { errno: u32 },
    /// Do not execute the system call; set `rax` to `-errno`.
    Errno { errno: u32 },
    /// Kill the thread immediately with `SIGSYS`.
    KillThread,
    /// Kill the entire process immediately with `SIGSYS`.
    KillProcess,
}

/// Decision returned to the system call dispatcher.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyscallDecision {
    /// System call is permitted to execute.
    Allow,
    /// System call was intercepted; `rax` has been set to the returned value.
    Return(usize),
    /// Process or thread has been terminated.
    Terminate,
}

/// Thread-level seccomp state machine.
#[derive(Clone, Debug, Default)]
pub struct SeccompState {
    /// Current operational mode.
    pub mode: SeccompMode,
    /// Stack of installed cBPF filters (evaluated newest to oldest).
    pub filters: Vec<Arc<[SockFilter]>>,
}

impl SeccompState {
    /// Creates a new default (disabled) seccomp state.
    pub const fn new() -> Self {
        Self {
            mode: SeccompMode::Disabled,
            filters: Vec::new(),
        }
    }

    /// Returns the number of filters currently attached.
    #[inline]
    pub fn filter_count(&self) -> usize {
        self.filters.len()
    }
}

/// Validates a classic BPF filter program for seccomp execution.
pub fn validate_seccomp_bpf(filters: &[SockFilter]) -> Result<()> {
    let len = filters.len();
    if len == 0 || len > BPF_MAXINSNS {
        return Err(Errno::EINVAL);
    }

    // The last instruction must be a return instruction.
    let last = &filters[len - 1];
    if (last.code & 0x07) != BPF_RET {
        return Err(Errno::EINVAL);
    }

    for (i, insn) in filters.iter().enumerate() {
        let class = insn.code & 0x07;
        let mode_or_size = insn.code & 0xe0;

        match class {
            BPF_LD => {
                match mode_or_size {
                    BPF_ABS => {
                        // In seccomp, BPF_ABS loads must be 32-bit words (BPF_W)
                        // aligned to 4 bytes within sizeof(struct seccomp_data) (64 bytes).
                        if (insn.code & 0x18) != BPF_W {
                            return Err(Errno::EINVAL);
                        }
                        if insn.k % 4 != 0 || insn.k > 60 {
                            return Err(Errno::EINVAL);
                        }
                    }
                    BPF_IMM | BPF_LEN => {}
                    BPF_MEM => {
                        if insn.k >= 16 {
                            return Err(Errno::EINVAL);
                        }
                    }
                    _ => return Err(Errno::EINVAL),
                }
            }
            BPF_LDX => {
                match mode_or_size {
                    BPF_IMM | BPF_LEN => {}
                    BPF_MEM => {
                        if insn.k >= 16 {
                            return Err(Errno::EINVAL);
                        }
                    }
                    BPF_ABS => {
                        if (insn.code & 0x18) != BPF_W || insn.k % 4 != 0 || insn.k > 60 {
                            return Err(Errno::EINVAL);
                        }
                    }
                    _ => return Err(Errno::EINVAL),
                }
            }
            BPF_ST | BPF_STX => {
                if insn.k >= 16 {
                    return Err(Errno::EINVAL);
                }
            }
            BPF_ALU => {
                let op = insn.code & 0xf0;
                let src = insn.code & 0x08;
                if (op == BPF_DIV || op == BPF_MOD) && src == BPF_K && insn.k == 0 {
                    // Division or modulo by immediate zero is rejected statically.
                    return Err(Errno::EINVAL);
                }
            }
            BPF_JMP => {
                let op = insn.code & 0xf0;
                if op == BPF_JA {
                    // Unconditional jump.
                    if i + 1 + insn.k as usize >= len {
                        return Err(Errno::EINVAL);
                    }
                } else {
                    // Conditional jump. Both true and false branch offsets must be within bounds.
                    if i + 1 + insn.jt as usize >= len || i + 1 + insn.jf as usize >= len {
                        return Err(Errno::EINVAL);
                    }
                }
            }
            BPF_RET => {
                let rtype = insn.code & 0x18;
                if rtype != BPF_K && rtype != BPF_A {
                    return Err(Errno::EINVAL);
                }
            }
            BPF_MISC => {
                let sub_op = insn.code & 0xf8;
                if sub_op != BPF_TAX && sub_op != BPF_TXA {
                    return Err(Errno::EINVAL);
                }
            }
            _ => return Err(Errno::EINVAL),
        }
    }

    Ok(())
}

/// Executes a verified classic BPF program against the given `SeccompData`.
pub fn run_seccomp_bpf(filters: &[SockFilter], data: &SeccompData) -> u32 {
    let mut pc = 0usize;
    let mut a = 0u32;
    let mut x = 0u32;
    let mut mem = [0u32; 16];
    let len = filters.len();

    while pc < len {
        let insn = &filters[pc];
        let class = insn.code & 0x07;

        match class {
            BPF_LD => {
                let mode = insn.code & 0xe0;
                match mode {
                    BPF_ABS => {
                        a = data.read_u32(insn.k).unwrap_or(0);
                    }
                    BPF_IMM => {
                        a = insn.k;
                    }
                    BPF_MEM => {
                        a = mem[(insn.k as usize) & 0x0f];
                    }
                    BPF_LEN => {
                        a = SeccompData::LEN as u32;
                    }
                    _ => return SECCOMP_RET_KILL_PROCESS,
                }
                pc += 1;
            }
            BPF_LDX => {
                let mode = insn.code & 0xe0;
                match mode {
                    BPF_IMM => {
                        x = insn.k;
                    }
                    BPF_MEM => {
                        x = mem[(insn.k as usize) & 0x0f];
                    }
                    BPF_LEN => {
                        x = SeccompData::LEN as u32;
                    }
                    BPF_ABS => {
                        x = data.read_u32(insn.k).unwrap_or(0);
                    }
                    _ => return SECCOMP_RET_KILL_PROCESS,
                }
                pc += 1;
            }
            BPF_ST => {
                mem[(insn.k as usize) & 0x0f] = a;
                pc += 1;
            }
            BPF_STX => {
                mem[(insn.k as usize) & 0x0f] = x;
                pc += 1;
            }
            BPF_ALU => {
                let op = insn.code & 0xf0;
                let src = insn.code & 0x08;
                let v = if src == BPF_X { x } else { insn.k };

                match op {
                    BPF_ADD => a = a.wrapping_add(v),
                    BPF_SUB => a = a.wrapping_sub(v),
                    BPF_MUL => a = a.wrapping_mul(v),
                    BPF_DIV => {
                        if v == 0 {
                            return 0;
                        }
                        a /= v;
                    }
                    BPF_MOD => {
                        if v == 0 {
                            return 0;
                        }
                        a %= v;
                    }
                    BPF_AND => a &= v,
                    BPF_OR => a |= v,
                    BPF_XOR => a ^= v,
                    BPF_LSH => a = if v >= 32 { 0 } else { a << v },
                    BPF_RSH => a = if v >= 32 { 0 } else { a >> v },
                    BPF_NEG => a = 0u32.wrapping_sub(a),
                    _ => return SECCOMP_RET_KILL_PROCESS,
                }
                pc += 1;
            }
            BPF_JMP => {
                let op = insn.code & 0xf0;
                if op == BPF_JA {
                    pc += 1 + insn.k as usize;
                } else {
                    let src = insn.code & 0x08;
                    let v = if src == BPF_X { x } else { insn.k };
                    let cond = match op {
                        BPF_JEQ => a == v,
                        BPF_JGT => a > v,
                        BPF_JGE => a >= v,
                        BPF_JSET => (a & v) != 0,
                        _ => return SECCOMP_RET_KILL_PROCESS,
                    };
                    if cond {
                        pc += 1 + insn.jt as usize;
                    } else {
                        pc += 1 + insn.jf as usize;
                    }
                }
            }
            BPF_RET => {
                let rtype = insn.code & 0x18;
                return if rtype == BPF_A { a } else { insn.k };
            }
            BPF_MISC => {
                let sub_op = insn.code & 0xf8;
                match sub_op {
                    BPF_TAX => x = a,
                    BPF_TXA => a = x,
                    _ => return SECCOMP_RET_KILL_PROCESS,
                }
                pc += 1;
            }
            _ => return SECCOMP_RET_KILL_PROCESS,
        }
    }

    SECCOMP_RET_KILL_PROCESS
}

/// Evaluates all installed filters and resolves the highest priority action.
pub fn eval_filters(filters: &[Arc<[SockFilter]>], data: &SeccompData) -> SeccompOutcome {
    let mut highest_rank = 0u32;
    let mut highest_action = SECCOMP_RET_ALLOW;
    let mut action_data = 0u16;

    // Evaluate newest to oldest.
    for filter in filters.iter().rev() {
        let ret = run_seccomp_bpf(filter, data);
        let action = ret & SECCOMP_RET_ACTION_FULL;
        let data_val = (ret & SECCOMP_RET_DATA) as u16;
        let rank = action_rank(action);

        if rank > highest_rank {
            highest_rank = rank;
            highest_action = action;
            action_data = data_val;
        } else if rank == highest_rank && action == SECCOMP_RET_ERRNO && data_val < action_data {
            action_data = data_val;
        }
    }

    match highest_action {
        SECCOMP_RET_KILL_PROCESS => SeccompOutcome::KillProcess,
        SECCOMP_RET_KILL_THREAD => SeccompOutcome::KillThread,
        SECCOMP_RET_TRAP => SeccompOutcome::Trap {
            errno: action_data as u32,
        },
        SECCOMP_RET_ERRNO => SeccompOutcome::Errno {
            errno: action_data as u32,
        },
        SECCOMP_RET_ALLOW | SECCOMP_RET_LOG => SeccompOutcome::Allow,
        _ => SeccompOutcome::KillProcess,
    }
}

/// Intercepts a system call before execution to enforce seccomp constraints.
pub fn check_syscall(
    sys_no: usize,
    args: &[usize; 6],
    ctx: &UserContext,
) -> SyscallDecision {
    let td = match Thread::current() {
        Some(td) => td,
        None => return SyscallDecision::Allow,
    };

    let (mode, filters) = {
        let inner = td.inner.lock();
        (inner.seccomp.mode, inner.seccomp.filters.clone())
    };

    match mode {
        SeccompMode::Disabled => SyscallDecision::Allow,
        SeccompMode::Strict => {
            // Strict mode permits only read (0), write (1), _exit (60), and rt_sigreturn (15).
            if sys_no == 0 || sys_no == 1 || sys_no == 60 || sys_no == 15 {
                SyscallDecision::Allow
            } else {
                td.post_signal(SigInfo::kernel(Signal::SIGKILL, crate::uapi::signal::code::KERNEL));
                exit1(&td, ExitStatus::Signaled { signal: Signal::SIGKILL, core_dumped: false });
                SyscallDecision::Terminate
            }
        }
        SeccompMode::Filter => {
            let data = SeccompData {
                nr: sys_no as i32,
                arch: AUDIT_ARCH_X86_64,
                instruction_pointer: ctx.general_regs().rip as u64,
                args: [
                    args[0] as u64,
                    args[1] as u64,
                    args[2] as u64,
                    args[3] as u64,
                    args[4] as u64,
                    args[5] as u64,
                ],
            };

            match eval_filters(&filters, &data) {
                SeccompOutcome::Allow => SyscallDecision::Allow,
                SeccompOutcome::Errno { errno } => {
                    SyscallDecision::Return((-(errno as isize)) as usize)
                }
                SeccompOutcome::Trap { errno } => {
                    td.post_signal(SigInfo::sigsys(
                        ctx.general_regs().rip,
                        sys_no as i32,
                        AUDIT_ARCH_X86_64,
                        errno as i32,
                    ));
                    SyscallDecision::Return((-Errno::ENOSYS.to_posix_raw() as isize) as usize)
                }
                SeccompOutcome::KillThread | SeccompOutcome::KillProcess => {
                    td.post_signal(SigInfo::sigsys(
                        ctx.general_regs().rip,
                        sys_no as i32,
                        AUDIT_ARCH_X86_64,
                        0,
                    ));
                    exit1(&td, ExitStatus::Signaled { signal: Signal::SIGSYS, core_dumped: false });
                    SyscallDecision::Terminate
                }
            }
        }
    }
}

/// Sets the seccomp mode to strict (`SECCOMP_SET_MODE_STRICT` or `prctl(PR_SET_SECCOMP, 1)`).
pub fn set_mode_strict() -> Result<()> {
    let td = Thread::current().ok_or(Errno::ESRCH)?;
    let mut inner = td.inner.lock();

    match inner.seccomp.mode {
        SeccompMode::Disabled => {
            inner.seccomp.mode = SeccompMode::Strict;
            Ok(())
        }
        SeccompMode::Strict => Ok(()), // Already strict; no-op.
        SeccompMode::Filter => Err(Errno::EINVAL), // Cannot transition from Filter to Strict.
    }
}

/// Capability bit 21 (`CAP_SYS_ADMIN`).
const CAP_SYS_ADMIN: u64 = 1 << 21;

/// Installs a new cBPF filter for the current thread and optionally synchronizes siblings.
pub fn set_mode_filter(flags: u32, insns: Vec<SockFilter>) -> Result<()> {
    if flags & !SECCOMP_FILTER_FLAG_MASK != 0 {
        return Err(Errno::EINVAL);
    }

    validate_seccomp_bpf(&insns)?;

    let td = Thread::current().ok_or(Errno::ESRCH)?;
    let proc = td.proc().ok_or(Errno::ESRCH)?;

    // Security check: either `no_new_privs` is active or process has `CAP_SYS_ADMIN`.
    {
        let proc_inner = proc.inner.lock();
        let has_no_new_privs = proc_inner.no_new_privs;
        let has_cap = (proc_inner.cap_effective & CAP_SYS_ADMIN) != 0;
        if !has_no_new_privs && !has_cap {
            return Err(Errno::EACCES);
        }
    }

    let filter_arc: Arc<[SockFilter]> = insns.into();

    let mut inner = td.inner.lock();
    if inner.seccomp.mode == SeccompMode::Strict {
        return Err(Errno::EINVAL); // Cannot add filters when strict mode is locked.
    }

    inner.seccomp.mode = SeccompMode::Filter;
    inner.seccomp.filters.push(filter_arc);
    let new_state = inner.seccomp.clone();
    drop(inner);

    // If TSYNC is requested, synchronize all sibling threads in the thread group.
    if flags & SECCOMP_FILTER_FLAG_TSYNC != 0 {
        let siblings = proc.inner.lock().threads.clone();
        for sib in siblings {
            if sib.tid == td.tid {
                continue;
            }
            let mut sib_inner = sib.inner.lock();
            if sib_inner.seccomp.mode == SeccompMode::Strict {
                return Err(Errno::ESRCH);
            }
            sib_inner.seccomp = new_state.clone();
        }
    }

    Ok(())
}

/// Checks whether a seccomp return action is supported by the kernel (`SECCOMP_GET_ACTION_AVAIL`).
pub fn get_action_avail(action: u32) -> Result<()> {
    match action {
        SECCOMP_RET_KILL_PROCESS
        | SECCOMP_RET_KILL_THREAD
        | SECCOMP_RET_TRAP
        | SECCOMP_RET_ERRNO
        | SECCOMP_RET_TRACE
        | SECCOMP_RET_LOG
        | SECCOMP_RET_ALLOW => Ok(()),
        _ => Err(Errno::EOPNOTSUPP),
    }
}
