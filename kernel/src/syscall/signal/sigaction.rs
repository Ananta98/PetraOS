/// `rt_sigaction(signum, new_act, old_act, sigsetsize)` — install a signal
/// handler (SYS_rt_sigaction = 13).
///
/// The user-space `struct sigaction` layout (x86-64 Linux ABI):
/// ```text
/// offset  0: sa_handler  (usize — function pointer or SIG_DFL=0 / SIG_IGN=1)
/// offset  8: sa_flags    (u64)
/// offset 16: sa_restorer (usize — trampoline, currently stored but unused)
/// offset 24: sa_mask     (u64 — 8-byte sigset_t for the standard 64-signal set)
/// ```
/// Total: 32 bytes.
///
/// `SIG_DFL` (0) installs the kernel default action.
/// `SIG_IGN` (1) installs an ignore disposition.
/// Any other value is treated as a user-space handler virtual address.
///
/// Returns `0` on success, or a negated `errno` on failure.
use crate::ipc::{SIGRTMAX, SigAction, SigHandler, SigHandlerKind, SigSet};
use crate::proc::process::Process;
use crate::syscall::{SyscallResult, to_continue_unit};
use crate::vm::vma::VmaManager;
use ostd::Error;

/// Size of the user-space `struct sigaction` in bytes.
const SIGACTION_SIZE: usize = 32;
const SA_HANDLER_OFFSET: usize = 0;
const SA_FLAGS_OFFSET: usize = 8;
const SA_RESTORER_OFFSET: usize = 16;
const SA_MASK_OFFSET: usize = 24;

/// `SIG_DFL` sentinel (matches Linux ABI).
const SIG_DFL: usize = 0;
/// `SIG_IGN` sentinel (matches Linux ABI).
const SIG_IGN: usize = 1;

/// System call entry: `rt_sigaction(signum, new_act, old_act, sigsetsize)`.
pub fn syscall_rt_sigaction(
    arg0: usize, // int signum
    arg1: usize, // const struct sigaction __user *new_act  (0 = NULL)
    arg2: usize, // struct sigaction __user *old_act         (0 = NULL)
    arg3: usize, // size_t sigsetsize (must equal 8)
    _: usize,
    _: usize,
    vm: &VmaManager,
    _: &mut ostd::arch::cpu::context::UserContext,
) -> SyscallResult {
    let signum = arg0 as u32;
    let new_act_ptr = arg1;
    let old_act_ptr = arg2;
    let sigsetsize = arg3;

    if signum == 0 || signum > SIGRTMAX {
        return to_continue_unit(Err(Error::InvalidArgs));
    }
    if sigsetsize != 8 {
        return to_continue_unit(Err(Error::InvalidArgs));
    }

    let process = Process::current();
    let signals = process.signals.clone();

    // ── Export old action before replacing it ─────────────────────────────
    if old_act_ptr != 0 {
        if let Err(err) = write_sigaction_to_user(vm, old_act_ptr, &signals.table, signum) {
            return to_continue_unit(Err(err));
        }
    }

    // ── Read and install the new action ──────────────────────────────────
    if new_act_ptr != 0 {
        match read_sigaction_from_user(vm, new_act_ptr) {
            Ok(action) => {
                signals.table.set_action(signum, action);
            }
            Err(err) => return to_continue_unit(Err(err)),
        }
    }

    to_continue_unit(Ok(()))
}

// ─────────────────────────────────────────────────────────────────────────────
// User-space copy helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Read a `struct sigaction` from user space at `ptr` and convert to a kernel
/// `SigAction`.
fn read_sigaction_from_user(vm: &VmaManager, ptr: usize) -> Result<SigAction, Error> {
    let mut buf = [0u8; SIGACTION_SIZE];
    vm.copy_from_user(ptr, &mut buf)?;

    let sa_handler = usize::from_ne_bytes(
        buf[SA_HANDLER_OFFSET..SA_HANDLER_OFFSET + 8]
            .try_into()
            .unwrap_or([0u8; 8]),
    );
    let sa_flags = u64::from_ne_bytes(
        buf[SA_FLAGS_OFFSET..SA_FLAGS_OFFSET + 8]
            .try_into()
            .unwrap_or([0u8; 8]),
    );
    let sa_mask = u64::from_ne_bytes(
        buf[SA_MASK_OFFSET..SA_MASK_OFFSET + 8]
            .try_into()
            .unwrap_or([0u8; 8]),
    );

    let handler = match sa_handler {
        SIG_DFL => SigHandler::Default,
        SIG_IGN => SigHandler::Ignore,
        addr => SigHandler::UserHandler(addr),
    };

    Ok(SigAction {
        handler,
        mask: SigSet::from_u64(sa_mask),
        flags: sa_flags as u32,
    })
}

/// Write the currently-installed action for `signum` into the user-space
/// `struct sigaction` at `ptr`.
fn write_sigaction_to_user(
    vm: &VmaManager,
    ptr: usize,
    table: &crate::ipc::SigTable,
    signum: u32,
) -> Result<(), Error> {
    let mut buf = [0u8; SIGACTION_SIZE];

    let sa_handler: usize = match table.get_handler_kind(signum) {
        SigHandlerKind::Default | SigHandlerKind::KernelHandler => SIG_DFL,
        SigHandlerKind::Ignore => SIG_IGN,
        SigHandlerKind::UserHandler(addr) => addr,
    };
    let sa_mask = table.handler_mask(signum).as_u64();

    buf[SA_HANDLER_OFFSET..SA_HANDLER_OFFSET + 8].copy_from_slice(&sa_handler.to_ne_bytes());
    buf[SA_FLAGS_OFFSET..SA_FLAGS_OFFSET + 8].copy_from_slice(&0u64.to_ne_bytes());
    buf[SA_RESTORER_OFFSET..SA_RESTORER_OFFSET + 8].copy_from_slice(&0usize.to_ne_bytes());
    buf[SA_MASK_OFFSET..SA_MASK_OFFSET + 8].copy_from_slice(&sa_mask.to_le_bytes());

    vm.copy_to_user(ptr, &buf)
}
