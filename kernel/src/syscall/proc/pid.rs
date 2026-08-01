use crate::proc::pid_table::PROCESS_TABLE;
use crate::proc::pid_table::Pid;
use crate::proc::process::Process;
use crate::syscall::SyscallResult;
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
    SyscallResult::from_result(Ok(Process::current().pid.as_u32() as i32))
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
    let ppid = Process::current()
        .ppid
        .as_ref()
        .map_or(0, |p| p.pid.as_u32());
    SyscallResult::from_result(Ok(ppid as i32))
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
        SyscallResult::from_result(Ok(target.pgid().as_u32() as i32))
    } else {
        SyscallResult::from_result(Err::<i32, _>(Error::InvalidArgs))
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
        return SyscallResult::from_result(Err::<i32, _>(Error::InvalidArgs));
    }

    let target_pgid = if pgid_raw == 0 {
        target_pid
    } else {
        Pid::from_raw(pgid_raw)
    };

    let mut result: Result<i32, Error> = Err(Error::InvalidArgs);
    if let Some(mut p) = PROCESS_TABLE.get_process(target_pid) {
        if p.session_id == current.session_id {
            if p.setpgid(target_pgid).is_ok() {
                result = Ok(0);
            }
        }
    }

    SyscallResult::from_result(result)
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
        SyscallResult::from_result(Ok(target.session_id.as_u32() as i32))
    } else {
        SyscallResult::from_result(Err::<i32, _>(Error::InvalidArgs))
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

    let mut result: Result<i32, Error> = Err(Error::AccessDenied);

    PROCESS_TABLE.update_process(current.pid, |p| {
        if p.pid != p.pgid() {
            let _ = p.setpgid(p.pid);
            p.session_id = p.pid;
            result = Ok(p.pid.as_u32() as i32);
        }
    });

    SyscallResult::from_result(result)
}
