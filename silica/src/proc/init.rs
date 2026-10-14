// SPDX-License-Identifier: GPL-2.0

//! Initial user process (`init`, PID 1) bootstrap and execution.
//!
//! Inspired by FreeBSD `sys/kern/init_main.c` and Asterinas `init_proc.rs`.
//! Locates the initial userspace binary or script, resolves shebang interpreters
//! and symlinks, creates the root address space, binds standard descriptors, and launches PID 1.

use alloc::{string::String, sync::Arc, vec::Vec};
use ostd::arch::cpu::context::UserContext;
use ostd::user::UserContextApi;

use crate::{
    api::{
        cred::Ucred,
        errno::{Errno, Result},
    },
    proc::{
        Proc, comm_from_name, create_init_process_with_vmar, create_main_thread,
        exec::{self, load_and_setup_full},
        shebang::{MAX_SHEBANG_RECURSION, Shebang},
    },
};

/// Candidate binary paths inspected when looking for the initial userspace process.
const INIT_CANDIDATE_PATHS: &[&str] = &["/init", "init", "/sbin/init", "/bin/init", "/bin/sh"];

/// Locates the initial userspace binary or script, following shebang wrappers and symlinks.
/// Returns `(final_path, final_argv, elf_data)`.
fn find_init_target() -> Option<(String, Vec<String>, Vec<u8>)> {
    let mut candidates = Vec::new();

    // 1. Explicit command line parameter (e.g., "init=/bin/sh" or "rdinit=/init")
    let kernel_cmdline = &ostd::boot::boot_info().kernel_cmdline;
    for arg in kernel_cmdline.split_whitespace() {
        if let Some(path) = arg.strip_prefix("init=").or_else(|| arg.strip_prefix("rdinit=")) {
            candidates.push(String::from(path));
            break;
        }
    }

    // 2. Standard candidate paths
    for &candidate in INIT_CANDIDATE_PATHS {
        candidates.push(String::from(candidate));
    }

    for path in candidates {
        if let Ok(mut data) = exec::read_binary_data(&path) {
            let mut current_path = path;
            let mut current_argv = alloc::vec![current_path.clone()];
            let mut depth = 0;

            // Follow shebang script wrappers up to recursion limit
            while Shebang::is_shebang(&data) {
                if depth >= MAX_SHEBANG_RECURSION {
                    ostd::warn!("init: Shebang recursion depth exceeded for init binary");
                    break;
                }
                depth += 1;

                let shebang = match Shebang::parse(&data) {
                    Ok(Some(s)) => s,
                    _ => break,
                };

                let argv_refs: Vec<&str> = current_argv.iter().map(|s| s.as_str()).collect();
                let next_argv_refs = shebang.build_argv(&current_path, &argv_refs);
                let next_argv: Vec<String> = next_argv_refs.iter().map(|s| String::from(*s)).collect();
                let interp = String::from(shebang.interpreter());

                match exec::read_binary_data(&interp) {
                    Ok(interp_data) => {
                        current_path = interp;
                        current_argv = next_argv;
                        data = interp_data;
                    }
                    Err(err) => {
                        ostd::warn!("init: Failed to read shebang interpreter \"{}\": {:?}", interp, err);
                        break;
                    }
                }
            }

            // Verify that we have a valid ELF binary
            if data.starts_with(b"\x7fELF") {
                return Some((current_path, current_argv, data));
            }
        }
    }

    None
}

/// Spawns and schedules the initial user process (`init`, PID 1).
///
/// Returns the newly created process descriptor on success.
pub fn spawn_init_process() -> Result<Arc<Proc>> {
    let (init_path, init_argv, elf_data) = match find_init_target() {
        Some(target) => target,
        None => {
            ostd::warn!("init: No userspace init binary found in initramfs or root filesystem");
            return Err(Errno::ENOENT);
        }
    };

    let mut init_argv = init_argv;
    let kernel_cmdline = &ostd::boot::boot_info().kernel_cmdline;
    if let Some((_, after_dash)) = kernel_cmdline.split_once("--") {
        for arg in after_dash.split_whitespace() {
            if !arg.starts_with("initrd=") && !arg.starts_with("BOOT_IMAGE=") {
                init_argv.push(String::from(arg));
            }
        }
    } else {
        for arg in kernel_cmdline.split_whitespace() {
            if !arg.contains('=') && arg != "--" {
                init_argv.push(String::from(arg));
            }
        }
    }

    ostd::info!(
        "init: Booting userspace init from \"{}\" ({} bytes, argv: {:?})",
        init_path,
        elf_data.len(),
        init_argv
    );

    let argv_refs: Vec<&str> = init_argv.iter().map(|s| s.as_str()).collect();
    let root_cred = Ucred::root();

    let mut init_envp: Vec<String> = Vec::new();
    let mut has_path = false;
    let mut has_term = false;
    let mut has_home = false;
    let mut has_user = false;

    let before_dash = kernel_cmdline.split_once("--").map(|(b, _)| b).unwrap_or(kernel_cmdline);
    for arg in before_dash.split_whitespace() {
        if arg.contains('=') && !arg.starts_with("init=") && !arg.starts_with("rdinit=") {
            if arg.starts_with("PATH=") { has_path = true; }
            if arg.starts_with("TERM=") { has_term = true; }
            if arg.starts_with("HOME=") { has_home = true; }
            if arg.starts_with("USER=") { has_user = true; }
            init_envp.push(String::from(arg));
        }
    }

    if !has_path {
        init_envp.push(String::from("PATH=/bin:/sbin:/usr/bin:/usr/sbin"));
    }
    if !has_term {
        init_envp.push(String::from("TERM=linux"));
    }
    if !has_home {
        init_envp.push(String::from("HOME=/root"));
    }
    if !has_user {
        init_envp.push(String::from("USER=root"));
    }

    let envp_refs: Vec<&str> = init_envp.iter().map(|s| s.as_str()).collect();

    // 1. Build address space, map ELF segments (supporting PIE and dynamic linking), setup AMD64 ABI user stack
    let (vmar, entry_point, sp) = load_and_setup_full(
        &elf_data,
        &argv_refs,
        &envp_refs,
        &init_path,
        &root_cred,
    )?;

    // 2. Create Process Control Block for PID 1 with root credentials and stdio
    let proc = create_init_process_with_vmar(vmar)?;

    // Set process comm name and exe_path
    {
        let mut inner = proc.inner.lock();
        inner.comm = comm_from_name(&init_path);
        inner.exe_path = Some(init_path.clone());
    }

    // 3. Set up user context
    let mut user_ctx = UserContext::default();
    user_ctx.set_instruction_pointer(entry_point);
    user_ctx.set_stack_pointer(sp);

    // 4. Create and run primary init thread
    let thread = create_main_thread(&proc, user_ctx)?;
    thread.run();

    ostd::info!(
        "init: Init process (PID 1) scheduled successfully (entry: {:#x}, sp: {:#x})",
        entry_point,
        sp
    );

    Ok(proc)
}
