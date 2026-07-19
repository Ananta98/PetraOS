use crate::proc::process::Process;
use crate::proc::thread::KernelThread;
use crate::proc::tls::{allocate_tls_block, get_fs_base, set_fs_base};
use crate::syscall::{SyscallResult, dispatch_syscall};
use crate::vm::vma::VmaManager;
use alloc::vec::Vec;
use core::sync::atomic::Ordering;
use ostd::Error;
use ostd::arch::cpu::context::UserContext;
use ostd::mm::PageFlags;
use ostd::user::{ReturnReason, UserContextApi, UserMode};

/// Layout and setup the user space stack for a process.
///
/// Builds stack according to the System V AMD64 ABI layout:
/// 1. Information block (null-terminated strings for argv and envp)
/// 2. Alignment padding (for 16-byte stack alignment)
/// 3. Auxiliary Vector (simplified: AT_ENTRY, AT_PAGESZ, AT_NULL)
/// 4. Environment pointers (ending in NULL)
/// 5. Argument pointers (ending in NULL)
/// 6. argc (count of arguments)
pub fn setup_user_stack(
    vm: &VmaManager,
    argv: &[&str],
    envp: &[&str],
    entry: usize,
) -> Result<usize, Error> {
    const USER_STACK_SIZE: usize = 65536; // 64KB stack
    const USER_STACK_TOP: usize = 0x7FFF_FFFF_0000;
    const USER_STACK_BOTTOM: usize = USER_STACK_TOP - USER_STACK_SIZE;

    // Map stack region with readable and writable permissions for user
    vm.map_region(USER_STACK_BOTTOM, USER_STACK_SIZE, PageFlags::RW)?;

    // We build the stack contents locally and copy it to user space in one transfer
    let mut stack_buf = alloc::vec![0u8; USER_STACK_SIZE];
    let mut str_pos = USER_STACK_SIZE;

    // Helper to push a null-terminated string to the information block
    let mut push_str = |s: &str| -> usize {
        let bytes = s.as_bytes();
        let len = bytes.len() + 1;
        str_pos -= len;
        stack_buf[str_pos..str_pos + bytes.len()].copy_from_slice(bytes);
        stack_buf[str_pos + bytes.len()] = 0;
        USER_STACK_BOTTOM + str_pos
    };

    // Push env strings
    let mut env_addrs = Vec::new();
    for env in envp.iter().rev() {
        env_addrs.push(push_str(env));
    }
    env_addrs.reverse();

    // Push arg strings
    let mut arg_addrs = Vec::new();
    for arg in argv.iter().rev() {
        arg_addrs.push(push_str(arg));
    }
    arg_addrs.reverse();

    // Align string position down to 8 bytes for following pointers
    str_pos &= !7;

    // Define simplified Auxiliary Vector: AT_ENTRY, AT_PAGESZ, AT_NULL
    let auxv = [
        (9, entry), // AT_ENTRY = 9
        (6, 4096),  // AT_PAGESZ = 6
        (0, 0),     // AT_NULL = 0
    ];

    // Calculate total number of usize values to write
    let n_usizes = 1 + (argv.len() + 1) + (envp.len() + 1) + (auxv.len() * 2);

    // Compute the start position for writing pointers such that the final stack pointer (rsp)
    // is aligned to a 16-byte boundary (System V ABI requirement).
    let mut val_pos = str_pos - n_usizes * 8;
    val_pos &= !15;

    let mut current_val_pos = val_pos;
    let mut write_usize = |val: usize| {
        let bytes = val.to_le_bytes();
        stack_buf[current_val_pos..current_val_pos + 8].copy_from_slice(&bytes);
        current_val_pos += 8;
    };

    // 1. Write argc
    write_usize(argv.len());

    // 2. Write argv pointers followed by NULL
    for addr in arg_addrs {
        write_usize(addr);
    }
    write_usize(0);

    // 3. Write envp pointers followed by NULL
    for addr in env_addrs {
        write_usize(addr);
    }
    write_usize(0);

    // 4. Write Auxiliary Vector
    for &(key, val) in &auxv {
        write_usize(key);
        write_usize(val);
    }

    assert_eq!(current_val_pos, val_pos + n_usizes * 8);

    // Copy to user space memory
    vm.copy_to_user(USER_STACK_BOTTOM, &stack_buf)?;

    // Return the initial user stack pointer (rsp)
    Ok(USER_STACK_BOTTOM + val_pos)
}

/// Set up the TLS block for the current thread and write its FS base
/// into the per-thread `KernelThread::tls_fs_base`.
///
/// Must be called from the thread that will execute user mode (i.e.,
/// from inside the task closure, after `vm.activate()`).
pub fn setup_tls_for_current_thread(
    vm: &crate::vm::vma::VmaManager,
    process: &Process,
) -> Result<(), Error> {
    if let Some(thread) = KernelThread::current() {
        let tp = allocate_tls_block(vm, &process.tls_template)?;
        thread.tls_fs_base.store(tp, Ordering::Release);
        if tp != 0 {
            set_fs_base(tp);
        }
    }
    Ok(())
}

/// Transition a process to User Mode and run its execution loop.
///
/// Sets up the initial register state, loads the instruction/stack pointers,
/// activates the process's page table, allocates a TLS block, sets the FS
/// segment base, and executes in user space.
pub fn run_process_user_mode(
    process: &mut Process,
    entry_point: usize,
    stack_ptr: usize,
) -> Result<i32, Error> {
    // 1. Initialize UserContext (Trap Frame representation in OSTD)
    let mut context = UserContext::default();
    context.set_instruction_pointer(entry_point);
    context.set_stack_pointer(stack_ptr);

    // Set interrupts and ID flags for user mode
    context.set_rflags(0x202);

    // 2. Initialize UserMode runner
    let mut user_mode = UserMode::new(context);

    // 3. Activate process virtual memory space
    process.vm.activate();

    // 4. Allocate TLS block and set FS base for this thread
    setup_tls_for_current_thread(&process.vm, process)?;

    // 5. Execution Loop
    let mut exit_status = -1;
    loop {
        // Restore FS base in case the task migrated CPUs since the last
        // iteration.  If the FS base changed via arch_prctl, `KernelThread`
        // already holds the updated value.
        if let Some(thread) = KernelThread::current() {
            let tp = thread.tls_fs_base.load(Ordering::Acquire);
            if tp != 0 {
                set_fs_base(tp);
            }
        }

        let reason = user_mode.execute(|| false);

        // Save the FS base after user execution so that any changes made
        // by the user (via arch_prctl(ARCH_SET_FS, …)) are preserved.
        if let Some(thread) = KernelThread::current() {
            let tp = get_fs_base();
            thread.tls_fs_base.store(tp, Ordering::Release);
        }
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

                // Dispatch system call
                match dispatch_syscall(
                    num,
                    arg0,
                    arg1,
                    arg2,
                    arg3,
                    arg4,
                    arg5,
                    &process.vm,
                    &mut ctx,
                ) {
                    SyscallResult::Continue(retval) => {
                        let ctx = user_mode.context_mut();
                        ctx.set_rax(retval);

                        // Sync rcx and r11 for fast sysret
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
                let ctx = user_mode.context_mut();
                let trap = ctx.trap_number();
                let err = ctx.trap_error_code();
                log::error!(
                    "Unhandled User Exception: trap_num={}, error_code={}",
                    trap,
                    err
                );
                break;
            }
            ReturnReason::KernelEvent => {
                ostd::task::Task::yield_now();
            }
        }
    }

    // Set process status to Zombie and record exit code
    process.exit(exit_status);
    Ok(exit_status)
}
