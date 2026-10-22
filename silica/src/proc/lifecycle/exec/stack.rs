// SPDX-License-Identifier: GPL-2.0

//! System V AMD64 ABI user stack and Linux auxiliary vector (`auxv`) builder.

use alloc::{sync::Arc, vec::Vec};

use crate::{
    uapi::{cred::Ucred, errno::Result},
    utils::userspace::user_ptr::UserPtr,
    vm::{Vmar, flags::MmapFlags, perms::VmPerms},
};

/// High address limit for the user stack.
pub const USER_STACK_TOP: usize = 0x0000_7fff_ffff_0000;
/// Total size of the user stack (8 MiB).
pub const USER_STACK_SIZE: usize = 8 * 1024 * 1024;
/// Base address of the user stack.
pub const USER_STACK_BASE: usize = USER_STACK_TOP - USER_STACK_SIZE;

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

pub fn push_bytes(vmar: &Arc<Vmar>, sp: &mut usize, bytes: &[u8]) -> Result<usize> {
    *sp -= bytes.len();
    UserPtr::new(*sp).write_bytes(vmar, bytes)?;
    Ok(*sp)
}

pub fn push_u64(vmar: &Arc<Vmar>, sp: &mut usize, val: u64) -> Result<()> {
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
