use crate::proc::pid_table::PROCESS_TABLE;
use crate::proc::process::Process;
use crate::syscall::{SyscallResult, to_continue, to_continue_i32};
use crate::vm::vma::VmaManager;
use ostd::Error;
use ostd::arch::cpu::context::UserContext;

/// `getuid()` — returns the real user ID of the calling process (SYS_getuid = 102).
pub fn syscall_getuid(
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    to_continue(Ok(Process::current().credentials.uid() as usize))
}

/// `geteuid()` — returns the effective user ID of the calling process (SYS_geteuid = 107).
pub fn syscall_geteuid(
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    to_continue(Ok(Process::current().credentials.euid() as usize))
}

/// `getgid()` — returns the real group ID of the calling process (SYS_getgid = 104).
pub fn syscall_getgid(
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    to_continue(Ok(Process::current().credentials.gid() as usize))
}

/// `getegid()` — returns the effective group ID of the calling process (SYS_getegid = 108).
pub fn syscall_getegid(
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    to_continue(Ok(Process::current().credentials.egid() as usize))
}

/// `setuid()` — sets the effective user ID of the calling process (SYS_setuid = 105).
pub fn syscall_setuid(
    arg0: usize, // uid
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    let uid = arg0 as u32;
    let current = Process::current();

    let mut result = Err(Error::AccessDenied);

    PROCESS_TABLE.update_process(current.pid, |p| {
        if p.credentials.euid() == 0 {
            alloc::sync::Arc::make_mut(&mut p.credentials).set_uid(uid);
            alloc::sync::Arc::make_mut(&mut p.credentials).set_euid(uid);
            alloc::sync::Arc::make_mut(&mut p.credentials).set_suid(uid);
            alloc::sync::Arc::make_mut(&mut p.credentials).set_fsuid(uid);
            result = Ok(0);
        } else if uid == p.credentials.uid() || uid == p.credentials.suid() {
            alloc::sync::Arc::make_mut(&mut p.credentials).set_euid(uid);
            alloc::sync::Arc::make_mut(&mut p.credentials).set_fsuid(uid);
            result = Ok(0);
        }
    });

    to_continue_i32(result)
}

/// `setgid()` — sets the effective group ID of the calling process (SYS_setgid = 106).
pub fn syscall_setgid(
    arg0: usize, // gid
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    let gid = arg0 as u32;
    let current = Process::current();

    let mut result = Err(Error::AccessDenied);

    PROCESS_TABLE.update_process(current.pid, |p| {
        if p.credentials.euid() == 0 {
            alloc::sync::Arc::make_mut(&mut p.credentials).set_gid(gid);
            alloc::sync::Arc::make_mut(&mut p.credentials).set_egid(gid);
            alloc::sync::Arc::make_mut(&mut p.credentials).set_sgid(gid);
            alloc::sync::Arc::make_mut(&mut p.credentials).set_fsgid(gid);
            result = Ok(0);
        } else if gid == p.credentials.gid() || gid == p.credentials.sgid() {
            alloc::sync::Arc::make_mut(&mut p.credentials).set_egid(gid);
            alloc::sync::Arc::make_mut(&mut p.credentials).set_fsgid(gid);
            result = Ok(0);
        }
    });

    to_continue_i32(result)
}

