pub mod credentials;
pub mod elf;
pub mod pid_table;
pub mod process;
pub mod process_group;
pub mod thread;
pub mod thread_local;
pub mod tid_table;
pub mod userspace;

// Re-export the most commonly used thread types so that other modules can
// write `crate::proc::KernelThread` without the full submodule path.

use crate::proc::elf::LoadedElf;
use crate::vm::VMA_MANAGER;
use crate::vm::vma::VmaManager;
use alloc::sync::Arc;
use alloc::vec::Vec;
use process::Process;

/// Spawn the **init** process (PID 1).
///
/// Mirrors the logic in Linux `kernel_init()` and Asterinas
/// `spawn_init_process()`:
///
/// 1. If a custom path is provided via the `init=` kernel command line
///    argument, use it.
/// 2. Otherwise probe the canonical fallback list in order:
///    `/sbin/init` → `/etc/init` → `/bin/init` → `/bin/sh`.
///
/// Each path is resolved through the VFS, read into memory, and loaded as
/// an ELF executable.  The first path that resolves and loads successfully
/// becomes PID 1.
///
/// # Panics
/// Panics if `vm::init()` has not been called before this function.
pub fn spawn_init_process() {
    const DEFAULT_INIT_EXEC_PATHS: &[&str] = &[
        "/bin/bash",
        "/sbin/init",
        "/etc/init",
        "/bin/init",
        "/bin/sh",
    ];

    let vm = VMA_MANAGER
        .get()
        .expect("vm::init() must be called before spawning init")
        .clone();

    for &path in DEFAULT_INIT_EXEC_PATHS {
        let executable_name = path.rfind('/').map_or(path, |i| &path[i + 1..]);
        ostd::early_println!("[init] Trying to load {}", path);
        match load_init_exec(vm.clone(), path, executable_name) {
            Ok((process, loaded, stack_ptr)) => {
                let entry = loaded.entry;
                ostd::early_println!("[init] Successfully loaded {} at {:#x}", path, entry);

                // Open stdin, stdout, and stderr — prefer /dev/console, fall back to /dev/null.
                let mut fds = process.fd_table.lock();
                let console_path = if fds.open("/dev/console", 0, 0).is_ok() {
                    // stdin (fd 0) successfully opened to console
                    "/dev/console"
                } else {
                    ostd::early_println!("[init] WARNING: /dev/console not found, falling back to /dev/null");
                    let _ = fds.open("/dev/null", 0, 0);
                    "/dev/null"
                };
                let _ = fds.open(console_path, 1, 0); // stdout (fd 1)
                let _ = fds.open(console_path, 1, 0); // stderr (fd 2)

                ostd::early_println!(
                    "[init] Stack pointer ready: {:#x}. Running user mode...",
                    stack_ptr
                );

                // Spawn the main thread. Its body activates the process VM
                // and enters user mode, executing the init program.
                let mut process_for_thread = process.clone();
                let pid = process.pid;
                let name = process.name.clone();
                process
                    .spawn_thread("main", move || {
                        let res = crate::proc::userspace::run_process_user_mode(
                            &mut process_for_thread,
                            entry,
                            stack_ptr,
                        );
                        if let Err(e) = res {
                            ostd::early_println!(
                                "[{} - PID {}] Process user mode returned: Err({:?})",
                                name,
                                pid.as_u32(),
                                e
                            );
                        }
                    })
                    .expect("failed to spawn init thread");
                return;
            }
            Err(e) => {
                ostd::early_println!("[init] Failed to load {}: {:?}", path, e);
            }
        }
    }
    ostd::early_println!("[init] ERROR: No init executable could be loaded!");
}

/// Try to load `path` as an ELF executable and exec it into a new init process.
///
/// Reads the file from the VFS, creates a fresh `Process`, and replaces its
/// address space with the loaded ELF image.  Returns `Ok((process, loaded, stack_ptr))`
/// on success, or `Err` if the path could not be resolved, read, or loaded.
fn load_init_exec(
    vm: Arc<VmaManager>,
    path: &str,
    executable_name: &str,
) -> core::result::Result<(Process, LoadedElf, usize), ostd::Error> {
    ostd::early_println!("[load_init_exec] Resolving path: {}", path);
    let dentry = match crate::fs::vfs::resolve_path(path) {
        Ok(d) => d,
        Err(e) => {
            ostd::early_println!("[load_init_exec] resolve_path failed for {}: {:?}", path, e);
            return Err(e);
        }
    };
    let meta = dentry.inode.metadata()?;
    ostd::early_println!("[load_init_exec] File size: {}", meta.size);
    let mut file_ops = dentry.inode.open(0)?;
    let mut elf_image = alloc::vec![0u8; meta.size];
    let mut offset = 0;
    file_ops.read(&mut elf_image, &mut offset)?;
    ostd::early_println!(
        "[load_init_exec] Read {} bytes into buffer",
        elf_image.len()
    );

    let mut process = Process::new(vm, executable_name);
    let envp = &[
        "PATH=/bin:/sbin:/usr/bin:/usr/sbin",
        "PS1=bash-5.2# ",
        "TERM=linux",
        "HOME=/root",
        "PWD=/",
        "SHELL=/bin/bash",
    ];
    let argv = &[path, "--login", "-i"];
    let (loaded, stack_ptr) = match process.exec(path, &elf_image, argv, envp) {
        Ok(res) => res,
        Err(e) => {
            ostd::early_println!("[load_init_exec] process.exec failed for {}: {:?}", path, e);
            return Err(e);
        }
    };

    Ok((process, loaded, stack_ptr))
}
