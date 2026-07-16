use crate::proc::pid_table::PROCESS_TABLE;
use crate::proc::process::Process;
use crate::syscall::{SyscallResult, to_continue, to_continue_i32};
use crate::vm::vma::VmaManager;
use ostd::Error;
use ostd::arch::cpu::context::UserContext;

/// `getuid()` — returns the real user ID of the calling process (SYS_getuid = 102).
pub(crate) fn syscall_getuid(
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    to_continue(Ok(Process::current().uid as usize))
}

/// `geteuid()` — returns the effective user ID of the calling process (SYS_geteuid = 107).
pub(crate) fn syscall_geteuid(
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    to_continue(Ok(Process::current().euid as usize))
}

/// `getgid()` — returns the real group ID of the calling process (SYS_getgid = 104).
pub(crate) fn syscall_getgid(
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    to_continue(Ok(Process::current().gid as usize))
}

/// `getegid()` — returns the effective group ID of the calling process (SYS_getegid = 108).
pub(crate) fn syscall_getegid(
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    to_continue(Ok(Process::current().egid as usize))
}

/// `setuid()` — sets the effective user ID of the calling process (SYS_setuid = 105).
pub(crate) fn syscall_setuid(
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
        if p.euid == 0 {
            p.uid = uid;
            p.euid = uid;
            p.suid = uid;
            p.fsuid = uid;
            result = Ok(0);
        } else if uid == p.uid || uid == p.suid {
            p.euid = uid;
            p.fsuid = uid;
            result = Ok(0);
        }
    });

    to_continue_i32(result)
}

/// `setgid()` — sets the effective group ID of the calling process (SYS_setgid = 106).
pub(crate) fn syscall_setgid(
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
        if p.euid == 0 {
            p.gid = gid;
            p.egid = gid;
            p.sgid = gid;
            p.fsgid = gid;
            result = Ok(0);
        } else if gid == p.gid || gid == p.sgid {
            p.egid = gid;
            p.fsgid = gid;
            result = Ok(0);
        }
    });

    to_continue_i32(result)
}

/// `setreuid()` — sets real and/or effective user ID (SYS_setreuid = 113).
pub(crate) fn syscall_setreuid(
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
        let old_ruid = p.uid;
        let old_euid = p.euid;

        let target_ruid = if ruid != -1 { ruid as u32 } else { old_ruid };
        let target_euid = if euid != -1 { euid as u32 } else { old_euid };

        let is_ruid_changed = ruid != -1;
        let is_euid_changed = euid != -1;

        if p.euid == 0 {
            p.uid = target_ruid;
            p.euid = target_euid;
            p.fsuid = target_euid;
            if is_ruid_changed || (is_euid_changed && target_euid != old_ruid) {
                p.suid = target_euid;
            }
            result = Ok(0);
        } else {
            let ruid_ok = !is_ruid_changed || (target_ruid == old_ruid || target_ruid == old_euid);
            let euid_ok = !is_euid_changed
                || (target_euid == old_ruid || target_euid == old_euid || target_euid == p.suid);

            if ruid_ok && euid_ok {
                p.uid = target_ruid;
                p.euid = target_euid;
                p.fsuid = target_euid;
                if is_ruid_changed || (is_euid_changed && target_euid != old_ruid) {
                    p.suid = target_euid;
                }
                result = Ok(0);
            }
        }
    });

    to_continue_i32(result)
}

/// `setregid()` — sets real and/or effective group ID (SYS_setregid = 114).
pub(crate) fn syscall_setregid(
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
        let old_rgid = p.gid;
        let old_egid = p.egid;

        let target_rgid = if rgid != -1 { rgid as u32 } else { old_rgid };
        let target_egid = if egid != -1 { egid as u32 } else { old_egid };

        let is_rgid_changed = rgid != -1;
        let is_egid_changed = egid != -1;

        if p.euid == 0 {
            p.gid = target_rgid;
            p.egid = target_egid;
            p.fsgid = target_egid;
            if is_rgid_changed || (is_egid_changed && target_egid != old_rgid) {
                p.sgid = target_egid;
            }
            result = Ok(0);
        } else {
            let rgid_ok = !is_rgid_changed || (target_rgid == old_rgid || target_rgid == old_egid);
            let egid_ok = !is_egid_changed
                || (target_egid == old_rgid || target_egid == old_egid || target_egid == p.sgid);

            if rgid_ok && egid_ok {
                p.gid = target_rgid;
                p.egid = target_egid;
                p.fsgid = target_egid;
                if is_rgid_changed || (is_egid_changed && target_egid != old_rgid) {
                    p.sgid = target_egid;
                }
                result = Ok(0);
            }
        }
    });

    to_continue_i32(result)
}

