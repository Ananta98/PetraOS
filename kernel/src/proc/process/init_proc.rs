use crate::arch::cpu::stack::KernelStack;
use crate::arch::userspace::{DEFAULT_USER_PAYLOAD, jump_to_userspace};
use crate::mm::vmm::paging::PageTable;
use crate::proc::process::pid::ProcessId;
use crate::proc::process::process::Process;
use crate::sync::spinlock::Spinlock;
use alloc::sync::Arc;

/// POSIX standard paths scanned in order to find the initial user-space init binary.
pub const DEFAULT_INIT_EXEC_PATHS: &[&str] = &[
    "/sbin/init",
    "/etc/init",
    "/bin/init",
    "/bin/sh",
];

/// Setup initial POSIX init binaries in VFS for execution path verification testing.
pub fn setup_test_init_files() -> Result<(), &'static str> {
    log::info!("[Init Process] Setting up test POSIX init binaries in VFS...");
    let _ = crate::fs::mkdir("/sbin");
    let _ = crate::fs::mkdir("/bin");

    if let Ok(dentry) = crate::fs::create_file("/sbin/init") {
        if let Ok(file_ops) = dentry.inode.ops.open() {
            let payload = DEFAULT_USER_PAYLOAD;
            let _ = file_ops.write(0, payload);
            log::info!(
                "[Init Process] Created POSIX init binary at /sbin/init (size: {} bytes)",
                payload.len()
            );
        }
    }
    Ok(())
}

/// Initialize the primary user process (PID 1).
///
/// Scans `DEFAULT_INIT_EXEC_PATHS` in order per POSIX specifications.
pub fn create_init_process() -> Result<(Arc<Spinlock<Process>>, u64, u64), &'static str> {
    log::info!(
        "[Init Process] Searching for POSIX init binary in DEFAULT_INIT_EXEC_PATHS: {:?}",
        DEFAULT_INIT_EXEC_PATHS
    );

    let mut proc = Process::new(ProcessId(1), ProcessId(0))?;

    // 1. Iterate over candidate init paths and execute
    for candidate_path in DEFAULT_INIT_EXEC_PATHS {
        log::info!("[Init Process] Testing candidate path: '{}'", candidate_path);
        if let Ok((entry_point, stack_top)) =
            proc.execute(candidate_path, 0, core::ptr::null(), core::ptr::null())
        {
            log::info!(
                "✔ [Init Process] Successfully resolved and loaded init binary from candidate path: '{}'",
                candidate_path
            );
            return Ok((Arc::new(Spinlock::new(proc)), entry_point, stack_top));
        }
    }

    // 2. Fallback if no candidate file found in VFS: map built-in payload
    log::warn!(
        "[Init Process] No candidate init binary found in VFS paths. Falling back to built-in payload."
    );

    let (entry_point, stack_top) = proc.load_builtin_payload()?;
    Ok((Arc::new(Spinlock::new(proc)), entry_point, stack_top))
}

/// Execute process initialization and jump to userspace.
pub fn run_init_process() -> ! {
    let _ = setup_test_init_files();

    let (proc_arc, entry_point, stack_top) = match create_init_process() {
        Ok(res) => res,
        Err(err) => panic!("Failed to create init process: {}", err),
    };

    let p_lock = proc_arc.lock();
    let cr3 = p_lock.address_space.page_table().root().as_u64();
    drop(p_lock);

    // Allocate a dynamic 16-byte aligned kernel stack for TSS RSP0 and Ring 0 transition
    let kernel_stack = KernelStack::new(16 * 1024);
    let kernel_rsp0 = kernel_stack.top();

    // Jump to user mode (Ring 3) with valid kernel stack top for TSS RSP0
    unsafe {
        jump_to_userspace(entry_point, stack_top, kernel_rsp0, cr3);
    }
}
