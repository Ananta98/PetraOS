//! System calls for CPU affinity inspection and assignment.

use super::types::{resolve_target_thread, resolve_target_threads};
use crate::arch::syscall::SyscallFrame;
use crate::syscalls::{SyscallError, SyscallResult, UserPtr};

/// `sys_sched_getaffinity` (SYS_SCHED_GETAFFINITY = 204)
///
/// Retrieves the CPU affinity mask of a process / thread.
pub fn sys_sched_getaffinity(frame: &mut SyscallFrame) -> SyscallResult {
    let pid = frame.arg1() as i32;
    let cpusetsize = frame.arg2() as usize;
    let mask_ptr = UserPtr::<u8>::from_u64(frame.arg3());

    if pid < 0 {
        return Err(SyscallError::EINVAL);
    }
    if cpusetsize < core::mem::size_of::<u64>() {
        return Err(SyscallError::EINVAL);
    }
    if !mask_ptr.is_valid_for(cpusetsize) {
        return Err(SyscallError::EFAULT);
    }

    let target = resolve_target_thread(pid)?;
    let affinity = target.lock().affinity;

    // Zero out user buffer
    for i in 0..cpusetsize {
        mask_ptr.add(i).write(0).ok_or(SyscallError::EFAULT)?;
    }

    // Write low 64-bit affinity mask
    let bytes = affinity.to_ne_bytes();
    for (i, byte) in bytes.iter().enumerate() {
        mask_ptr.add(i).write(*byte).ok_or(SyscallError::EFAULT)?;
    }

    // Return size of the kernel cpumask in bytes (8 bytes for up to 64 CPUs)
    Ok(core::mem::size_of::<u64>())
}

/// `sys_sched_setaffinity` (SYS_SCHED_SETAFFINITY = 203)
///
/// Sets the CPU affinity mask of a process / thread.
pub fn sys_sched_setaffinity(frame: &mut SyscallFrame) -> SyscallResult {
    let pid = frame.arg1() as i32;
    let cpusetsize = frame.arg2() as usize;
    let mask_ptr = UserPtr::<u8>::from_u64(frame.arg3());

    if pid < 0 || cpusetsize == 0 {
        return Err(SyscallError::EINVAL);
    }
    if !mask_ptr.is_valid_for(cpusetsize) {
        return Err(SyscallError::EFAULT);
    }

    // Read mask up to 64 bits
    let read_len = core::cmp::min(cpusetsize, core::mem::size_of::<u64>());
    let mut bytes = [0u8; 8];
    for i in 0..read_len {
        bytes[i] = mask_ptr.add(i).read().ok_or(SyscallError::EFAULT)?;
    }
    let user_mask = u64::from_ne_bytes(bytes);

    // Validate that the mask includes at least one active / online CPU
    let cpu_count = crate::arch::cpu_count();
    let online_mask = if cpu_count >= 64 {
        !0u64
    } else {
        (1u64 << cpu_count) - 1
    };

    if (user_mask & online_mask) == 0 {
        return Err(SyscallError::EINVAL);
    }

    let targets = resolve_target_threads(pid)?;
    for thread in targets {
        thread.lock().affinity = user_mask;
    }

    Ok(0)
}
