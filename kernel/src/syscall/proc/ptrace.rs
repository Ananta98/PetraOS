//! Linux-compatible `ptrace` system call implementation (`SYS_ptrace` = 101).
//!
//! Provides process tracing, register manipulation, memory peek/poke, single-stepping,
//! attaching, and detaching.

use crate::arch::ptrace::{UserRegsStruct, peek_user_reg, poke_user_reg};
use crate::ipc::{SIGKILL, SIGSTOP, send_signal_to_pid};
use crate::proc::pid_table::{PROCESS_TABLE, Pid};
use crate::proc::process::Process;
use crate::syscall::{SyscallResult, to_continue, to_continue_unit};
use crate::vm::vma::VmaManager;
use ostd::Error;
use ostd::arch::cpu::context::UserContext;

pub const PTRACE_TRACEME: usize = 0;
pub const PTRACE_PEEKTEXT: usize = 1;
pub const PTRACE_PEEKDATA: usize = 2;
pub const PTRACE_PEEKUSER: usize = 3;
pub const PTRACE_POKETEXT: usize = 4;
pub const PTRACE_POKEDATA: usize = 5;
pub const PTRACE_POKEUSER: usize = 6;
pub const PTRACE_CONT: usize = 7;
pub const PTRACE_KILL: usize = 8;
pub const PTRACE_SINGLESTEP: usize = 9;
pub const PTRACE_GETREGS: usize = 12;
pub const PTRACE_SETREGS: usize = 13;
pub const PTRACE_ATTACH: usize = 16;
pub const PTRACE_DETACH: usize = 17;
pub const PTRACE_SETOPTIONS: usize = 0x4200;

