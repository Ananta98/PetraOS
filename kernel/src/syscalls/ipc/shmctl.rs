//! sys_shmctl system call handler.

use super::*;
use crate::syscalls::{SyscallError, SyscallResult, UserPtr};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::arch::timer::hpet;
use crate::ipc::semaphore::SemaphoreManager;
use crate::ipc::semaphore::{
    GETALL, GETNCNT, GETPID, GETVAL, GETZCNT, IPC_RMID, IPC_SET, IPC_STAT,
    SEMAPHORE_MANAGER, SETALL, SETVAL, SemBuf, SemError, SemidDs, SemopResult,
};
use crate::ipc::shm::{SHM_MANAGER, ShmError, ShmidDs, ShmInfo};
use crate::proc::thread::ThreadState;


pub fn sys_shmctl(frame: &mut SyscallFrame) -> SyscallResult {
    let shmid = frame.arg1() as i32;
    let cmd = frame.arg2() as i32;
    let buf_ptr = frame.arg3();

    let (uid, gid) = current_uid_gid();
    let cmd_stripped = cmd & !0x100; // Strip IPC_64 flag if present

    match cmd_stripped {
        crate::ipc::shm::IPC_RMID => {
            let mut mgr = SHM_MANAGER.lock();
            mgr.shmctl(shmid, crate::ipc::shm::IPC_RMID, None, None, None, uid, gid)?;
            Ok(0)
        }
        crate::ipc::shm::IPC_STAT | crate::ipc::shm::SHM_STAT => {
            let ds_uptr = UserPtr::<ShmidDs>::from_u64(buf_ptr);
            if !ds_uptr.is_valid() {
                return Err(SyscallError::EFAULT);
            }
            let mut ds = ShmidDs::default();
            let res = {
                let mut mgr = SHM_MANAGER.lock();
                mgr.shmctl(shmid, cmd_stripped, Some(&mut ds), None, None, uid, gid)?
            };
            ds_uptr.write(ds).ok_or(SyscallError::EFAULT)?;
            Ok(res as usize)
        }
        crate::ipc::shm::IPC_SET => {
            let ds_uptr = UserPtr::<ShmidDs>::from_u64(buf_ptr);
            if !ds_uptr.is_valid() {
                return Err(SyscallError::EFAULT);
            }
            let ds = ds_uptr.read().ok_or(SyscallError::EFAULT)?;
            let mut mgr = SHM_MANAGER.lock();
            mgr.shmctl(shmid, crate::ipc::shm::IPC_SET, None, Some(&ds), None, uid, gid)?;
            Ok(0)
        }
        crate::ipc::shm::IPC_INFO | crate::ipc::shm::SHM_INFO => {
            let info_uptr = UserPtr::<ShmInfo>::from_u64(buf_ptr);
            if !info_uptr.is_valid() {
                return Err(SyscallError::EFAULT);
            }
            let mut info = ShmInfo::default();
            let res = {
                let mut mgr = SHM_MANAGER.lock();
                mgr.shmctl(shmid, cmd_stripped, None, None, Some(&mut info), uid, gid)?
            };
            info_uptr.write(info).ok_or(SyscallError::EFAULT)?;
            Ok(res as usize)
        }
        crate::ipc::shm::SHM_LOCK | crate::ipc::shm::SHM_UNLOCK => {
            let mut mgr = SHM_MANAGER.lock();
            mgr.shmctl(shmid, cmd_stripped, None, None, None, uid, gid)?;
            Ok(0)
        }
        _ => Err(SyscallError::EINVAL),
    }
}
