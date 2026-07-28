use crate::proc::process::Process;
use crate::syscall::{SyscallResult, to_continue_i32};
use crate::vm::vma::VmaManager;
use ostd::Error;
use ostd::arch::cpu::context::UserContext;

/// `socketpair()` — SYS_socketpair = 53
pub fn syscall_socketpair(
    _domain: usize,
    _type: usize,
    _protocol: usize,
    sv_ptr: usize,
    _: usize,
    _: usize,
    vm: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    if sv_ptr == 0 {
        return to_continue_i32(Err(Error::InvalidArgs));
    }

    let (reader, writer) = crate::ipc::create_pipe();
    let proc = Process::current();
    let mut fd_table = proc.fd_table.lock();

    let fd0 = match fd_table.insert_pipe_reader(reader) {
        Ok(f) => f,
        Err(e) => return to_continue_i32(Err(e)),
    };

    let fd1 = match fd_table.insert_pipe_writer(writer) {
        Ok(f) => f,
        Err(e) => return to_continue_i32(Err(e)),
    };

    let fds: [i32; 2] = [fd0, fd1];
    let mut buf = [0u8; 8];
    buf[0..4].copy_from_slice(&fds[0].to_ne_bytes());
    buf[4..8].copy_from_slice(&fds[1].to_ne_bytes());

    to_continue_i32(vm.copy_to_user(sv_ptr, &buf).map(|_| 0))
}
