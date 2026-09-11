use crate::arch::cpu::stack::KernelStack;
use crate::arch::userspace::jump_to_userspace;
use crate::bootcmd::BootCommandLine;
use crate::mm::vmm::paging::PageTable;
use crate::proc::process::cmdline::CommandLine;
use crate::proc::process::pid::ProcessId;
use crate::proc::process::process::Process;
use crate::sync::Mutex;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec;

/// POSIX boot order: real init first, interactive shell fallbacks.
/// `/sbin/init` is the PetraOS init script (login-prompt loop); the bash
/// entries keep the system bootable when init is absent or broken.
pub const DEFAULT_INIT_EXEC_PATHS: &[&str] =
    &["/sbin/init", "/bin/bash", "/usr/bin/bash", "/bin/sh"];

/// Initialize the primary user process (PID 1).
///
/// Environment variables and boot arguments are loaded dynamically from Limine's boot command line.
/// Scans `DEFAULT_INIT_EXEC_PATHS` per POSIX specifications.
pub fn create_init_process() -> Result<(Arc<Mutex<Process>>, u64, u64), &'static str> {
    log::info!(
        "[Init Process] Searching for POSIX init binary in DEFAULT_INIT_EXEC_PATHS: {:?}",
        DEFAULT_INIT_EXEC_PATHS
    );

    let init_pid = super::pid::next_pid();
    let mut proc = Process::new(init_pid, ProcessId(0))?;

    // Set up standard file descriptors (0 = stdin, 1 = stdout, 2 = stderr) pointing to /dev/console
    if let Ok(console_file) = crate::fs::open_file("/dev/console", crate::fs::O_RDWR) {
        proc.fd_table.setup_std_fds(console_file);
    }

    // Retrieve environment variables and boot arguments from BootCommandLine
    let boot_cmdline = BootCommandLine::new();
    let init_env = boot_cmdline.to_envs_vec();
    let boot_args = boot_cmdline.to_args_vec();

    // 1. Iterate over candidate init paths and execute
    for candidate_path in DEFAULT_INIT_EXEC_PATHS {
        log::debug!(
            "[Init Process] Checking candidate path: '{}'",
            candidate_path
        );

        let prog_name = candidate_path.rsplit('/').next().unwrap_or(candidate_path);
        let mut args = vec![String::from(prog_name)];
        args.extend(boot_args.clone());
        let cmdline = CommandLine::new(args, init_env.clone());

        if let Ok((entry_point, stack_top)) = proc.execute_cmdline(candidate_path, cmdline) {
            log::debug!(
                "✔ [Init Process] Successfully resolved and loaded init binary from candidate path: '{}'",
                candidate_path
            );
            let proc_arc = Arc::new(Mutex::new(proc));

            // Create and attach the primary thread for the init process
            let init_tid = crate::proc::thread::next_tid();
            let init_thread = Arc::new(Mutex::new(crate::proc::thread::Thread::new(
                init_tid,
                String::from("init"),
                1024,
                Arc::downgrade(&proc_arc),
            )));

            let root_page_table = proc_arc
                .lock()
                .address_space
                .lock()
                .page_table()
                .root()
                .as_u64() as usize;

            let kernel_stack = KernelStack::new()
                .map_err(|_| "Failed to allocate kernel stack for init process")?;

            let mut t_lock = init_thread.lock();
            t_lock.context.cr3 = root_page_table;
            t_lock.state = crate::proc::thread::ThreadState::Running;
            t_lock.set_kernel_stack(kernel_stack);
            drop(t_lock);

            proc_arc
                .lock()
                .threads
                .insert(init_tid, init_thread.clone());
            super::process_table::register_process(proc_arc.clone());

            // Register as active thread on BSP CPU 0
            crate::sched::set_current_thread_on_cpu(0, Some(init_thread));

            return Ok((proc_arc, entry_point, stack_top));
        }
    }

    log::error!(
        "[Init Process] No candidate init binary found in VFS paths: {:?}",
        DEFAULT_INIT_EXEC_PATHS
    );
    Err("Failed to find or execute init binary in DEFAULT_INIT_EXEC_PATHS")
}

/// Execute process initialization and jump to userspace.
pub fn run_init_process() -> ! {
    let (proc_arc, entry_point, stack_top) = match create_init_process() {
        Ok(res) => res,
        Err(err) => panic!("Failed to create init process: {}", err),
    };

    let p_lock = proc_arc.lock();
    let cr3 = p_lock.address_space.lock().page_table().root().as_u64();
    let kernel_rsp0 = p_lock
        .threads
        .values()
        .next()
        .map(|t| t.lock().kernel_stack_top())
        .unwrap_or(0);
    drop(p_lock);

    // Jump to user mode (Ring 3) with valid kernel stack top for TSS RSP0
    unsafe {
        jump_to_userspace(entry_point, stack_top, kernel_rsp0, cr3);
    }
}
