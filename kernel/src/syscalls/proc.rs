use super::{SyscallError, SyscallResult, is_user_ptr_valid, read_user_string};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::mm::vmm::paging::PageTable;
use crate::proc::ProcessId;

/// `sys_yield` (SYS_YIELD = 24)
/// Yield the CPU to another runnable thread.
pub fn sys_yield(_frame: &mut SyscallFrame) -> SyscallResult {
    crate::proc::thread::Thread::yield_cpu();
    Ok(0)
}

/// `sys_getpid` (SYS_GETPID = 39)
/// Get process ID.
pub fn sys_getpid(_frame: &mut SyscallFrame) -> SyscallResult {
    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    Ok(proc.pid.as_u64() as usize)
}

/// `sys_getppid` (SYS_GETPPID = 110)
/// Get parent process ID.
pub fn sys_getppid(_frame: &mut SyscallFrame) -> SyscallResult {
    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    Ok(proc.ppid.as_u64() as usize)
}

/// `sys_getpgrp` (SYS_GETPGRP = 111)
/// Get process group ID.
pub fn sys_getpgrp(_frame: &mut SyscallFrame) -> SyscallResult {
    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    Ok(proc.pgid.as_u64() as usize)
}

/// `sys_setpgid` (SYS_SETPGID = 109)
/// Set process group ID.
pub fn sys_setpgid(frame: &mut SyscallFrame) -> SyscallResult {
    let pid_raw = frame.arg1() as i32;
    let pgid_raw = frame.arg2() as i32;

    let target_pid = if pid_raw <= 0 {
        let current_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
        current_arc.lock().pid
    } else {
        ProcessId(pid_raw as u64)
    };

    let target_proc = crate::proc::find_process(target_pid).ok_or(SyscallError::ESRCH)?;
    let mut proc = target_proc.lock();

    let new_pgid = if pgid_raw <= 0 {
        proc.pid
    } else {
        ProcessId(pgid_raw as u64)
    };

    proc.pgid = new_pgid;
    Ok(0)
}

/// `sys_getuid` (SYS_GETUID = 102)
/// Get real user ID.
pub fn sys_getuid(_frame: &mut SyscallFrame) -> SyscallResult {
    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    Ok(proc.creds.uid as usize)
}

/// `sys_getgid` (SYS_GETGID = 104)
/// Get real group ID.
pub fn sys_getgid(_frame: &mut SyscallFrame) -> SyscallResult {
    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    Ok(proc.creds.gid as usize)
}

/// `sys_setuid` (SYS_SETUID = 105)
/// Set user ID.
pub fn sys_setuid(frame: &mut SyscallFrame) -> SyscallResult {
    let uid = frame.arg1() as u32;
    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let mut proc = proc_arc.lock();
    let creds = alloc::sync::Arc::make_mut(&mut proc.creds);
    creds.uid = uid;
    creds.euid = uid;
    Ok(0)
}

/// `sys_setgid` (SYS_SETGID = 106)
/// Set group ID.
pub fn sys_setgid(frame: &mut SyscallFrame) -> SyscallResult {
    let gid = frame.arg1() as u32;
    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let mut proc = proc_arc.lock();
    let creds = alloc::sync::Arc::make_mut(&mut proc.creds);
    creds.gid = gid;
    creds.egid = gid;
    Ok(0)
}

/// `sys_geteuid` (SYS_GETEUID = 107)
/// Get effective user ID.
pub fn sys_geteuid(_frame: &mut SyscallFrame) -> SyscallResult {
    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    Ok(proc.creds.euid as usize)
}

/// `sys_getegid` (SYS_GETEGID = 108)
/// Get effective group ID.
pub fn sys_getegid(_frame: &mut SyscallFrame) -> SyscallResult {
    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    Ok(proc.creds.egid as usize)
}

/// `sys_setsid` (SYS_SETSID = 112)
/// Creates a new session if the calling process is not a process group leader.
pub fn sys_setsid(_frame: &mut SyscallFrame) -> SyscallResult {
    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let mut proc = proc_arc.lock();
    proc.pgid = proc.pid;
    Ok(proc.pid.as_u64() as usize)
}

/// `sys_getgroups` (SYS_GETGROUPS = 115)
/// Get list of supplementary group IDs.
pub fn sys_getgroups(frame: &mut SyscallFrame) -> SyscallResult {
    let size = frame.arg1() as i32;
    let list_ptr = frame.arg2() as *mut u32;

    if size < 0 {
        return Err(SyscallError::EINVAL);
    }
    if size == 0 {
        return Ok(1);
    }
    if !is_user_ptr_valid(list_ptr as u64, core::mem::size_of::<u32>()) {
        return Err(SyscallError::EFAULT);
    }

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    let gid = proc.creds.gid;
    drop(proc);

    // SAFETY: Validated user memory pointer bounds.
    unsafe {
        core::ptr::write_volatile(list_ptr, gid);
    }
    Ok(1)
}

