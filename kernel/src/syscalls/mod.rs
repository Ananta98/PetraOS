pub mod process;

use crate::arch::SyscallFrame;

pub fn handle_syscall(frame: &mut SyscallFrame) {
    let sys_num = frame.syscall_num();
    let arg1 = frame.arg1();
    let arg2 = frame.arg2();

    match sys_num {
        1 => { // SYS_EXIT
            let exit_code = arg1 as i32;
            log::info!("Syscall: exit({}) called", exit_code);
            process::exit::sys_exit(exit_code);
        }
        2 => { // SYS_FORK
            log::info!("Syscall: fork() called");
            frame.set_return_value(process::fork::sys_fork());
        }
        3 => { // SYS_EXEC
            let elf_ptr = arg1;
            let elf_size = arg2;
            log::info!("Syscall: exec(ptr={:#x}, size={}) called", elf_ptr, elf_size);
            match process::exec::sys_exec(frame, elf_ptr, elf_size) {
                Ok(_) => {
                    log::info!("Syscall: exec successful, returning to new entry point");
                }
                Err(err) => {
                    log::error!("Syscall: exec failed: {}", err);
                    frame.set_return_value(u64::MAX);
                }
            }
        }
        5 => { // SYS_WAITPID
            let pid = arg1 as i64;
            log::info!("Syscall: waitpid(pid={}) called", pid);
            frame.set_return_value(process::waitpid::sys_waitpid(pid));
        }
        _ => {
            log::warn!("Syscall: unknown syscall number {}", sys_num);
            frame.set_return_value(u64::MAX);
        }
    }
}
