use crate::proc::pid_table::PROCESS_TABLE;
use crate::proc::pid_table::Pid;
use crate::proc::process::Process;
use crate::syscall::{SyscallResult, to_continue_i32};
use crate::vm::vma::VmaManager;
use ostd::Error;
use ostd::arch::cpu::context::UserContext;

/// `getpid()` — returns the process ID of the calling process (SYS_getpid = 39).
pub fn syscall_getpid(
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    to_continue_i32(Ok(Process::current().pid.as_u32() as i32))
}

/// `getppid()` — returns the parent process ID of the calling process (SYS_getppid = 110).
pub fn syscall_getppid(
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    let ppid = Process::current().ppid.map_or(0, |p| p.as_u32());
    to_continue_i32(Ok(ppid as i32))
}

/// `getpgid()` — returns the process group ID of the process (SYS_getpgid = 121).
pub fn syscall_getpgid(
    arg0: usize, // pid
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    let pid_raw = arg0 as u32;
    let current = Process::current();

    let target_pid = if pid_raw == 0 {
        current.pid
    } else {
        Pid::from_raw(pid_raw)
    };

    if let Some(target) = PROCESS_TABLE.get_process(target_pid) {
        to_continue_i32(Ok(target.pgid.as_u32() as i32))
    } else {
        to_continue_i32(Err(Error::InvalidArgs))
    }
}

/// `setpgid()` — sets the process group ID of a process (SYS_setpgid = 109).
pub fn syscall_setpgid(
    arg0: usize, // pid
    arg1: usize, // pgid
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    let pid_raw = arg0 as u32;
    let pgid_raw = arg1 as u32;
    let current = Process::current();

    let target_pid = if pid_raw == 0 {
        current.pid
    } else {
        Pid::from_raw(pid_raw)
    };

    let target_is_valid = if target_pid == current.pid {
        true
    } else {
        let children = current.children.lock();
        children.contains(&target_pid)
    };

    if !target_is_valid {
        return to_continue_i32(Err(Error::InvalidArgs));
    }

    let target_pgid = if pgid_raw == 0 {
        target_pid
    } else {
        Pid::from_raw(pgid_raw)
    };

    let mut result = Err(Error::InvalidArgs);
    PROCESS_TABLE.update_process(target_pid, |p| {
        if p.sid == current.sid {
            p.pgid = target_pgid;
            result = Ok(0);
        }
    });

    to_continue_i32(result)
}

/// `getsid()` — returns the session ID of the process (SYS_getsid = 124).
pub fn syscall_getsid(
    arg0: usize, // pid
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    let pid_raw = arg0 as u32;
    let current = Process::current();

    let target_pid = if pid_raw == 0 {
        current.pid
    } else {
        Pid::from_raw(pid_raw)
    };

    if let Some(target) = PROCESS_TABLE.get_process(target_pid) {
        to_continue_i32(Ok(target.sid.as_u32() as i32))
    } else {
        to_continue_i32(Err(Error::InvalidArgs))
    }
}

/// `setsid()` — creates a session and sets the process group ID (SYS_setsid = 112).
pub fn syscall_setsid(
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    let current = Process::current();

    let mut result = Err(Error::AccessDenied);

    PROCESS_TABLE.update_process(current.pid, |p| {
        if p.pid != p.pgid {
            p.pgid = p.pid;
            p.sid = p.pid;
            result = Ok(p.pid.as_u32() as i32);
        }
    });

    to_continue_i32(result)
}