/// `setreuid()` — sets real and/or effective user ID (SYS_setreuid = 113).
pub fn syscall_setreuid(
    arg0: usize, // ruid
    arg1: usize, // euid
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    let ruid = arg0 as i32;
    let euid = arg1 as i32;
    let current = Process::current();

    let mut result = Err(Error::AccessDenied);

    PROCESS_TABLE.update_process(current.pid, |p| {
        let old_ruid = p.credentials.uid();
        let old_euid = p.credentials.euid();

        let target_ruid = if ruid != -1 { ruid as u32 } else { old_ruid };
        let target_euid = if euid != -1 { euid as u32 } else { old_euid };

        let is_ruid_changed = ruid != -1;
        let is_euid_changed = euid != -1;

        if p.credentials.euid() == 0 {
            alloc::sync::Arc::make_mut(&mut p.credentials).set_uid(target_ruid);
            alloc::sync::Arc::make_mut(&mut p.credentials).set_euid(target_euid);
            alloc::sync::Arc::make_mut(&mut p.credentials).set_fsuid(target_euid);
            if is_ruid_changed || (is_euid_changed && target_euid != old_ruid) {
                alloc::sync::Arc::make_mut(&mut p.credentials).set_suid(target_euid);
            }
            result = Ok(0);
        } else {
            let ruid_ok = !is_ruid_changed || (target_ruid == old_ruid || target_ruid == old_euid);
            let euid_ok = !is_euid_changed
                || (target_euid == old_ruid
                    || target_euid == old_euid
                    || target_euid == p.credentials.suid());

            if ruid_ok && euid_ok {
                alloc::sync::Arc::make_mut(&mut p.credentials).set_uid(target_ruid);
                alloc::sync::Arc::make_mut(&mut p.credentials).set_euid(target_euid);
                alloc::sync::Arc::make_mut(&mut p.credentials).set_fsuid(target_euid);
                if is_ruid_changed || (is_euid_changed && target_euid != old_ruid) {
                    alloc::sync::Arc::make_mut(&mut p.credentials).set_suid(target_euid);
                }
                result = Ok(0);
            }
        }
    });

    to_continue_i32(result)
}

/// `setregid()` — sets real and/or effective group ID (SYS_setregid = 114).
pub fn syscall_setregid(
    arg0: usize, // rgid
    arg1: usize, // egid
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    let rgid = arg0 as i32;
    let egid = arg1 as i32;
    let current = Process::current();

    let mut result = Err(Error::AccessDenied);

    PROCESS_TABLE.update_process(current.pid, |p| {
        let old_rgid = p.credentials.gid();
        let old_egid = p.credentials.egid();

        let target_rgid = if rgid != -1 { rgid as u32 } else { old_rgid };
        let target_egid = if egid != -1 { egid as u32 } else { old_egid };

        let is_rgid_changed = rgid != -1;
        let is_egid_changed = egid != -1;

        if p.credentials.euid() == 0 {
            alloc::sync::Arc::make_mut(&mut p.credentials).set_gid(target_rgid);
            alloc::sync::Arc::make_mut(&mut p.credentials).set_egid(target_egid);
            alloc::sync::Arc::make_mut(&mut p.credentials).set_fsgid(target_egid);
            if is_rgid_changed || (is_egid_changed && target_egid != old_rgid) {
                alloc::sync::Arc::make_mut(&mut p.credentials).set_sgid(target_egid);
            }
            result = Ok(0);
        } else {
            let rgid_ok = !is_rgid_changed || (target_rgid == old_rgid || target_rgid == old_egid);
            let egid_ok = !is_egid_changed
                || (target_egid == old_rgid
                    || target_egid == old_egid
                    || target_egid == p.credentials.sgid());

            if rgid_ok && egid_ok {
                alloc::sync::Arc::make_mut(&mut p.credentials).set_gid(target_rgid);
                alloc::sync::Arc::make_mut(&mut p.credentials).set_egid(target_egid);
                alloc::sync::Arc::make_mut(&mut p.credentials).set_fsgid(target_egid);
                if is_rgid_changed || (is_egid_changed && target_egid != old_rgid) {
                    alloc::sync::Arc::make_mut(&mut p.credentials).set_sgid(target_egid);
                }
                result = Ok(0);
            }
        }
    });

    to_continue_i32(result)
}

