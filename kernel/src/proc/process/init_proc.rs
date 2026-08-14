use crate::arch::cpu::stack::KernelStack;
use crate::arch::userspace::jump_to_userspace;
use crate::mm::vmm::paging::PageTable;
use crate::proc::process::cmdline::CommandLine;
use crate::proc::process::pid::ProcessId;
use crate::proc::process::process::Process;
use crate::sync::spinlock::Spinlock;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec;

/// POSIX standard paths scanned in order to find the initial user-space init binary.
pub const DEFAULT_INIT_EXEC_PATHS: &[&str] = &[
    "/sbin/init",
    "/etc/init",
    "/bin/init",
    "/bin/sh",
];

/// Initialize the primary user process (PID 1).
///
/// Scans `DEFAULT_INIT_EXEC_PATHS` in order per POSIX specifications.
pub fn create_init_process() -> Result<(Arc<Spinlock<Process>>, u64, u64), &'static str> {
    log::info!(
        "[Init Process] Searching for POSIX init binary in DEFAULT_INIT_EXEC_PATHS: {:?}",
        DEFAULT_INIT_EXEC_PATHS
    );

    let mut proc = Process::new(ProcessId(1), ProcessId(0))?;

    // Default environment variables for user space initialization
    let default_env = vec![
        String::from("PATH=/bin:/sbin:/usr/bin:/usr/sbin:/usr/local/bin:/usr/local/sbin"),
        String::from("TERM=petraos"),
        String::from("HOME=/"),
        String::from("USER=root"),
    ];

    // 1. Iterate over candidate init paths and execute
    for candidate_path in DEFAULT_INIT_EXEC_PATHS {
        log::info!("[Init Process] Checking candidate path: '{}'", candidate_path);

        let cmdline = CommandLine::new(
            vec![String::from(*candidate_path)],
            default_env.clone(),
        );

        if let Ok((entry_point, stack_top)) = proc.execute_cmdline(candidate_path, cmdline) {
            log::info!(
                "✔ [Init Process] Successfully resolved and loaded init binary from candidate path: '{}'",
                candidate_path
            );
            let proc_arc = Arc::new(Spinlock::new(proc));
            super::process_table::register_process(proc_arc.clone());
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
    drop(p_lock);

    // Allocate a dynamic 16-byte aligned kernel stack for TSS RSP0 and Ring 0 transition
    let kernel_stack = KernelStack::new(16 * 1024);
    let kernel_rsp0 = kernel_stack.top();

    // Jump to user mode (Ring 3) with valid kernel stack top for TSS RSP0
    unsafe {
        jump_to_userspace(entry_point, stack_top, kernel_rsp0, cr3);
    }
}
