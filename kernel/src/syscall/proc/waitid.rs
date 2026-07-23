use crate::proc::pid_table::PROCESS_TABLE;
use crate::proc::pid_table::Pid;
use crate::proc::process::{Process, ProcessState};
use crate::syscall::{SyscallResult, to_continue_i32};
use crate::vm::vma::VmaManager;
use ostd::Error;
use ostd::arch::cpu::context::UserContext;

/// `waitid(idtype, id, infop, options, rusage)` — wait for process to change state (SYS_waitid = 247).
pub fn syscall_waitid(
    arg0: usize, // idtype_t idtype
    arg1: usize, // id_t id
    arg2: usize, // siginfo_t *infop
    arg3: usize, // int options
    _: usize,    // struct rusage *rusage (ignored)
    _: usize,
    vm: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    let idtype = arg0;
    let id = arg1 as u32;
    let infop = arg2;
    let options = arg3;

    // Validate options: must specify at least one of WEXITED, WSTOPPED, WCONTINUED
    if (options & (4 | 2 | 8)) == 0 {
        return to_continue_i32(Err(Error::InvalidArgs));
    }

    let current_process = Process::current();

    loop {
        // Check if there are any children. If none, return ECHILD.
        let has_children = {
            let children = current_process.children.lock();
            !children.is_empty()
        };

        if !has_children {
            return to_continue_i32(Err(Error::InvalidArgs)); // ECHILD
        }

        let mut matching_child_exists = false;
        let mut zombie_child = None;
        let mut zombie_index = None;

        let mut children = current_process.children.lock();
        for (i, &child_pid) in children.iter().enumerate() {
            if let Some(child) = PROCESS_TABLE.get_process(child_pid) {
                let matches = match idtype {
                    0 => true,                        // P_ALL
                    1 => child_pid.as_u32() == id,    // P_PID
                    2 => child.pgid().as_u32() == id, // P_PGID
                    _ => return to_continue_i32(Err(Error::InvalidArgs)),
                };

                if matches {
                    matching_child_exists = true;
                    if child.state == ProcessState::Zombie {
                        zombie_child = Some(child);
                        zombie_index = Some(i);
                        break;
                    }
                }
            }
        }

        if !matching_child_exists {
            return to_continue_i32(Err(Error::InvalidArgs)); // ECHILD
        }

        if let Some(child) = zombie_child {
            let child_pid = child.pid;
            let exit_code = child.exit_code;
            let child_uid = child.credentials.uid();

            // If WNOWAIT is not set, reap the child
            if (options & 0x01000000) == 0 {
                if let Some(idx) = zombie_index {
                    children.remove(idx);
                }
                // Drop children lock before calling unregister to avoid nested locks
                core::mem::drop(children);
                PROCESS_TABLE.unregister_process(child_pid);
            } else {
                core::mem::drop(children);
            }

            if infop != 0 {
                // siginfo_t layout on x86_64:
                // signo (offset 0), errno (offset 4), code (offset 8),
                // padding (offset 12), pid (offset 16), uid (offset 20), status (offset 24)
                let mut siginfo_bytes = [0u8; 128];
                let signo = 17i32; // SIGCHLD
                let errno = 0i32;
                let code = 1i32; // CLD_EXITED

                siginfo_bytes[0..4].copy_from_slice(&signo.to_ne_bytes());
                siginfo_bytes[4..8].copy_from_slice(&errno.to_ne_bytes());
                siginfo_bytes[8..12].copy_from_slice(&code.to_ne_bytes());
                siginfo_bytes[16..20].copy_from_slice(&(child_pid.as_u32() as i32).to_ne_bytes());
                siginfo_bytes[20..24].copy_from_slice(&(child_uid as i32).to_ne_bytes());
                siginfo_bytes[24..28].copy_from_slice(&exit_code.to_ne_bytes());

                if let Err(err) = vm.copy_to_user(infop, &siginfo_bytes) {
                    return to_continue_i32(Err(err));
                }
            }

            return to_continue_i32(Ok(0));
        }

        // If WNOHANG is set, return 0 immediately with si_signo set to 0.
        if (options & 1) != 0 {
            core::mem::drop(children);
            if infop != 0 {
                let zero = 0i32;
                if let Err(err) = vm.copy_to_user(infop, &zero.to_ne_bytes()) {
                    return to_continue_i32(Err(err));
                }
            }
            return to_continue_i32(Ok(0));
        }

        // Drop lock before yielding
        core::mem::drop(children);
        core::hint::spin_loop();
    }
}
