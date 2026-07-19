use crate::proc::process::Process;
use crate::syscall::SyscallResult;
use crate::syscall::net::{SocketFile, allocate_ephemeral_port, parse_sockaddr};
use crate::syscall::to_continue_unit;
use crate::vm::vma::VmaManager;
use ostd::Error;
use smoltcp::socket::tcp::State;
use smoltcp::wire::IpEndpoint;

pub fn syscall_connect(
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

    let remote_endpoint = match parse_sockaddr(vm, addr_ptr, addrlen) {
        Ok(ep) => ep,
        Err(err) => return to_continue_unit(Err(err)),
    };

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

    *socket_file.remote.lock() = Some(remote_endpoint);

    // If UDP, we are done
    if socket_file.socket_type == 2 {
        return to_continue_unit(Ok(()));
    }

    // TCP connect handshake
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
    let interface = &mut stack.interface;
    let sockets = &mut stack.sockets;

    let tcp_socket = sockets.get_mut::<smoltcp::socket::tcp::Socket>(*socket_file.handle.lock());

    let context = interface.context();
    if let Err(_) = tcp_socket.connect(context, remote_endpoint, local_ep) {
        return to_continue_unit(Err(Error::IoError));
    }

    // Drop locks before polling to avoid deadlocks
    drop(stack_guard);

    // Poll until established or error
    for _ in 0..100 {
        crate::net::poll();

        let mut stack_guard = crate::net::NET_STACK.lock();
        if let Some(stack) = stack_guard.as_mut() {
            let tcp_socket = stack
                .sockets
                .get_mut::<smoltcp::socket::tcp::Socket>(*socket_file.handle.lock());
            if tcp_socket.state() == State::Established {
                return to_continue_unit(Ok(()));
            }
        }
    }

    to_continue_unit(Err(Error::IoError))
}
