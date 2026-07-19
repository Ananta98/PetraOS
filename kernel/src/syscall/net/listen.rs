use crate::proc::process::Process;
use crate::syscall::SyscallResult;
use crate::syscall::net::{SocketFile, allocate_ephemeral_port};
use crate::syscall::to_continue_unit;
use crate::vm::vma::VmaManager;
use ostd::Error;
use smoltcp::wire::IpEndpoint;

pub fn syscall_listen(
    arg0: usize, // sockfd
    arg1: usize, // backlog
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _vm: &VmaManager,
    _: &mut ostd::arch::cpu::context::UserContext,
) -> SyscallResult {
    let sockfd = arg0 as i32;

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

    if socket_file.socket_type != 1 {
        return to_continue_unit(Err(Error::InvalidArgs));
    }

    let mut local_endpoint = *socket_file.local.lock();
    if local_endpoint.is_none() {
        let ep = IpEndpoint::new(
            smoltcp::wire::IpAddress::Ipv4(smoltcp::wire::Ipv4Address::UNSPECIFIED),
            allocate_ephemeral_port(),
        );
        *socket_file.local.lock() = Some(ep);
        local_endpoint = Some(ep);
    }

    let local_ep = local_endpoint.unwrap();

    let mut stack_guard = crate::net::NET_STACK.lock();
    let stack = match stack_guard.as_mut() {
        Some(s) => s,
        None => return to_continue_unit(Err(Error::InvalidArgs)),
    };
    let sockets = &mut stack.sockets;

    let tcp_socket = sockets.get_mut::<smoltcp::socket::tcp::Socket>(*socket_file.handle.lock());

    if let Err(_) = tcp_socket.listen(local_ep) {
        return to_continue_unit(Err(Error::InvalidArgs));
    }

    to_continue_unit(Ok(()))
}
