pub mod elf;
pub mod pid_table;
pub mod process;
pub mod thread;
pub mod tid_table;
pub mod user;

// Re-export the most commonly used thread types so that other modules can
// write `crate::proc::KernelThread` without the full submodule path.
pub use thread::{KernelThread, spawn_kernel_thread};
pub use tid_table::{THREAD_TABLE, Tid};

use crate::proc::elf::LoadedElf;
use crate::vm::VMA_MANAGER;
use crate::vm::vma::VmaManager;
use alloc::sync::Arc;
use ostd::Error;
use process::Process;

/// Spawn the **init** process (PID 1).
///
/// Mirrors the logic in Linux `kernel_init()` and Asterinas
/// `spawn_init_process()`:
///
/// 1. If a custom path is provided (future: from `init=` on the kernel
///    command line), use it.
/// 2. Otherwise probe the canonical fallback list in order:
///    `/sbin/init` → `/etc/init` → `/bin/init` → `/bin/sh`.
///
/// Each path is resolved through the VFS, read into memory, and loaded as
/// an ELF executable.  The first path that resolves and loads successfully
/// becomes PID 1.
///
/// # Panics
/// Panics if `vm::init()` has not been called before this function, or if
/// none of the probed paths can be loaded.
pub fn spawn_init_process() {
    const DEFAULT_INIT_EXEC_PATHS: &[&str] = &["/sbin/init"];

    let vm = VMA_MANAGER
        .get()
        .expect("vm::init() must be called before spawning init")
        .clone();

    for &path in DEFAULT_INIT_EXEC_PATHS {
        let executable_name = path.rfind('/').map_or(path, |i| &path[i + 1..]);
        if let Ok((process, loaded)) = try_load_init_exec(vm.clone(), path, executable_name) {
            let entry = loaded.entry;

            // Set up the user-space stack (argv, envp, auxv per System V ABI).
            let stack_ptr = crate::proc::user::setup_user_stack(&process.vm, &[path], &[], entry)
                .expect("failed to setup user stack");

            // Spawn the main thread.  Its body activates the process VM
            // and enters user mode, executing the init program.
            let mut process_for_thread = process.clone();
            process
                .spawn_thread("main", move || {
                    let _ = crate::proc::user::run_process_user_mode(
                        &mut process_for_thread,
                        entry,
                        stack_ptr,
                    );
                })
                .expect("failed to spawn init thread");

            return;
        }
    }
}

/// Try to load `path` as an ELF executable and exec it into a new init process.
///
/// Reads the file from the VFS, creates a fresh `Process`, and replaces its
/// address space with the loaded ELF image.  Returns `Ok((process, loaded))`
/// on success, or `Err` if the path could not be resolved, read, or loaded.
fn try_load_init_exec(
    vm: Arc<VmaManager>,
    path: &str,
    executable_name: &str,
) -> core::result::Result<(Process, LoadedElf), ostd::Error> {
    let dentry = crate::fs::vfs::resolve_path(path)?;
    let meta = dentry.inode.metadata()?;
    let mut file_ops = dentry.inode.open(0)?;
    let mut elf_image = alloc::vec![0u8; meta.size];
    let mut offset = 0;
    file_ops.read(&mut elf_image, &mut offset)?;

    let mut process = Process::new(vm, executable_name);
    let loaded = process.exec(path, &elf_image, &[path], &[])?;

    Ok((process, loaded))
}
