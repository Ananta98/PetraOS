/// `rt_sigqueueinfo(pid, sig, uinfo)` and `rt_tgsigqueueinfo(tgid, tid, sig, uinfo)` —
/// send a signal with supplementary signal metadata to a process or thread group
/// (SYS_rt_sigqueueinfo = 129, SYS_rt_tgsigqueueinfo = 297).
///
/// Linux semantics:
/// - `rt_sigqueueinfo` delivers signal `sig` with `siginfo_t` metadata from `uinfo`
///   to the process identified by `pid`.
/// - `rt_tgsigqueueinfo` delivers signal `sig` with `siginfo_t` metadata from `uinfo`
///   to the specific process/thread group `tgid` (and thread `tid`).
/// - `sig == 0`: performs existence check for target process without enqueueing a signal.
/// - `si_signo` in `uinfo` must match `sig`.
///
/// Returns `0` on success, or a negated `errno` on failure.
use crate::ipc::signal::types::{SIGRTMAX, SigInfo};
use crate::proc::pid_table::{PROCESS_TABLE, Pid};
use crate::proc::process::Process;
use crate::syscall::{SyscallResult};
use crate::vm::vma::VmaManager;
use ostd::Error;

/// Size of `siginfo_t` in bytes on Linux x86_64.
const SIGINFO_SIZE: usize = 128;
const SI_SIGNO_OFFSET: usize = 0;
const SI_CODE_OFFSET: usize = 8;

/// Copy and parse `siginfo_t` fields (`si_signo`, `si_code`) from user space.
fn read_siginfo_from_user(vm: &VmaManager, uinfo_ptr: usize) -> Result<(i32, i32), Error> {
    if uinfo_ptr == 0 {
        return Err(Error::InvalidArgs);
    }
    let mut buf = [0u8; SIGINFO_SIZE];
    vm.copy_from_user(uinfo_ptr, &mut buf)?;

    let si_signo = i32::from_ne_bytes(
        buf[SI_SIGNO_OFFSET..SI_SIGNO_OFFSET + 4]
            .try_into()
            .unwrap_or([0u8; 4]),
    );
    let si_code = i32::from_ne_bytes(
        buf[SI_CODE_OFFSET..SI_CODE_OFFSET + 4]
            .try_into()
            .unwrap_or([0u8; 4]),
    );

    Ok((si_signo, si_code))
}

/// System call entry: `rt_sigqueueinfo(pid, sig, uinfo)`.
pub fn syscall_rt_sigqueueinfo(
    arg0: usize, // pid_t pid
    arg1: usize, // int sig
    arg2: usize, // siginfo_t __user *uinfo
    _: usize,
    _: usize,
    _: usize,
    vm: &VmaManager,
    _: &mut ostd::arch::cpu::context::UserContext,
) -> SyscallResult {
    let pid_raw = arg0 as isize;
    let signum = arg1 as u32;
    let uinfo_ptr = arg2;

    if pid_raw <= 0 {
        return SyscallResult::from_err(Error::InvalidArgs);
    }
    if signum > SIGRTMAX {
        return SyscallResult::from_err(Error::InvalidArgs);
    }

    let target = Pid::from_raw(pid_raw as u32);
    if PROCESS_TABLE.get_process(target).is_none() {
        return SyscallResult::from_err(Error::InvalidArgs);
    }

    if signum == 0 {
        // Validity check only.
        return SyscallResult::from_result(Ok(()));
    }

    let (si_signo, si_code) = match read_siginfo_from_user(vm, uinfo_ptr) {
        Ok(res) => res,
        Err(err) => return SyscallResult::from_err(err),
    };

    if si_signo != signum as i32 {
        return SyscallResult::from_err(Error::InvalidArgs);
    }

    let sender_pid = Process::current().pid.as_u32();
    let info = SigInfo {
        signum,
        sender_pid,
        code: si_code,
    };

    SyscallResult::from_result(crate::ipc::dispatch::send_siginfo_to_pid(target, info))
}

/// System call entry: `rt_tgsigqueueinfo(tgid, tid, sig, uinfo)`.
pub fn syscall_rt_tgsigqueueinfo(
    arg0: usize, // pid_t tgid
    arg1: usize, // pid_t tid
    arg2: usize, // int sig
    arg3: usize, // siginfo_t __user *uinfo
    _: usize,
    _: usize,
    vm: &VmaManager,
    _: &mut ostd::arch::cpu::context::UserContext,
) -> SyscallResult {
    let tgid_raw = arg0 as isize;
    let tid_raw = arg1 as isize;
    let signum = arg2 as u32;
    let uinfo_ptr = arg3;

    if tgid_raw <= 0 || tid_raw <= 0 {
        return SyscallResult::from_err(Error::InvalidArgs);
    }
    if signum > SIGRTMAX {
        return SyscallResult::from_err(Error::InvalidArgs);
    }

    let target = Pid::from_raw(tgid_raw as u32);
    if PROCESS_TABLE.get_process(target).is_none() {
        return SyscallResult::from_err(Error::InvalidArgs);
    }

    if signum == 0 {
        // Validity check only.
        return SyscallResult::from_result(Ok(()));
    }

    let (si_signo, si_code) = match read_siginfo_from_user(vm, uinfo_ptr) {
        Ok(res) => res,
        Err(err) => return SyscallResult::from_err(err),
    };

    if si_signo != signum as i32 {
        return SyscallResult::from_err(Error::InvalidArgs);
    }

    let sender_pid = Process::current().pid.as_u32();
    let info = SigInfo {
        signum,
        sender_pid,
        code: si_code,
    };

    SyscallResult::from_result(crate::ipc::dispatch::send_siginfo_to_pid(target, info))
}
