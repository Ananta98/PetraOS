// SPDX-License-Identifier: GPL-2.0

//! Executable binary loading, shebang script dispatch, and `execve` state transition.

pub mod elf;
pub mod shebang;
pub mod stack;

pub use self::{
    elf::{
        DEFAULT_INTERP_LOAD_BIAS, DEFAULT_PIE_LOAD_BIAS, ElfLoadInfo, load_elf, load_elf_with_bias,
    },
    shebang::{MAX_SHEBANG_LINE_LEN, MAX_SHEBANG_RECURSION, SHEBANG_MAGIC, Shebang},
    stack::{
        AT_BASE, AT_CLKTCK, AT_EGID, AT_ENTRY, AT_EUID, AT_EXECFN, AT_FLAGS, AT_GID, AT_NULL,
        AT_PAGESZ, AT_PHDR, AT_PHENT, AT_PHNUM, AT_RANDOM, AT_SECURE, AT_UID, USER_STACK_BASE,
        USER_STACK_SIZE, USER_STACK_TOP, push_bytes, push_u64, setup_user_stack,
        setup_user_stack_full,
    },
};

use alloc::{string::String, sync::Arc, vec::Vec};
use ostd::arch::cpu::context::UserContext;
use ostd::user::UserContextApi;

use crate::{
    api::{
        cred::Ucred,
        errno::{Errno, Result},
        signal::{SigHandler, SigStack},
    },
    proc::{
        comm_from_name,
        thread::{Thread, ThreadFlags, ThreadHooks, ThreadState},
    },
    vm::Vmar,
};

/// Reads an executable or interpreter binary by path.
pub fn read_binary_data(path: &str) -> Result<Vec<u8>> {
    // 1. Try VFS open
    if let Ok(file) = crate::fs::open(path, crate::fs::OpenFlags::READ) {
        let mut data = Vec::new();
        let mut buf = [0u8; 8192];
        loop {
            let n = file.read(&mut buf)?;
            if n == 0 {
                break;
            }
            data.extend_from_slice(&buf[..n]);
        }
        return Ok(data);
    }

    // 2. Try raw initramfs lookup
    if let Some(data) = crate::fs::read_file_from_initramfs(path) {
        if data.starts_with(b"\x7fELF") || data.starts_with(b"#!") {
            return Ok(Vec::from(data));
        }
        if let Ok(target) = core::str::from_utf8(data) {
            let target = target.trim();
            if let Some(target_data) = crate::fs::read_file_from_initramfs(target) {
                return Ok(Vec::from(target_data));
            }
            let trimmed_target = target.trim_start_matches('/');
            if let Some(target_data) = crate::fs::read_file_from_initramfs(trimmed_target) {
                return Ok(Vec::from(target_data));
            }
        }
        return Ok(Vec::from(data));
    }

    // 3. Try stripped leading slash for initramfs
    let trimmed = path.trim_start_matches('/');
    if let Some(data) = crate::fs::read_file_from_initramfs(trimmed) {
        if data.starts_with(b"\x7fELF") || data.starts_with(b"#!") {
            return Ok(Vec::from(data));
        }
        if let Ok(target) = core::str::from_utf8(data) {
            let target = target.trim();
            if let Some(target_data) = crate::fs::read_file_from_initramfs(target) {
                return Ok(Vec::from(target_data));
            }
            let trimmed_target = target.trim_start_matches('/');
            if let Some(target_data) = crate::fs::read_file_from_initramfs(trimmed_target) {
                return Ok(Vec::from(target_data));
            }
        }
        return Ok(Vec::from(data));
    }

    Err(Errno::ENOENT)
}

/// Loads ELF binary, dynamically resolves any `PT_INTERP` interpreter, initializes heap `brk`,
/// and sets up user stack frame with full `auxv`. Returns `(vmar, exec_entry, sp)`.
pub fn load_and_setup_full(
    elf_data: &[u8],
    argv: &[&str],
    envp: &[&str],
    exec_path: &str,
    cred: &Ucred,
) -> Result<(Arc<Vmar>, usize, usize)> {
    let vmar = Vmar::new();

    // 1. Load the main ELF binary
    let exec_info = load_elf_with_bias(&vmar, elf_data, DEFAULT_PIE_LOAD_BIAS)?;

    // 2. Resolve PT_INTERP dynamic linker
    let (interp_base, exec_entry_point) = if let Some(interp_path) = &exec_info.interp_path {
        let interp_data = read_binary_data(interp_path)?;
        let interp_info = load_elf_with_bias(&vmar, &interp_data, DEFAULT_INTERP_LOAD_BIAS)?;
        (interp_info.load_bias, interp_info.entry_point)
    } else {
        (0, exec_info.entry_point)
    };

    // 3. Initialize process heap break (`brk`)
    vmar.init_brk(exec_info.brk_init);

    // 4. Construct System V AMD64 ABI user stack
    let sp = setup_user_stack_full(
        &vmar,
        argv,
        envp,
        exec_path,
        exec_info.entry_point,
        exec_info.phdr_addr,
        exec_info.phent_size,
        exec_info.phnum,
        interp_base,
        cred,
    )?;

    // 5. Install signal trampoline once
    crate::arch::signal::install_trampoline(&vmar)?;

    Ok((vmar, exec_entry_point, sp))
}

