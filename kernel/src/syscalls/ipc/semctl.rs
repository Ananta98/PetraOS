//! sys_semctl system call handler.

use super::*;
use crate::syscalls::{SyscallError, SyscallResult, UserPtr};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::ipc::semaphore::{
    GETALL, GETNCNT, GETPID, GETVAL, GETZCNT, IPC_RMID, IPC_SET, IPC_STAT,
    SEMAPHORE_MANAGER, SETALL, SETVAL, SemidDs,
};


pub fn sys_semctl(frame: &mut SyscallFrame) -> SyscallResult {
    let semid = frame.arg1() as i32;
    let semnum = frame.arg2() as i32;
    let cmd = frame.arg3() as i32;
    let arg_raw: SemunRaw = frame.arg4();

    let (uid, _gid) = current_uid_gid();

    // Strip IPC_64 flag (value 0 on x86_64 – no-op)
    let cmd_stripped = cmd & !0x100;

    match cmd_stripped {
        // Commands that don't use union semun argument
        IPC_RMID => {
            let mut mgr = SEMAPHORE_MANAGER.lock();
            let result = mgr.semctl(semid, semnum, IPC_RMID, None, None, None, None, uid)?;
            Ok(result as usize)
        }

        GETVAL | GETPID | GETNCNT | GETZCNT => {
            let mut mgr = SEMAPHORE_MANAGER.lock();
            let result = mgr.semctl(semid, semnum, cmd_stripped, None, None, None, None, uid)?;
            Ok(result as usize)
        }

        SETVAL => {
            // arg_raw is an int value (not a pointer) on x86_64
            let val = arg_raw as i32;
            let mut mgr = SEMAPHORE_MANAGER.lock();
            let result = mgr.semctl(semid, semnum, SETVAL, Some(val), None, None, None, uid)?;
            Ok(result as usize)
        }

        IPC_STAT => {
            // arg_raw is a pointer to `struct semid_ds`
            let ds_uptr = UserPtr::<SemidDs>::from_u64(arg_raw);
            if !ds_uptr.is_valid() {
                return Err(SyscallError::EFAULT);
            }
            let mut ds = SemidDs::default();
            {
                let mut mgr = SEMAPHORE_MANAGER.lock();
                mgr.semctl(
                    semid,
                    semnum,
                    IPC_STAT,
                    None,
                    Some(&mut ds),
                    None,
                    None,
                    uid,
                )?;
            }
            // SAFETY: pointer is validated via UserPtr::is_valid()
            unsafe {
                let raw = ds_uptr.as_ptr() as *mut SemidDs;
                core::ptr::write_volatile(raw, ds);
            }
            Ok(0)
        }

        IPC_SET => {
            let ds_uptr = UserPtr::<SemidDs>::from_u64(arg_raw);
            if !ds_uptr.is_valid() {
                return Err(SyscallError::EFAULT);
            }
            let mut ds = ds_uptr.read().ok_or(SyscallError::EFAULT)?;
            let mut mgr = SEMAPHORE_MANAGER.lock();
            mgr.semctl(semid, semnum, IPC_SET, None, Some(&mut ds), None, None, uid)?;
            Ok(0)
        }

        GETALL => {
            // arg_raw is pointer to u16 array
            let set_nsems = {
                let mgr = SEMAPHORE_MANAGER.lock();
                mgr.sets.get(&semid).ok_or(SyscallError::ENOENT)?.nsems()
            };
            let mut buf: alloc::vec::Vec<u16> = alloc::vec![0u16; set_nsems];
            {
                let mut mgr = SEMAPHORE_MANAGER.lock();
                mgr.semctl(semid, semnum, GETALL, None, None, None, Some(&mut buf), uid)?;
            }
            // Write the array back to userspace
            for (i, &v) in buf.iter().enumerate() {
                let elem_addr = arg_raw
                    .checked_add((i * 2) as u64)
                    .ok_or(SyscallError::EFAULT)?;
                let uptr = UserPtr::<u16>::from_u64(elem_addr);
                if !uptr.is_valid() {
                    return Err(SyscallError::EFAULT);
                }
                // SAFETY: validated via UserPtr::is_valid()
                unsafe {
                    core::ptr::write_volatile(uptr.as_ptr() as *mut u16, v);
                }
            }
            Ok(0)
        }

        SETALL => {
            let set_nsems = {
                let mgr = SEMAPHORE_MANAGER.lock();
                mgr.sets.get(&semid).ok_or(SyscallError::ENOENT)?.nsems()
            };
            let mut buf: alloc::vec::Vec<u16> = alloc::vec![0u16; set_nsems];
            for (i, slot) in buf.iter_mut().enumerate() {
                let elem_addr = arg_raw
                    .checked_add((i * 2) as u64)
                    .ok_or(SyscallError::EFAULT)?;
                let uptr = UserPtr::<u16>::from_u64(elem_addr);
                if !uptr.is_valid() {
                    return Err(SyscallError::EFAULT);
                }
                *slot = uptr.read().ok_or(SyscallError::EFAULT)?;
            }
            let mut mgr = SEMAPHORE_MANAGER.lock();
            mgr.semctl(semid, semnum, SETALL, None, None, Some(&buf), None, uid)?;
            Ok(0)
        }

        _ => Err(SyscallError::EINVAL),
    }
}