/// `setresuid()` — sets real, effective, and saved user ID (SYS_setresuid = 117).
pub fn syscall_setresuid(
    arg0: usize, // ruid
    arg1: usize, // euid
    arg2: usize, // suid
    _: usize,
    _: usize,
    _: usize,
    _: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    let ruid = arg0 as i32;
    let euid = arg1 as i32;
    let suid = arg2 as i32;
    let current = Process::current();

    let mut result = Err(Error::AccessDenied);

    PROCESS_TABLE.update_process(current.pid, |p| {
        let old_ruid = p.credentials.uid();
        let old_euid = p.credentials.euid();
        let old_suid = p.credentials.suid();

        let target_ruid = if ruid != -1 { ruid as u32 } else { old_ruid };
        let target_euid = if euid != -1 { euid as u32 } else { old_euid };
        let target_suid = if suid != -1 { suid as u32 } else { old_suid };

        if p.credentials.euid() == 0 {
            alloc::sync::Arc::make_mut(&mut p.credentials).set_uid(target_ruid);
            alloc::sync::Arc::make_mut(&mut p.credentials).set_euid(target_euid);
            alloc::sync::Arc::make_mut(&mut p.credentials).set_suid(target_suid);
            alloc::sync::Arc::make_mut(&mut p.credentials).set_fsuid(target_euid);
            result = Ok(0);
        } else {
            let matches_any =
                |val: u32| -> bool { val == old_ruid || val == old_euid || val == old_suid };

            if matches_any(target_ruid) && matches_any(target_euid) && matches_any(target_suid) {
                alloc::sync::Arc::make_mut(&mut p.credentials).set_uid(target_ruid);
                alloc::sync::Arc::make_mut(&mut p.credentials).set_euid(target_euid);
                alloc::sync::Arc::make_mut(&mut p.credentials).set_suid(target_suid);
                alloc::sync::Arc::make_mut(&mut p.credentials).set_fsuid(target_euid);
                result = Ok(0);
            }
        }
    });

    to_continue_i32(result)
}

/// `setresgid()` — sets real, effective, and saved group ID (SYS_setresgid = 119).
pub fn syscall_setresgid(
    arg0: usize, // rgid
    arg1: usize, // egid
    arg2: usize, // sgid
    _: usize,
    _: usize,
    _: usize,
    _: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    let rgid = arg0 as i32;
    let egid = arg1 as i32;
    let sgid = arg2 as i32;
    let current = Process::current();

    let mut result = Err(Error::AccessDenied);

    PROCESS_TABLE.update_process(current.pid, |p| {
        let old_rgid = p.credentials.gid();
        let old_egid = p.credentials.egid();
        let old_sgid = p.credentials.sgid();

        let target_rgid = if rgid != -1 { rgid as u32 } else { old_rgid };
        let target_egid = if egid != -1 { egid as u32 } else { old_egid };
        let target_sgid = if sgid != -1 { sgid as u32 } else { old_sgid };

        if p.credentials.euid() == 0 {
            alloc::sync::Arc::make_mut(&mut p.credentials).set_gid(target_rgid);
            alloc::sync::Arc::make_mut(&mut p.credentials).set_egid(target_egid);
            alloc::sync::Arc::make_mut(&mut p.credentials).set_sgid(target_sgid);
            alloc::sync::Arc::make_mut(&mut p.credentials).set_fsgid(target_egid);
            result = Ok(0);
        } else {
            let matches_any =
                |val: u32| -> bool { val == old_rgid || val == old_egid || val == old_sgid };

            if matches_any(target_rgid) && matches_any(target_egid) && matches_any(target_sgid) {
                alloc::sync::Arc::make_mut(&mut p.credentials).set_gid(target_rgid);
                alloc::sync::Arc::make_mut(&mut p.credentials).set_egid(target_egid);
                alloc::sync::Arc::make_mut(&mut p.credentials).set_sgid(target_sgid);
                alloc::sync::Arc::make_mut(&mut p.credentials).set_fsgid(target_egid);
                result = Ok(0);
            }
        }
    });

    to_continue_i32(result)
}

