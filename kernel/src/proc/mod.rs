pub mod elf;
pub mod pid_table;
pub mod process;
pub mod scheduler;
pub mod thread;
pub mod tid_table;
pub mod user;

// Re-export the most commonly used thread types so that other modules can
// write `crate::proc::KernelThread` without the full submodule path.
pub use thread::{KernelThread, spawn_kernel_thread};
pub use tid_table::{THREAD_TABLE, Tid};

use crate::proc::elf::LoadedElf;
use crate::vm::VMA_MANAGER;
use ostd::Error;
use process::Process;

pub fn init() {
    scheduler::init();
    spawn_init_process();
}

/// Fallback executable paths probed for the init process, in order.
///
/// Matches the Linux v6.19 `kernel_init()` probe sequence:
/// `/sbin/init` → `/etc/init` → `/bin/init` → `/bin/sh`.
const DEFAULT_INIT_EXEC_PATHS: &[&str] = &["/sbin/init", "/etc/init", "/bin/init", "/bin/sh"];

/// Optional in-kernel init image.
///
/// Keep this as `None` until the boot/module layer can provide real init
/// bytes; when wired, [`spawn_init_process`] will execute it with `xmas-elf`.
const EMBEDDED_INIT_ELF: Option<&[u8]> = None;

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
/// If an in-kernel init ELF image is available, PetraOS creates PID 1 and
/// executes that image with `argv = [path]` and an empty `envp`. Otherwise,
/// without a VFS, it falls back to registering the intended init process name.
///
/// # Panics
/// Panics if `vm::init()` has not been called before this function.
pub fn spawn_init_process() {
    spawn_init_process_with_path(None);
}

/// Low-level init-process spawner used by [`spawn_init_process`] and
/// future boot-parameter parsing.
///
/// * `executable_path` – explicit path (e.g. from `init=` cmdline arg).
///   `None` means "probe the default list".
pub fn spawn_init_process_with_path(executable_path: Option<&str>) {
    let vm = VMA_MANAGER
        .get()
        .expect("vm::init() must be called before spawning init")
        .clone();

    // Without a VFS we cannot probe the fallback list yet, so choose the
    // explicit init path or the first Linux-compatible default.
    let path = executable_path.unwrap_or(DEFAULT_INIT_EXEC_PATHS[0]);
    let executable_name = path.rfind('/').map_or(path, |i| &path[i + 1..]);
    let mut init = Process::new(vm, executable_name);

    if let Some(elf_image) = EMBEDDED_INIT_ELF {
        kernel_execve(&mut init, path, elf_image, &[path], &[])
            .expect("failed to execute init ELF image");
    }
}

/// Create PID 1 from an in-memory ELF image.
///
/// This is the init-process entry point to use while PetraOS has no VFS-backed
/// `/sbin/init` lookup. The caller supplies the already available ELF bytes
/// (for example from a boot module or an `include_bytes!` image), and this
/// function executes it as PID 1.
pub fn spawn_init_process_from_elf(
    executable_path: Option<&str>,
    elf_image: &[u8],
) -> Result<(Process, LoadedElf), Error> {
    let vm = VMA_MANAGER
        .get()
        .expect("vm::init() must be called before spawning init")
        .clone();
    let path = executable_path.unwrap_or(DEFAULT_INIT_EXEC_PATHS[0]);
    let executable_name = path.rfind('/').map_or(path, |i| &path[i + 1..]);
    let mut init = Process::new(vm, executable_name);
    let image = kernel_execve(&mut init, path, elf_image, &[path], &[])?;

    Ok((init, image))
}

/// Replace `process` with a new executable image.
///
/// This is PetraOS's current in-memory equivalent of `execve(path, argv, envp)`.
/// The VFS layer will later use the same process method after reading `path`
/// from storage.
pub fn kernel_execve(
    process: &mut Process,
    path: &str,
    elf_image: &[u8],
    argv: &[&str],
    envp: &[&str],
) -> Result<LoadedElf, Error> {
    process.exec(path, elf_image, argv, envp)
}