/// Loads ELF binary and sets up user stack, returning `(vmar, entry_point, sp)`.
pub fn load_and_setup(
    elf_data: &[u8],
    argv: &[&str],
    envp: &[&str],
) -> Result<(Arc<Vmar>, usize, usize)> {
    let exec_path = argv.first().copied().unwrap_or("/init");
    let cred = Ucred::root();
    load_and_setup_full(elf_data, argv, envp, exec_path, &cred)
}

/// Loads a new program into the calling thread's process, replacing its address space
/// and initializing `ctx` to the entry point and stack pointer.
pub fn execve(
    td: &Thread,
    path: &str,
    argv: &[&str],
    envp: &[&str],
    ctx: &mut UserContext,
) -> Result<()> {
    execve_depth(td, path, argv, envp, ctx, 0)
}

fn execve_depth(
    td: &Thread,
    path: &str,
    argv: &[&str],
    envp: &[&str],
    ctx: &mut UserContext,
    depth: usize,
) -> Result<()> {
    if depth > MAX_SHEBANG_RECURSION {
        crate::return_errno!(ELOOP, "too many levels of shebang script recursion");
    }

    let proc = td.proc().ok_or(Errno::ESRCH)?;
    let elf_data = read_binary_data(path)?;

    // Check if the binary is a Shebang script
    if Shebang::is_shebang(&elf_data) {
        let shebang = Shebang::parse(&elf_data)?.ok_or(Errno::ENOEXEC)?;
        let new_argv = shebang.build_argv(path, argv);
        return execve_depth(td, shebang.interpreter(), &new_argv, envp, ctx, depth + 1);
    }

    let cred = proc.cred();
    // Load ELF, interpreter, and build new address space
    let (new_vmar, entry_point, sp) = load_and_setup_full(&elf_data, argv, envp, path, &cred)?;

    // FIX Bug 3: Terminate all sibling threads prior to address space swap
    let siblings: Vec<Arc<Thread>> = {
        let mut inner = proc.inner.lock();
        let siblings = inner
            .threads
            .iter()
            .filter(|t| t.tid != td.tid)
            .cloned()
            .collect();
        inner.threads.retain(|t| t.tid == td.tid);
        siblings
    };
    for sibling in siblings {
        sibling.handle_clear_child_tid(&proc);
        let mut td_inner = sibling.inner.lock();
        td_inner.state = ThreadState::Dead;
        td_inner
            .flags
            .insert(ThreadFlags::TDF_ASTPENDING | ThreadFlags::KILL_PENDING);
    }

    // Close descriptors marked FD_CLOEXEC
    proc.fd_table.lock().close_on_exec();

    // Reset caught signals to default
    let mut sigacts = proc.sigacts.lock();
    for act in sigacts.actions.iter_mut() {
        if matches!(act.sa_handler, SigHandler::Handler(_)) {
            act.sa_handler = SigHandler::Default;
        }
    }
    drop(sigacts);

    // FIX Bug 6: State sanitization
    // 1. Reset thread alternate signal stack
    td.inner.lock().altstack = SigStack::default();
    // 2. Clear clear_child_tid pointer
    td.inner.lock().clear_child_tid = 0;
    // 3. Clear SysV shared memory attachments
    proc.inner.lock().shm_attachments.clear();

    // Swap process address space
    proc.set_vmspace(Arc::clone(&new_vmar));
    new_vmar.activate();

    // Reset thread TLS base
    td.set_fs_base(0);

    // Update command name and exe_path
    {
        let mut inner = proc.inner.lock();
        inner.comm = comm_from_name(path);
        inner.exe_path = Some(String::from(path));
    }

    // Reset user context and prepare execution from the entry point
    *ctx = UserContext::default();
    ctx.set_instruction_pointer(entry_point);
    ctx.set_stack_pointer(sp);

    // Wake any parent sleeping in vfork
    proc.wake_vfork();
    if let Some(ppid) = proc.ppid()
        && let Some(parent) = crate::proc::allproc_find(ppid)
    {
        parent.wake_vfork();
    }

    Ok(())
}

/// `kern_execve`: loads a new program into the calling thread's process and enters user loop.
pub fn kern_execve(td: &Thread, path: &str, argv: &[&str], envp: &[&str]) -> Result<()> {
    let mut user_ctx = UserContext::default();
    execve(td, path, argv, envp, &mut user_ctx)?;
    td.user_loop(user_ctx, ThreadHooks::empty());
    Ok(())
}
