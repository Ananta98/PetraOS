//! sys_shmctl system call handler.

use super::*;
use crate::ipc::shm::{SHM_MANAGER, ShmInfo, ShmidDs};
use crate::syscalls::{wrap_syscall, SyscallError, SyscallResult, UserPtr};

#[wrap_syscall]
pub fn sys_shmctl(shmid: i32, cmd: i32, buf_ptr: u64) -> SyscallResult {
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
