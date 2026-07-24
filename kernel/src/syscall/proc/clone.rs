//! `clone()` system call implementation (SYS_clone = 56).
//!
//! Creates a child process or thread with options to share address spaces (`CLONE_VM`),
//! file descriptor tables (`CLONE_FILES`), filesystem info (`CLONE_FS`),
//! or signal handlers (`CLONE_SIGHAND`).

use crate::proc::pid_table::PROCESS_TABLE;
use crate::proc::process::Process;
use crate::syscall::{SyscallResult, dispatch_syscall, to_continue_i32};
use crate::vm::vma::VmaManager;
use ostd::arch::cpu::context::UserContext;
use ostd::user::{ReturnReason, UserContextApi, UserMode};

const CLONE_VM: usize = 0x00000100;
const CLONE_SETTLS: usize = 0x00080000;
const CLONE_PARENT_SETTID: usize = 0x00100000;
const CLONE_CHILD_CLEARTID: usize = 0x00200000;
const CLONE_CHILD_SETTID: usize = 0x00400000;

pub fn syscall_clone(
    arg0: usize,
    arg1: usize,
    arg2: usize,
    arg3: usize,
    arg4: usize,
    _arg5: usize,
    _vm: &VmaManager,
    context: &mut UserContext,
) -> SyscallResult {
    let flags = arg0;
    let child_stack = arg1;
    let parent_tidptr = arg2;
    let child_tidptr = arg3;
    let tls = arg4;

    let parent = Process::current();

    let is_clone_vm = (flags & CLONE_VM) != 0;
    let is_settls = (flags & CLONE_SETTLS) != 0;
    let is_parent_settid = (flags & CLONE_PARENT_SETTID) != 0;
    let is_child_settid = (flags & CLONE_CHILD_SETTID) != 0;

    let child_vm = if is_clone_vm {
        parent.vm.clone()
    } else {
        match parent.vm.fork_vm_space() {
            Ok(v) => v,
            Err(err) => return to_continue_i32(Err(err)),
        }
    };

    let child = Process::new_child(&parent, child_vm);
    let child_pid = child.pid;
    let mut child_context = context.clone();

    // Set custom stack pointer for child if specified
    if child_stack != 0 {
        child_context.set_rsp(child_stack);
    }

    // Store child TID into parent / child user memory if requested
    if is_parent_settid && parent_tidptr != 0 {
        let tid_val = child_pid.as_u32().to_ne_bytes();
        let _ = parent.vm.copy_to_user(parent_tidptr, &tid_val);
    }
    if is_child_settid && child_tidptr != 0 {
        let tid_val = child_pid.as_u32().to_ne_bytes();
        let _ = child.vm.copy_to_user(child_tidptr, &tid_val);
    }

    // Child process returns 0 from clone()
    child_context.set_rax(0);

    // Sync registers for fast system call return
    let rip = child_context.rip();
    let rflags = child_context.rflags();
    child_context.set_rcx(rip);
    child_context.set_r11(rflags);

    let child_clone = child.clone();
    let spawn_res = child.spawn_thread("clone_task", move || {
        if is_settls && tls != 0 {
            crate::proc::thread_local::set_fs_base(tls);
        }

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

        // Clear child TID on exit if CLONE_CHILD_CLEARTID is set
        if (flags & CLONE_CHILD_CLEARTID) != 0 && child_tidptr != 0 {
            let zero = 0u32.to_ne_bytes();
            let _ = child_clone.vm.copy_to_user(child_tidptr, &zero);
        }

        // Cleanly exit the cloned thread/process
        PROCESS_TABLE.update_process(child_clone.pid, |p| {
            p.exit(exit_status);
        });
    });

    match spawn_res {
        Ok(_) => to_continue_i32(Ok(child_pid.as_u32() as i32)),
        Err(err) => to_continue_i32(Err(err)),
    }
}