/// Linux 64-bit resource limit structure.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct RLimit64 {
    pub rlim_cur: u64,
    pub rlim_max: u64,
}

const RLIM_INFINITY: u64 = !0u64;

fn get_default_rlimit(resource: i32) -> RLimit64 {
    match resource {
        3 /* RLIMIT_STACK */ => RLimit64 {
            rlim_cur: 8 * 1024 * 1024,
            rlim_max: 64 * 1024 * 1024,
        },
        7 /* RLIMIT_NOFILE */ => RLimit64 {
            rlim_cur: 1024,
            rlim_max: 4096,
        },
        6 /* RLIMIT_NPROC */ => RLimit64 {
            rlim_cur: 4096,
            rlim_max: 4096,
        },
        _ => RLimit64 {
            rlim_cur: RLIM_INFINITY,
            rlim_max: RLIM_INFINITY,
        },
    }
}

/// `sys_getrlimit` (SYS_GETRLIMIT = 97)
/// Get resource limits.
pub fn sys_getrlimit(frame: &mut SyscallFrame) -> SyscallResult {
    let resource = frame.arg1() as i32;
    let rlim_ptr = frame.arg2() as *mut RLimit64;

    if !is_user_ptr_valid(rlim_ptr as u64, core::mem::size_of::<RLimit64>()) {
        return Err(SyscallError::EFAULT);
    }

    let limit = get_default_rlimit(resource);
    // SAFETY: Validated user memory pointer bounds.
    unsafe {
        core::ptr::write_volatile(rlim_ptr, limit);
    }
    Ok(0)
}

/// `sys_setrlimit` (SYS_SETRLIMIT = 160)
/// Set resource limits.
pub fn sys_setrlimit(frame: &mut SyscallFrame) -> SyscallResult {
    let _resource = frame.arg1() as i32;
    let rlim_ptr = frame.arg2() as *const RLimit64;

    if !is_user_ptr_valid(rlim_ptr as u64, core::mem::size_of::<RLimit64>()) {
        return Err(SyscallError::EFAULT);
    }
    Ok(0)
}

/// `sys_prlimit64` (SYS_PRLIMIT64 = 302)
/// Get/set resource limits of an arbitrary process.
pub fn sys_prlimit64(frame: &mut SyscallFrame) -> SyscallResult {
    let _pid = frame.arg1() as i32;
    let resource = frame.arg2() as i32;
    let new_limit_ptr = frame.arg3() as *const RLimit64;
    let old_limit_ptr = frame.arg4() as *mut RLimit64;

    if !new_limit_ptr.is_null()
        && !is_user_ptr_valid(new_limit_ptr as u64, core::mem::size_of::<RLimit64>())
    {
        return Err(SyscallError::EFAULT);
    }

    if !old_limit_ptr.is_null() {
        if !is_user_ptr_valid(old_limit_ptr as u64, core::mem::size_of::<RLimit64>()) {
            return Err(SyscallError::EFAULT);
        }
        let limit = get_default_rlimit(resource);
        // SAFETY: Validated user memory pointer bounds.
        unsafe {
            core::ptr::write_volatile(old_limit_ptr, limit);
        }
    }

    Ok(0)
}

/// `sys_fork` (SYS_FORK = 57)
/// Fork the current running process and thread context.
pub fn sys_fork(frame: &mut SyscallFrame) -> SyscallResult {
    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let child_arc =
        crate::proc::Process::fork(proc_arc, frame).map_err(|_| SyscallError::EAGAIN)?;
    let child_pid = child_arc.lock().pid.as_u64();
    Ok(child_pid as usize)
}

/// `sys_vfork` (SYS_VFORK = 58)
/// Create a child process and block parent until exec/exit.
pub fn sys_vfork(frame: &mut SyscallFrame) -> SyscallResult {
    sys_fork(frame)
}

