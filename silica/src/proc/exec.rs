// SPDX-License-Identifier: GPL-2.0

//! Executable loading and `execve` support.
//!
//! Loads ELF executables using `xmas_elf`, sets up the initial AMD64 ABI user stack frame,
//! enforces W^X permissions via [`Vmar::mprotect`], and replaces process address spaces.

use alloc::{sync::Arc, vec::Vec};
use ostd::user::UserContextApi;
use xmas_elf::{
    ElfFile,
    header::{Class, Data, Machine},
    program::Type,
};

use crate::{
    api::{
        errno::{Errno, Result},
        signal::SigHandler,
    },
    proc::thread::Thread,
    utils::userspace::user_ptr::UserPtr,
    vm::{VMAR_CAP_ADDR, VMAR_LOWEST_ADDR, Vmar, flags::MmapFlags, perms::VmPerms},
};

/// High address limit for the user stack.
pub const USER_STACK_TOP: usize = 0x0000_7fff_ffff_0000;
/// Total size of the user stack (8 MiB).
pub const USER_STACK_SIZE: usize = 8 * 1024 * 1024;
/// Base address of the user stack.
pub const USER_STACK_BASE: usize = USER_STACK_TOP - USER_STACK_SIZE;

/// Loads an ELF binary image into `vmar` and returns the entry point virtual address.
pub fn load_elf(vmar: &Arc<Vmar>, elf_data: &[u8]) -> Result<usize> {
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

    let entry_point = elf.header.pt2.entry_point() as usize;
    let mut max_seg_end = 0;

    // 2. Iterate and map PT_LOAD segments
    for ph in elf.program_iter() {
        if ph.get_type() != Ok(Type::Load) {
            continue;
        }

        let vaddr = ph.virtual_addr() as usize;
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
        0x0040_0000
    };
    vmar.init_brk(brk_init);

    Ok(entry_point)
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

/// Sets up the user stack adhering strictly to System V AMD64 ABI.
/// Returns the initial stack pointer (`%rsp`), guaranteed to be 16-byte aligned.
pub fn setup_user_stack(
    vmar: &Arc<Vmar>,
    argv: &[&str],
    envp: &[&str],
    entry_point: usize,
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

    // Align stack to 8 bytes before pointer arrays
    sp &= !7;

    // Push 16 cryptographically random bytes for AT_RANDOM (used for stack canary & SSP)
    let mut random_bytes = [0u8; 16];
    crate::crypto::get_random_bytes(&mut random_bytes);
    let at_random_ptr = push_bytes(vmar, &mut sp, &random_bytes)?;
    sp &= !7;

    // 4. Construct auxiliary vector (AT_NULL, AT_PAGESZ, AT_ENTRY, AT_RANDOM)
    let auxv: [(u64, u64); 4] = [
        (6, ostd::mm::PAGE_SIZE as u64), // AT_PAGESZ
        (9, entry_point as u64),         // AT_ENTRY
        (25, at_random_ptr as u64),      // AT_RANDOM
        (0, 0),                          // AT_NULL
    ];

    // Calculate total 8-byte entries on stack:
    // argc (1) + argv ptrs + NULL (1) + envp ptrs + NULL (1) + auxv (2 * 4 = 8)
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

/// Loads ELF binary and sets up user stack, returning `(vmar, entry_point, sp)`.
pub fn load_and_setup(
    elf_data: &[u8],
    argv: &[&str],
    envp: &[&str],
) -> Result<(Arc<Vmar>, usize, usize)> {
    let vmar = Vmar::new();
    let entry_point = load_elf(&vmar, elf_data)?;
    let sp = setup_user_stack(&vmar, argv, envp, entry_point)?;

    // Last, because the stack is mapped at a fixed address and `MAP_FIXED` will
    // happily cover an existing mapping: installing the trampoline before the stack
    // put the trampoline underneath it.
    //
    // Before anything can install a handler, though: a handler has to come back
    // through it, and there is no vDSO to point `sa_restorer` at otherwise.
    crate::arch::signal::install_trampoline(&vmar)?;

    Ok((vmar, entry_point, sp))
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
    let proc = td.proc().ok_or(Errno::ESRCH)?;

    let file_data_vec;
    let elf_data = if let Ok(file) = crate::fs::open(path, crate::fs::OpenFlags::READ) {
        let mut data = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            let n = file.read(&mut buf)?;
            if n == 0 {
                break;
            }
            data.extend_from_slice(&buf[..n]);
        }
        file_data_vec = data;
        &file_data_vec[..]
    } else if let Some(data) = crate::fs::read_file_from_initramfs(path) {
        data
    } else if let Some(data) = crate::fs::read_file_from_initramfs(path.trim_start_matches('/')) {
        data
    } else {
        return Err(Errno::ENOENT);
    };

    // Load ELF and build new address space
    let (new_vmar, entry_point, sp) = load_and_setup(elf_data, argv, envp)?;

    // Close descriptors marked FD_CLOEXEC
    proc.fd_table.lock().close_on_exec();

    // Reset caught signals to default
    {
        let mut sigacts = proc.sigacts.lock();
        for act in sigacts.actions.iter_mut() {
            if matches!(act.sa_handler, SigHandler::Handler(_)) {
                act.sa_handler = SigHandler::Default;
            }
        }
    }

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