/// System call entry point for `ptrace(request, pid, addr, data)`.
pub fn syscall_ptrace(
    request: usize,
    pid: usize,
    addr: usize,
    data: usize,
    _: usize,
    _: usize,
    vm: &VmaManager,
    context: &mut UserContext,
) -> SyscallResult {
    let current_proc = Process::current();

    match request {
        PTRACE_TRACEME => {
            if current_proc.is_traced() {
                return to_continue_unit(Err(Error::AccessDenied));
            }
            let tracer = current_proc
                .ppid
                .as_ref()
                .map(|parent| parent.pid)
                .unwrap_or_else(|| Pid::from_raw(1));
            current_proc.set_tracer_pid(Some(tracer));
            to_continue_unit(Ok(()))
        }

        PTRACE_PEEKTEXT | PTRACE_PEEKDATA => {
            let target_proc = if pid == 0 {
                current_proc.clone()
            } else {
                match PROCESS_TABLE.get_process(Pid::from_raw(pid as u32)) {
                    Some(p) => p,
                    None => return to_continue(Err(Error::InvalidArgs)),
                }
            };

            let mut word_bytes = [0u8; 8];
            if let Err(err) = target_proc.vm.copy_from_user(addr, &mut word_bytes) {
                return to_continue(Err(err));
            }

            let word_val = usize::from_ne_bytes(word_bytes);
            if data != 0 {
                if let Err(err) = vm.copy_to_user(data, &word_bytes) {
                    return to_continue(Err(err));
                }
            }
            to_continue(Ok(word_val))
        }

        PTRACE_POKETEXT | PTRACE_POKEDATA => {
            let target_proc = if pid == 0 {
                current_proc.clone()
            } else {
                match PROCESS_TABLE.get_process(Pid::from_raw(pid as u32)) {
                    Some(p) => p,
                    None => return to_continue_unit(Err(Error::InvalidArgs)),
                }
            };

            let word_bytes = data.to_ne_bytes();
            if let Err(err) = target_proc.vm.copy_to_user(addr, &word_bytes) {
                return to_continue_unit(Err(err));
            }
            to_continue_unit(Ok(()))
        }

        PTRACE_PEEKUSER => {
            let reg_idx = if addr >= 8 { addr / 8 } else { addr };
            match peek_user_reg(context, reg_idx) {
                Some(val) => {
                    let word_bytes = (val as usize).to_ne_bytes();
                    if data != 0 {
                        if let Err(err) = vm.copy_to_user(data, &word_bytes) {
                            return to_continue(Err(err));
                        }
                    }
                    to_continue(Ok(val as usize))
                }
                None => to_continue(Err(Error::InvalidArgs)),
            }
        }

        PTRACE_POKEUSER => {
            let reg_idx = if addr >= 8 { addr / 8 } else { addr };
            if poke_user_reg(context, reg_idx, data as u64) {
                to_continue_unit(Ok(()))
            } else {
                to_continue_unit(Err(Error::InvalidArgs))
            }
        }

        PTRACE_GETREGS => {
            if data == 0 {
                return to_continue_unit(Err(Error::InvalidArgs));
            }
            let regs = UserRegsStruct::from_user_context(context);
            if let Err(err) = vm.copy_to_user(data, &regs.to_bytes()) {
                return to_continue_unit(Err(err));
            }
            to_continue_unit(Ok(()))
        }

        PTRACE_SETREGS => {
            if data == 0 {
                return to_continue_unit(Err(Error::InvalidArgs));
            }
            let mut regs_bytes = [0u8; UserRegsStruct::SIZE];
            if let Err(err) = vm.copy_from_user(data, &mut regs_bytes) {
                return to_continue_unit(Err(err));
            }
            let regs = UserRegsStruct::from_bytes(&regs_bytes);
            regs.apply_to_user_context(context);
            to_continue_unit(Ok(()))
        }

        PTRACE_ATTACH => {
            if pid == 0 {
                return to_continue_unit(Err(Error::InvalidArgs));
            }
            let target_pid = Pid::from_raw(pid as u32);
            let target_proc = match PROCESS_TABLE.get_process(target_pid) {
                Some(p) => p,
                None => return to_continue_unit(Err(Error::InvalidArgs)),
            };

            target_proc.set_tracer_pid(Some(current_proc.pid));
            let _ = send_signal_to_pid(target_pid, SIGSTOP, current_proc.pid.as_u32());
            to_continue_unit(Ok(()))
        }

        PTRACE_DETACH => {
            let target_pid = if pid == 0 {
                current_proc.pid
            } else {
                Pid::from_raw(pid as u32)
            };

            let target_proc = match PROCESS_TABLE.get_process(target_pid) {
                Some(p) => p,
                None => return to_continue_unit(Err(Error::InvalidArgs)),
            };

            target_proc.set_tracer_pid(None);
            if data != 0 {
                let _ = send_signal_to_pid(target_pid, data as u32, current_proc.pid.as_u32());
            }
            to_continue_unit(Ok(()))
        }

        PTRACE_CONT => {
            let target_pid = if pid == 0 {
                current_proc.pid
            } else {
                Pid::from_raw(pid as u32)
            };

            if data != 0 {
                let _ = send_signal_to_pid(target_pid, data as u32, current_proc.pid.as_u32());
            }
            to_continue_unit(Ok(()))
        }

        PTRACE_SINGLESTEP => {
            // Enable Trap Flag (TF, bit 8) in UserContext RFLAGS register
            let rflags = context.rflags();
            context.set_rflags(rflags | 0x100);
            to_continue_unit(Ok(()))
        }

        PTRACE_KILL => {
            let target_pid = if pid == 0 {
                current_proc.pid
            } else {
                Pid::from_raw(pid as u32)
            };

            let _ = send_signal_to_pid(target_pid, SIGKILL, current_proc.pid.as_u32());
            to_continue_unit(Ok(()))
        }

        PTRACE_SETOPTIONS => {
            let target_proc = if pid == 0 {
                current_proc.clone()
            } else {
                match PROCESS_TABLE.get_process(Pid::from_raw(pid as u32)) {
                    Some(p) => p,
                    None => return to_continue_unit(Err(Error::InvalidArgs)),
                }
            };

            target_proc.set_ptrace_options(data as u32);
            to_continue_unit(Ok(()))
        }

        _ => to_continue(Err(Error::InvalidArgs)),
    }
}
