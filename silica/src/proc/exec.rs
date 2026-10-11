// SPDX-License-Identifier: GPL-2.0

//! Executable loading and `execve` support.
//!
//! Loads ELF executables using `xmas_elf`, sets up the initial AMD64 ABI user stack frame,
//! enforces W^X permissions via [`Vmar::mprotect`], and replaces process address spaces.
//! Supports both static executables (`ET_EXEC`) and Position-Independent Executables (`ET_DYN`),
//! dynamic linking via `PT_INTERP` (e.g. musl libc `ld-musl-x86_64.so.1`), and complete
//! Linux auxiliary vectors (`auxv`).

use alloc::{string::String, sync::Arc, vec::Vec};
use ostd::user::UserContextApi;
use xmas_elf::{
    ElfFile,
    header::{Class, Data, Machine, Type as HeaderType},
    program::Type,
};

use crate::{
    api::{
        cred::Ucred,
        errno::{Errno, Result},
        signal::SigHandler,
    },
    proc::thread::Thread,
    utils::userspace::user_ptr::UserPtr,
    vm::{VMAR_CAP_ADDR, VMAR_LOWEST_ADDR, Vmar, flags::MmapFlags, perms::VmPerms},
};

use super::shebang::{MAX_SHEBANG_RECURSION, Shebang};

/// High address limit for the user stack.
pub const USER_STACK_TOP: usize = 0x0000_7fff_ffff_0000;
/// Total size of the user stack (8 MiB).
pub const USER_STACK_SIZE: usize = 8 * 1024 * 1024;
/// Base address of the user stack.
pub const USER_STACK_BASE: usize = USER_STACK_TOP - USER_STACK_SIZE;

/// Default base address for loading Position-Independent Executables (PIE, ET_DYN).
///
/// Matches Linux standard x86-64 `ELF_ET_DYN_BASE` (`0x555555554000` / `0x555555550000`).
pub const DEFAULT_PIE_LOAD_BIAS: usize = 0x0000_5555_5555_0000;

/// Default base address for loading the dynamic linker interpreter (`PT_INTERP`, e.g. ld-musl).
///
/// Placed safely below the user stack and high enough above typical user heap/mmap areas.
pub const DEFAULT_INTERP_LOAD_BIAS: usize = 0x0000_7f00_0000_0000;

// Linux Auxiliary Vector (auxv) keys
pub const AT_NULL: u64 = 0;
pub const AT_IGNORE: u64 = 1;
pub const AT_EXECFD: u64 = 2;
pub const AT_PHDR: u64 = 3;
pub const AT_PHENT: u64 = 4;
pub const AT_PHNUM: u64 = 5;
pub const AT_PAGESZ: u64 = 6;
pub const AT_BASE: u64 = 7;
pub const AT_FLAGS: u64 = 8;
pub const AT_ENTRY: u64 = 9;
pub const AT_NOTELF: u64 = 10;
pub const AT_UID: u64 = 11;
pub const AT_EUID: u64 = 12;
pub const AT_GID: u64 = 13;
pub const AT_EGID: u64 = 14;
pub const AT_PLATFORM: u64 = 15;
pub const AT_HWCAP: u64 = 16;
pub const AT_CLKTCK: u64 = 17;
pub const AT_SECURE: u64 = 23;
pub const AT_BASE_PLATFORM: u64 = 24;
pub const AT_RANDOM: u64 = 25;
pub const AT_HWCAP2: u64 = 26;
pub const AT_EXECFN: u64 = 31;
pub const AT_SYSINFO_EHDR: u64 = 33;

/// Metadata collected when loading an ELF image into a VMAR.
#[derive(Debug, Clone)]
pub struct ElfLoadInfo {
    /// Virtual address of the binary's entry point (adjusted by `load_bias`).
    pub entry_point: usize,
    /// Virtual address where the program headers table resides in memory (`AT_PHDR`).
    pub phdr_addr: usize,
    /// Size in bytes of each program header entry (`AT_PHENT`).
    pub phent_size: usize,
    /// Number of program headers (`AT_PHNUM`).
    pub phnum: usize,
    /// Path to the dynamic linker interpreter if `PT_INTERP` was present.
    pub interp_path: Option<String>,
    /// Load bias applied to segments.
    pub load_bias: usize,
    /// End address of the highest mapped segment, used to initialize process `brk`.
    pub brk_init: usize,
}

