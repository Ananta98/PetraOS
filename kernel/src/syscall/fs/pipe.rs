/// `pipe2(pipefd, flags)` — create a unidirectional pipe (SYS_pipe2 = 293).
///
/// Allocates a new pipe channel and returns two file descriptors:
/// - `pipefd[0]` for the read end.
/// - `pipefd[1]` for the write end.
///
/// # Flags
/// - `O_NONBLOCK` (0x800) / `O_CLOEXEC` (0x80000) can be passed in `flags`.
///
/// Returns `0` on success, or a negated `errno` on failure.
use crate::fs::fd_table::FileDescriptor;
use crate::proc::process::Process;
use crate::syscall::{SyscallResult, to_continue_unit};
use crate::vm::vma::VmaManager;
use ostd::Error;

/// System call entry: `pipe2(pipefd, flags)`.
pub fn syscall_pipe2(
    arg0: usize, // int pipefd[2] (__user *pipefd)
    arg1: usize, // int flags
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    vm: &VmaManager,
    _: &mut ostd::arch::cpu::context::UserContext,
) -> SyscallResult {
    let pipefd_ptr = arg0;
    let flags = arg1 as u32;

    // Validate user pointer.
    if pipefd_ptr == 0 {
        return to_continue_unit(Err(Error::InvalidArgs));
    }

    // Validate flags: only O_NONBLOCK (0x800) and O_CLOEXEC (0x80000) are allowed.
    const ALLOWED_FLAGS: u32 = 0x800 | 0x80000;
    if (flags & !ALLOWED_FLAGS) != 0 {
        return to_continue_unit(Err(Error::InvalidArgs));
    }

    // Create the unidirectional pipe ends.
    let (read_ops, write_ops) = crate::ipc::create_pipe();

    let process = Process::current();
    let mut fd_table = process.fd_table.lock();

    // Allocate two file descriptors.
    let read_fd = match fd_table.alloc_fd(0) {
        Ok(fd) => fd,
        Err(err) => return to_continue_unit(Err(err)),
    };

    let write_fd = match fd_table.alloc_fd(read_fd + 1) {
        Ok(fd) => fd,
        Err(err) => return to_continue_unit(Err(err)),
    };

    // Wrap in FileDescriptors.
    let read_descriptor = FileDescriptor::new(read_ops, flags);
    let write_descriptor = FileDescriptor::new(write_ops, flags);

    // Copy the descriptors to user space.
    let mut bytes = [0u8; 8];
    bytes[0..4].copy_from_slice(&read_fd.to_ne_bytes());
    bytes[4..8].copy_from_slice(&write_fd.to_ne_bytes());

    if let Err(err) = vm.copy_to_user(pipefd_ptr, &bytes) {
        return to_continue_unit(Err(err));
    }

    // Now insert them into the fd table.
    fd_table.insert(read_fd, read_descriptor);
    fd_table.insert(write_fd, write_descriptor);

    to_continue_unit(Ok(()))
}
