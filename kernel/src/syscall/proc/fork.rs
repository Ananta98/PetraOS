/// `fork()` — create a child process (SYS_fork = 57).
///
/// Clones the calling process's virtual memory space with Copy-on-Write
/// semantics and spawns a new thread in the child process starting from the
/// same register context.
///
/// Returns the child's PID to the parent, and `0` to the child.
use crate::proc::pid_table::PROCESS_TABLE;
use crate::proc::process::Process;
use crate::syscall::{SyscallResult, dispatch_syscall, to_continue_i32};
use crate::vm::vma::VmaManager;
use ostd::Error;
use ostd::arch::cpu::context::UserContext;
use ostd::user::{ReturnReason, UserContextApi, UserMode};

pub(crate) fn syscall_fork(
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: &VmaManager,
    context: &mut UserContext,
) -> SyscallResult {
    let parent = Process::current();

    // Fork the process state (Cow vm cloning + Process table registration)
    let child = match parent.fork() {
        Ok(c) => c,
        Err(err) => return to_continue_i32(Err(err)),
    };

    let child_pid = child.pid;
    let mut child_context = context.clone();

    // The child process returns 0 from fork()
    child_context.set_rax(0);

    // Sync registers for fast system call return
    let rip = child_context.rip();
    let rflags = child_context.rflags();
    child_context.set_rcx(rip);
    child_context.set_r11(rflags);

    let child_clone = child.clone();
    let spawn_res = child.spawn_thread("main", move || {
        let mut user_mode = UserMode::new(child_context);
        child_clone.vm.activate();

        let mut exit_status = 0;
        loop {
            let reason = user_mode.execute(|| false);
            match reason {
                ReturnReason::UserSyscall => {
                    let mut ctx = user_mode.context_mut();
                    let num = ctx.rax();
                    let arg0 = ctx.rdi();
                    let arg1 = ctx.rsi();
                    let arg2 = ctx.rdx();
                    let arg3 = ctx.r10();
                    let arg4 = ctx.r8();
                    let arg5 = ctx.r9();

                    match dispatch_syscall(
                        num,
                        arg0,
                        arg1,
                        arg2,
                        arg3,
                        arg4,
                        arg5,
                        &child_clone.vm,
                        &mut ctx,
                    ) {
                        SyscallResult::Continue(retval) => {
                            let mut ctx = user_mode.context_mut();
                            ctx.set_rax(retval);
                            let rip = ctx.rip();
                            let rflags = ctx.rflags();
                            ctx.set_rcx(rip);
                            ctx.set_r11(rflags);
                        }
                        SyscallResult::Exit(status) => {
                            exit_status = status;
                            break;
                        }
                    }
                }
                ReturnReason::UserException => {
                    exit_status = -1;
                    break;
                }
                ReturnReason::KernelEvent => {
                    ostd::task::Task::yield_now();
                }
            }
        }

        // Cleanly exit the child process
        PROCESS_TABLE.update_process(child_clone.pid, |p| {
            p.exit(exit_status);
        });
    });

    match spawn_res {
        Ok(_) => to_continue_i32(Ok(child_pid.as_u32() as i32)),
        Err(err) => to_continue_i32(Err(err)),
    }
}