/// Reads an executable or interpreter binary by path.
///
/// First queries the global VFS root via [`crate::fs::open`] (which resolves symlinks and mounts).
/// Falls back to raw initramfs archive lookup if VFS lookup fails.
pub fn read_binary_data(path: &str) -> Result<Vec<u8>> {
    // 1. Try VFS open (automatically resolves symlinks up to MAX_SYMLINK_DEPTH)
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
        // Might be a symlink target stored as text in initramfs
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

/// Loads an ELF binary image into `vmar` applying a suggested PIE load bias if the image is `ET_DYN`.
pub fn load_elf_with_bias(
    vmar: &Arc<Vmar>,
    elf_data: &[u8],
    suggested_pie_bias: usize,
) -> Result<ElfLoadInfo> {
    let aligned_buf;
    let elf_data = if !(elf_data.as_ptr() as usize).is_multiple_of(core::mem::align_of::<u64>()) {
        aligned_buf = Vec::from(elf_data);
        &aligned_buf[..]
    } else {
        elf_data
    };

    let elf = ElfFile::new(elf_data).map_err(|_| Errno::ENOEXEC)?;

    // 1. Validate ELF Header
    if elf.header.pt1.class() != Class::SixtyFour {
        crate::return_errno!(ENOEXEC, "ELF must be 64-bit");
    }
    if elf.header.pt1.data() != Data::LittleEndian {
        crate::return_errno!(ENOEXEC, "ELF must be little-endian");
    }
    if elf.header.pt2.machine().as_machine() != Machine::X86_64 {
        crate::return_errno!(ENOEXEC, "ELF target machine must be x86_64");
    }

    // Determine load bias based on ELF type
    let load_bias = match elf.header.pt2.type_().as_type() {
        HeaderType::Executable => 0,
        HeaderType::SharedObject => suggested_pie_bias,
        _ => crate::return_errno!(ENOEXEC, "ELF type must be ET_EXEC or ET_DYN"),
    };

    let entry_point = (elf.header.pt2.entry_point() as usize)
        .checked_add(load_bias)
        .ok_or(Errno::ENOEXEC)?;

    let phent_size = elf.header.pt2.ph_entry_size() as usize;
    let phnum = elf.header.pt2.ph_count() as usize;

    let mut interp_path: Option<String> = None;
    let mut phdr_addr: Option<usize> = None;
    let mut first_load_vaddr: Option<usize> = None;
    let mut max_seg_end = 0;

    // Scan program headers for PT_INTERP, PT_PHDR, and record first load segment address
    for ph in elf.program_iter() {
        match ph.get_type() {
            Ok(Type::Interp) => {
                let offset = ph.offset() as usize;
                let file_sz = ph.file_size() as usize;
                if offset.saturating_add(file_sz) <= elf_data.len() {
                    let interp_bytes = &elf_data[offset..offset + file_sz];
                    let len = interp_bytes
                        .iter()
                        .position(|&b| b == 0)
                        .unwrap_or(interp_bytes.len());
                    if let Ok(s) = core::str::from_utf8(&interp_bytes[..len]) {
                        let trimmed = s.trim();
                        if !trimmed.is_empty() {
                            interp_path = Some(String::from(trimmed));
                        }
                    }
                }
            }
            Ok(Type::Phdr) => {
                phdr_addr = Some((ph.virtual_addr() as usize).wrapping_add(load_bias));
            }
            Ok(Type::Load) => {
                if first_load_vaddr.is_none() {
                    first_load_vaddr = Some((ph.virtual_addr() as usize).wrapping_add(load_bias));
                }
            }
            _ => {}
        }
    }

    let phdr_addr = phdr_addr.unwrap_or_else(|| {
        let base = first_load_vaddr.unwrap_or(load_bias);
        base.wrapping_add(elf.header.pt2.ph_offset() as usize)
    });

    // 2. Iterate and map PT_LOAD segments
    for ph in elf.program_iter() {
        if ph.get_type() != Ok(Type::Load) {
            continue;
        }

        let raw_vaddr = ph.virtual_addr() as usize;
        let vaddr = raw_vaddr.checked_add(load_bias).ok_or(Errno::ENOEXEC)?;
        let mem_sz = ph.mem_size() as usize;
        let file_sz = ph.file_size() as usize;
        let offset = ph.offset() as usize;

        if mem_sz < file_sz {
            crate::return_errno!(ENOEXEC, "segment mem_sz < file_sz");
        }
        if mem_sz == 0 {
            continue;
        }

        let page_start = vaddr & !(ostd::mm::PAGE_SIZE - 1);
        let page_offset = vaddr - page_start;
        let total_span =
            (page_offset + mem_sz + ostd::mm::PAGE_SIZE - 1) & !(ostd::mm::PAGE_SIZE - 1);

        let seg_end = page_start + total_span;
        if seg_end > max_seg_end {
            max_seg_end = seg_end;
        }

        if page_start < VMAR_LOWEST_ADDR || page_start.saturating_add(total_span) > VMAR_CAP_ADDR {
            crate::return_errno!(
                ENOEXEC,
                "segment address range outside allowed userspace limits"
            );
        }

        let mut final_perms = VmPerms::empty();
        if ph.flags().is_read() {
            final_perms |= VmPerms::READ;
        }
        if ph.flags().is_write() {
            final_perms |= VmPerms::WRITE;
        }
        if ph.flags().is_execute() {
            final_perms |= VmPerms::EXEC;
        }

        // Map segment with staging write permission so kernel can populate and copy
        let staging_perms = final_perms | VmPerms::WRITE;
        vmar.mmap_anonymous(
            page_start,
            total_span,
            staging_perms,
            MmapFlags::PRIVATE | MmapFlags::FIXED,
        )?;

        // Populate physical frames
        vmar.populate_range(&(page_start..page_start + total_span))?;

        // Activate page table on current CPU to write file bytes
        vmar.activate();

        if file_sz > 0 {
            if offset + file_sz > elf_data.len() {
                crate::return_errno!(ENOEXEC, "ELF segment file offset out of range");
            }
            UserPtr::new(vaddr)
                .write_bytes(vmar, &elf_data[offset..offset + file_sz])
                .map_err(|_| Errno::EIO)?;
        }

        // Zero out trailing partial-page bytes between file_sz and mem_sz if within same page
        if mem_sz > file_sz {
            let bss_start = vaddr + file_sz;
            let bss_page_rem = ostd::mm::PAGE_SIZE - (bss_start & (ostd::mm::PAGE_SIZE - 1));
            let zero_len = (mem_sz - file_sz).min(bss_page_rem);
            if zero_len > 0 {
                let mut writer = vmar
                    .vm_space()
                    .writer(bss_start, zero_len)
                    .map_err(|_| Errno::EIO)?;
                let _ = writer.fill_zeros(zero_len);
            }
        }

        // Lock down permissions to enforce W^X if writable was not requested
        if !final_perms.contains(VmPerms::WRITE) {
            vmar.mprotect(final_perms, page_start..page_start + total_span)?;
        }
    }

    let brk_init = if max_seg_end > 0 {
        max_seg_end
    } else {
        load_bias + 0x0040_0000
    };

    Ok(ElfLoadInfo {
        entry_point,
        phdr_addr,
        phent_size,
        phnum,
        interp_path,
        load_bias,
        brk_init,
    })
}

/// Loads an ELF binary image into `vmar` and returns the entry point virtual address.
pub fn load_elf(vmar: &Arc<Vmar>, elf_data: &[u8]) -> Result<usize> {
    let info = load_elf_with_bias(vmar, elf_data, DEFAULT_PIE_LOAD_BIAS)?;
    vmar.init_brk(info.brk_init);
    Ok(info.entry_point)
}

fn push_bytes(vmar: &Arc<Vmar>, sp: &mut usize, bytes: &[u8]) -> Result<usize> {
    *sp -= bytes.len();
    UserPtr::new(*sp).write_bytes(vmar, bytes)?;
    Ok(*sp)
}

fn push_u64(vmar: &Arc<Vmar>, sp: &mut usize, val: u64) -> Result<()> {
    *sp -= 8;
    UserPtr::new(*sp).write_bytes(vmar, &val.to_ne_bytes())?;
    Ok(())
}

/// Sets up the user stack adhering strictly to System V AMD64 ABI with full Linux `auxv`.
/// Returns the initial stack pointer (`%rsp`), guaranteed to be 16-byte aligned.
pub fn setup_user_stack_full(
    vmar: &Arc<Vmar>,
    argv: &[&str],
    envp: &[&str],
    exec_path: &str,
    entry_point: usize,
    phdr_addr: usize,
    phent_size: usize,
    phnum: usize,
    interp_base: usize,
    cred: &Ucred,
) -> Result<usize> {
    // 1. Map user stack region
    vmar.mmap_anonymous(
        USER_STACK_BASE,
        USER_STACK_SIZE,
        VmPerms::READ | VmPerms::WRITE,
        MmapFlags::PRIVATE | MmapFlags::FIXED,
    )?;

    // 2. Populate the top pages of the stack
    let init_stack_pages = 16 * ostd::mm::PAGE_SIZE;
    vmar.populate_range(&(USER_STACK_TOP - init_stack_pages..USER_STACK_TOP))?;
    vmar.activate();

    let mut sp = USER_STACK_TOP;

    // 3. Push string tables
    // Push exec_path string for AT_EXECFN
    let mut execfn_bytes = Vec::from(exec_path.as_bytes());
    execfn_bytes.push(0); // Null terminator
    let execfn_ptr = push_bytes(vmar, &mut sp, &execfn_bytes)?;

    // Push envp strings
    let mut envp_ptrs = Vec::new();
    for env in envp.iter().rev() {
        let mut s = Vec::from(env.as_bytes());
        s.push(0); // Null terminator
        let addr = push_bytes(vmar, &mut sp, &s)?;
        envp_ptrs.push(addr);
    }
    envp_ptrs.reverse();

    // Push argv strings
    let mut argv_ptrs = Vec::new();
    for arg in argv.iter().rev() {
        let mut s = Vec::from(arg.as_bytes());
        s.push(0); // Null terminator
        let addr = push_bytes(vmar, &mut sp, &s)?;
        argv_ptrs.push(addr);
    }
    argv_ptrs.reverse();

    // Align stack to 8 bytes before random bytes
    sp &= !7;

    // Push 16 cryptographically random bytes for AT_RANDOM (used for stack canary & SSP)
    let mut random_bytes = [0u8; 16];
    crate::crypto::get_random_bytes(&mut random_bytes);
    let at_random_ptr = push_bytes(vmar, &mut sp, &random_bytes)?;
    sp &= !7;

    // 4. Construct complete Linux auxiliary vector (auxv)
    let auxv: &[(u64, u64)] = &[
        (AT_PHDR, phdr_addr as u64),
        (AT_PHENT, phent_size as u64),
        (AT_PHNUM, phnum as u64),
        (AT_PAGESZ, ostd::mm::PAGE_SIZE as u64),
        (AT_BASE, interp_base as u64),
        (AT_FLAGS, 0),
        (AT_ENTRY, entry_point as u64),
        (AT_UID, cred.cr_ruid.as_u32() as u64),
        (AT_EUID, cred.cr_uid.as_u32() as u64),
        (AT_GID, cred.cr_rgid.as_u32() as u64),
        (AT_EGID, cred.cr_gid.as_u32() as u64),
        (AT_CLKTCK, 100),
        (AT_SECURE, 0),
        (AT_RANDOM, at_random_ptr as u64),
        (AT_EXECFN, execfn_ptr as u64),
        (AT_NULL, 0),
    ];

    // Calculate total 8-byte entries on stack:
    // argc (1) + argv ptrs + NULL (1) + envp ptrs + NULL (1) + auxv (2 * count)
    let total_words = 1 + argv_ptrs.len() + 1 + envp_ptrs.len() + 1 + (auxv.len() * 2);

    // Enforce System V AMD64 ABI 16-byte alignment at entry: %rsp % 16 == 0
    let target_sp = sp - (total_words * 8);
    if !target_sp.is_multiple_of(16) {
        push_u64(vmar, &mut sp, 0)?;
    }

    // Push Auxiliary Vector (in reverse)
    for &(key, val) in auxv.iter().rev() {
        push_u64(vmar, &mut sp, val)?;
        push_u64(vmar, &mut sp, key)?;
    }

    // Push envp pointers (NULL terminated)
    push_u64(vmar, &mut sp, 0)?;
    for &ptr in envp_ptrs.iter().rev() {
        push_u64(vmar, &mut sp, ptr as u64)?;
    }

    // Push argv pointers (NULL terminated)
    push_u64(vmar, &mut sp, 0)?;
    for &ptr in argv_ptrs.iter().rev() {
        push_u64(vmar, &mut sp, ptr as u64)?;
    }

    // Push argc
    push_u64(vmar, &mut sp, argv_ptrs.len() as u64)?;

    debug_assert_eq!(sp % 16, 0, "AMD64 ABI stack alignment violated");
    Ok(sp)
}

/// Sets up the user stack adhering strictly to System V AMD64 ABI.
/// Returns the initial stack pointer (`%rsp`), guaranteed to be 16-byte aligned.
pub fn setup_user_stack(
    vmar: &Arc<Vmar>,
    argv: &[&str],
    envp: &[&str],
    entry_point: usize,
) -> Result<usize> {
    let cred = Ucred::root();
    let exec_path = argv.first().copied().unwrap_or("/init");
    setup_user_stack_full(vmar, argv, envp, exec_path, entry_point, 0, 56, 0, 0, &cred)
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

    // 1. Load main ELF with default PIE bias
    let main_info = load_elf_with_bias(&vmar, elf_data, DEFAULT_PIE_LOAD_BIAS)?;

    // 2. Initialize heap brk from the main ELF
    vmar.init_brk(main_info.brk_init);

    // 3. Handle dynamic linker interpreter (PT_INTERP) if present
    let (interp_base, exec_entry_point) = if let Some(ref interp_path) = main_info.interp_path {
        ostd::info!(
            "exec: Loading dynamic linker interpreter: \"{}\"",
            interp_path
        );
        let interp_data = read_binary_data(interp_path)?;
        let interp_info = load_elf_with_bias(&vmar, &interp_data, DEFAULT_INTERP_LOAD_BIAS)?;
        (interp_info.load_bias, interp_info.entry_point)
    } else {
        (0, main_info.entry_point)
    };

    // 4. Set up user stack with full auxiliary vector
    let sp = setup_user_stack_full(
        &vmar,
        argv,
        envp,
        exec_path,
        main_info.entry_point,
        main_info.phdr_addr,
        main_info.phent_size,
        main_info.phnum,
        interp_base,
        cred,
    )?;

    // 5. Install signal trampoline
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
    ctx: &mut ostd::arch::cpu::context::UserContext,
) -> Result<()> {
    execve_depth(td, path, argv, envp, ctx, 0)
}

fn execve_depth(
    td: &Thread,
    path: &str,
    argv: &[&str],
    envp: &[&str],
    ctx: &mut ostd::arch::cpu::context::UserContext,
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

    // Install trampoline in new address space
    crate::arch::signal::install_trampoline(&new_vmar)?;

    // Swap process address space
    proc.set_vmspace(Arc::clone(&new_vmar));
    new_vmar.activate();

    // Reset thread TLS base
    td.set_fs_base(0);

    // Update command name
    proc.inner.lock().comm = super::comm_from_name(path);

    // Reset user context and prepare execution from the entry point
    *ctx = ostd::arch::cpu::context::UserContext::default();
    ctx.set_instruction_pointer(entry_point);
    ctx.set_stack_pointer(sp);

    Ok(())
}

/// `kern_execve`: loads a new program into the calling thread's process.
pub fn kern_execve(td: &Thread, path: &str, argv: &[&str], envp: &[&str]) -> Result<()> {
    let mut user_ctx = ostd::arch::cpu::context::UserContext::default();
    execve(td, path, argv, envp, &mut user_ctx)?;
    td.user_loop(user_ctx, crate::proc::thread::ThreadHooks::empty());
    Ok(())
}
