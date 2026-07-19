use crate::proc::process::Process;
use crate::syscall::SyscallResult;
use crate::syscall::net::{SocketFile, allocate_ephemeral_port, parse_sockaddr};
use crate::syscall::to_continue_unit;
use crate::vm::vma::VmaManager;
use ostd::Error;

pub fn syscall_bind(
    arg0: usize, // sockfd
    arg1: usize, // addr
    arg2: usize, // addrlen
    _: usize,
    _: usize,
    _: usize,
    vm: &VmaManager,
    _: &mut ostd::arch::cpu::context::UserContext,
) -> SyscallResult {
    let sockfd = arg0 as i32;
    let addr_ptr = arg1;
    let addrlen = arg2;

    let mut endpoint = match parse_sockaddr(vm, addr_ptr, addrlen) {
        Ok(ep) => ep,
        Err(err) => return to_continue_unit(Err(err)),
    };

    if endpoint.port == 0 {
        endpoint.port = allocate_ephemeral_port();
    }

    let process = Process::current();
    let fd_table = process.fd_table.lock();
    let fd_entry = match fd_table.get_fd(sockfd) {
        Ok(entry) => entry,
        Err(err) => return to_continue_unit(Err(err)),
    };

    let open_file = fd_entry.open_file.lock();
    let socket_file = match open_file
        .file_ops
        .as_any()
        .and_then(|any| any.downcast_ref::<SocketFile>())
    {
        Some(sf) => sf,
        None => return to_continue_unit(Err(Error::InvalidArgs)),
    };

    *socket_file.local.lock() = Some(endpoint);

    let mut stack_guard = crate::net::NET_STACK.lock();
    let stack = match stack_guard.as_mut() {
        Some(s) => s,
        None => return to_continue_unit(Err(Error::InvalidArgs)),
    };
    let sockets = &mut stack.sockets;

    if socket_file.socket_type == 2 {
        let udp = sockets.get_mut::<smoltcp::socket::udp::Socket>(*socket_file.handle.lock());
        if let Err(_) = udp.bind(endpoint) {
            return to_continue_unit(Err(Error::InvalidArgs));
        }
    }

    to_continue_unit(Ok(()))
}