/// `getresuid()` — returns real, effective, and saved user ID (SYS_getresuid = 118).
pub fn syscall_getresuid(
    arg0: usize, // ruid_ptr
    arg1: usize, // euid_ptr
    arg2: usize, // suid_ptr
    _: usize,
    _: usize,
    _: usize,
    vm: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    let ruid_ptr = arg0;
    let euid_ptr = arg1;
    let suid_ptr = arg2;
    let current = Process::current();

    if ruid_ptr != 0 {
        let val = current.credentials.uid();
        if let Err(err) = vm.copy_to_user(ruid_ptr, &val.to_ne_bytes()) {
            return to_continue_i32(Err(err));
        }
    }
    if euid_ptr != 0 {
        let val = current.credentials.euid();
        if let Err(err) = vm.copy_to_user(euid_ptr, &val.to_ne_bytes()) {
            return to_continue_i32(Err(err));
        }
    }
    if suid_ptr != 0 {
        let val = current.credentials.suid();
        if let Err(err) = vm.copy_to_user(suid_ptr, &val.to_ne_bytes()) {
            return to_continue_i32(Err(err));
        }
    }

    to_continue_i32(Ok(0))
}

/// `getresgid()` — returns real, effective, and saved group ID (SYS_getresgid = 120).
pub fn syscall_getresgid(
    arg0: usize, // rgid_ptr
    arg1: usize, // egid_ptr
    arg2: usize, // sgid_ptr
    _: usize,
    _: usize,
    _: usize,
    vm: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    let rgid_ptr = arg0;
    let egid_ptr = arg1;
    let sgid_ptr = arg2;
    let current = Process::current();

    if rgid_ptr != 0 {
        let val = current.credentials.gid();
        if let Err(err) = vm.copy_to_user(rgid_ptr, &val.to_ne_bytes()) {
            return to_continue_i32(Err(err));
        }
    }
    if egid_ptr != 0 {
        let val = current.credentials.egid();
        if let Err(err) = vm.copy_to_user(egid_ptr, &val.to_ne_bytes()) {
            return to_continue_i32(Err(err));
        }
    }
    if sgid_ptr != 0 {
        let val = current.credentials.sgid();
        if let Err(err) = vm.copy_to_user(sgid_ptr, &val.to_ne_bytes()) {
            return to_continue_i32(Err(err));
        }
    }

    to_continue_i32(Ok(0))
}

/// `setfsuid()` — sets the user ID used for filesystem checks (SYS_setfsuid = 122).
pub fn syscall_setfsuid(
    arg0: usize, // fsuid
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    let fsuid = arg0 as u32;
    let current = Process::current();
    let mut prev_fsuid = current.credentials.fsuid();

    PROCESS_TABLE.update_process(current.pid, |p| {
        prev_fsuid = p.credentials.fsuid();
        if p.credentials.euid() == 0
            || fsuid == p.credentials.uid()
            || fsuid == p.credentials.euid()
            || fsuid == p.credentials.suid()
            || fsuid == p.credentials.fsuid()
        {
            alloc::sync::Arc::make_mut(&mut p.credentials).set_fsuid(fsuid);
        }
    });

    to_continue(Ok(prev_fsuid as usize))
}

/// `setfsgid()` — sets the group ID used for filesystem checks (SYS_setfsgid = 123).
pub fn syscall_setfsgid(
    arg0: usize, // fsgid
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    let fsgid = arg0 as u32;
    let current = Process::current();
    let mut prev_fsgid = current.credentials.fsgid();

    PROCESS_TABLE.update_process(current.pid, |p| {
        prev_fsgid = p.credentials.fsgid();
        if p.credentials.euid() == 0
            || fsgid == p.credentials.gid()
            || fsgid == p.credentials.egid()
            || fsgid == p.credentials.sgid()
            || fsgid == p.credentials.fsgid()
        {
            alloc::sync::Arc::make_mut(&mut p.credentials).set_fsgid(fsgid);
        }
    });

    to_continue(Ok(prev_fsgid as usize))
}