/// `setresuid()` — sets real, effective, and saved user ID (SYS_setresuid = 117).
pub(crate) fn syscall_setresuid(
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
        let old_ruid = p.uid;
        let old_euid = p.euid;
        let old_suid = p.suid;

        let target_ruid = if ruid != -1 { ruid as u32 } else { old_ruid };
        let target_euid = if euid != -1 { euid as u32 } else { old_euid };
        let target_suid = if suid != -1 { suid as u32 } else { old_suid };

        if p.euid == 0 {
            p.uid = target_ruid;
            p.euid = target_euid;
            p.suid = target_suid;
            p.fsuid = target_euid;
            result = Ok(0);
        } else {
            let matches_any =
                |val: u32| -> bool { val == old_ruid || val == old_euid || val == old_suid };

            if matches_any(target_ruid) && matches_any(target_euid) && matches_any(target_suid) {
                p.uid = target_ruid;
                p.euid = target_euid;
                p.suid = target_suid;
                p.fsuid = target_euid;
                result = Ok(0);
            }
        }
    });

    to_continue_i32(result)
}

/// `setresgid()` — sets real, effective, and saved group ID (SYS_setresgid = 119).
pub(crate) fn syscall_setresgid(
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
        let old_rgid = p.gid;
        let old_egid = p.egid;
        let old_sgid = p.sgid;

        let target_rgid = if rgid != -1 { rgid as u32 } else { old_rgid };
        let target_egid = if egid != -1 { egid as u32 } else { old_egid };
        let target_sgid = if sgid != -1 { sgid as u32 } else { old_sgid };

        if p.euid == 0 {
            p.gid = target_rgid;
            p.egid = target_egid;
            p.sgid = target_sgid;
            p.fsgid = target_egid;
            result = Ok(0);
        } else {
            let matches_any =
                |val: u32| -> bool { val == old_rgid || val == old_egid || val == old_sgid };

            if matches_any(target_rgid) && matches_any(target_egid) && matches_any(target_sgid) {
                p.gid = target_rgid;
                p.egid = target_egid;
                p.sgid = target_sgid;
                p.fsgid = target_egid;
                result = Ok(0);
            }
        }
    });

    to_continue_i32(result)
}

/// `getresuid()` — returns real, effective, and saved user ID (SYS_getresuid = 118).
pub(crate) fn syscall_getresuid(
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
        let val = current.uid;
        if let Err(err) = vm.copy_to_user(ruid_ptr, &val.to_ne_bytes()) {
            return to_continue_i32(Err(err));
        }
    }
    if euid_ptr != 0 {
        let val = current.euid;
        if let Err(err) = vm.copy_to_user(euid_ptr, &val.to_ne_bytes()) {
            return to_continue_i32(Err(err));
        }
    }
    if suid_ptr != 0 {
        let val = current.suid;
        if let Err(err) = vm.copy_to_user(suid_ptr, &val.to_ne_bytes()) {
            return to_continue_i32(Err(err));
        }
    }

    to_continue_i32(Ok(0))
}

/// `getresgid()` — returns real, effective, and saved group ID (SYS_getresgid = 120).
pub(crate) fn syscall_getresgid(
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
        let val = current.gid;
        if let Err(err) = vm.copy_to_user(rgid_ptr, &val.to_ne_bytes()) {
            return to_continue_i32(Err(err));
        }
    }
    if egid_ptr != 0 {
        let val = current.egid;
        if let Err(err) = vm.copy_to_user(egid_ptr, &val.to_ne_bytes()) {
            return to_continue_i32(Err(err));
        }
    }
    if sgid_ptr != 0 {
        let val = current.sgid;
        if let Err(err) = vm.copy_to_user(sgid_ptr, &val.to_ne_bytes()) {
            return to_continue_i32(Err(err));
        }
    }

    to_continue_i32(Ok(0))
}

/// `setfsuid()` — sets the user ID used for filesystem checks (SYS_setfsuid = 122).
pub(crate) fn syscall_setfsuid(
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
    let mut prev_fsuid = current.fsuid;

    PROCESS_TABLE.update_process(current.pid, |p| {
        prev_fsuid = p.fsuid;
        if p.euid == 0 || fsuid == p.uid || fsuid == p.euid || fsuid == p.suid || fsuid == p.fsuid {
            p.fsuid = fsuid;
        }
    });

    to_continue(Ok(prev_fsuid as usize))
}

/// `setfsgid()` — sets the group ID used for filesystem checks (SYS_setfsgid = 123).
pub(crate) fn syscall_setfsgid(
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
    let mut prev_fsgid = current.fsgid;

    PROCESS_TABLE.update_process(current.pid, |p| {
        prev_fsgid = p.fsgid;
        if p.euid == 0 || fsgid == p.gid || fsgid == p.egid || fsgid == p.sgid || fsgid == p.fsgid {
            p.fsgid = fsgid;
        }
    });

    to_continue(Ok(prev_fsgid as usize))
}