/// `sys_execve` (SYS_EXECVE = 59)
/// Execute program file.
pub fn sys_execve(frame: &mut SyscallFrame) -> SyscallResult {
    let path_ptr = frame.arg1() as *const u8;
    let argv_ptr = frame.arg2() as *const *const u8;
    let envp_ptr = frame.arg3() as *const *const u8;

    let path = unsafe { read_user_string(path_ptr, 256)? };

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let mut proc = proc_arc.lock();

    let (entry_point, stack_top) = proc
        .execute(&path, 0, argv_ptr, envp_ptr)
        .map_err(|_| SyscallError::ENOENT)?;

    let new_cr3 = proc.address_space.lock().page_table().root().as_u64();

    // SAFETY: Switching CPU page directory to the newly executed program's address space.
    unsafe {
        crate::arch::set_address_space_root(new_cr3);
    }

    if let Some(thread_arc) = crate::proc::current_thread() {
        let mut t = thread_arc.lock();
        t.context.cr3 = new_cr3 as usize;
        t.context.fs_base = 0;
    }
    crate::arch::cpu::msr::write_fs_base(0);

    frame.rip = entry_point;
    frame.rsp = stack_top;

    Ok(0)
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct RUsage {
    pub ru_utime: crate::syscalls::time::TimeVal,
    pub ru_stime: crate::syscalls::time::TimeVal,
    pub ru_maxrss: i64,
    pub ru_ixrss: i64,
    pub ru_idrss: i64,
    pub ru_isrss: i64,
    pub ru_minflt: i64,
    pub ru_majflt: i64,
    pub ru_nswap: i64,
    pub ru_inblock: i64,
    pub ru_oublock: i64,
    pub ru_msgsnd: i64,
    pub ru_msgrcv: i64,
    pub ru_nsignals: i64,
    pub ru_nvcsw: i64,
    pub ru_nivcsw: i64,
}

/// `sys_wait4` (SYS_WAIT4 = 61)
/// Wait for process state change.
pub fn sys_wait4(frame: &mut SyscallFrame) -> SyscallResult {
    let pid_raw = frame.arg1() as i32;
    let wstatus = frame.arg2() as *mut i32;
    let options = frame.arg3() as i32;
    let rusage_ptr = frame.arg4() as *mut RUsage;

    let wnohang = (options & 1) != 0;
    let wuntraced = (options & 2) != 0;

    let (child_pid, status) = loop {
        let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
        let mut proc = proc_arc.lock();

        match proc.try_wait4(pid_raw, wuntraced)? {
            Some(res) => {
                drop(proc);
                break res;
            }
            None => {
                drop(proc);
                if wnohang {
                    break (crate::proc::ProcessId(0), 0);
                }
                crate::proc::thread::Thread::yield_cpu();
            }
        }
    };

    if !wstatus.is_null() && is_user_ptr_valid(wstatus as u64, core::mem::size_of::<i32>()) {
        // SAFETY: User pointer validated within Ring 3 address bounds.
        unsafe {
            core::ptr::write_volatile(wstatus, status);
        }
    }

    if !rusage_ptr.is_null() && is_user_ptr_valid(rusage_ptr as u64, core::mem::size_of::<RUsage>())
    {
        // SAFETY: User pointer validated within Ring 3 address bounds.
        unsafe {
            core::ptr::write_volatile(rusage_ptr, RUsage::default());
        }
    }

    Ok(child_pid.as_u64() as usize)
}

/// `sys_exit` (SYS_EXIT = 60)
/// Terminate the calling thread or process.
pub fn sys_exit(frame: &mut SyscallFrame) -> SyscallResult {
    let code = frame.arg1() as i32;
    log::info!("sys_exit called with status code {}", code);
    do_exit(code)
}

/// `sys_exit_group` (SYS_EXIT_GROUP = 231)
/// Exit all threads in a process.
pub fn sys_exit_group(frame: &mut SyscallFrame) -> SyscallResult {
    let code = frame.arg1() as i32;
    log::info!("sys_exit_group called with status code {}", code);
    do_exit(code)
}

/// Common exit path shared by `sys_exit` and `sys_exit_group`.
///
/// This function never returns: it marks the current thread and process as zombie,
/// then yields the CPU via `schedule(false)`. If no other runnable thread exists,
/// it falls into the idle loop. Either path prevents `iretq` from firing into a
/// dead user-space context.
fn do_exit(code: i32) -> ! {
    let ppid_opt = if let Some(proc_arc) = crate::proc::current_process() {
        let mut proc = proc_arc.lock();
        proc.exit(code);
        proc.ppid
    } else {
        crate::proc::ProcessId(0)
    };

    if let Some(thread_arc) = crate::proc::current_thread() {
        let mut t = thread_arc.lock();
        t.state = crate::proc::ThreadState::Zombie;
        t.exit_code = Some(code as u32);
    }

    if ppid_opt.as_u64() > 0 {
        if let Some(parent_arc) = crate::proc::find_process(ppid_opt) {
            let mut parent = parent_arc.lock();
            let _ = parent.send_signal(crate::ipc::signal::SIGCHLD);
        }
    }

    loop {
        crate::sched::schedule(false);
    }
}
