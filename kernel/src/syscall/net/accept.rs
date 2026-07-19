use crate::proc::process::Process;
use crate::syscall::SyscallResult;
use crate::syscall::to_continue;
use crate::vm::vma::VmaManager;
use crate::syscall::net::SocketFile;
use crate::fs::fd_table::FileDescriptor;
use ostd::Error;
use smoltcp::socket::tcp::State;
use ostd::sync::SpinLock;
use alloc::boxed::Box;

pub fn syscall_accept(
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
    let addrlen_ptr = arg2;

    let process = Process::current();
    let fd_table = process.fd_table.lock();
    let fd_entry = match fd_table.get_fd(sockfd) {
        Ok(entry) => entry,
        Err(err) => return to_continue(Err(err)),
    };

    let open_file = fd_entry.open_file.lock();
    let socket_file = match open_file.file_ops.as_any()
        .and_then(|any| any.downcast_ref::<SocketFile>())
    {
        Some(sf) => sf,
        None => return to_continue(Err(Error::InvalidArgs)),
    };

    if socket_file.socket_type != 1 {
        return to_continue(Err(Error::InvalidArgs));
    }

    let listener_handle = *socket_file.handle.lock();
    let local_endpoint = match *socket_file.local.lock() {
        Some(ep) => ep,
        None => return to_continue(Err(Error::InvalidArgs)),
    };

    let mut remote_ep = None;

    // Poll interface until a connection is established on the listener socket
    loop {
            let mut stack_guard = crate::net::NET_STACK.lock();
            if let Some(stack) = stack_guard.as_mut() {
                let tcp_socket = stack.sockets.get_mut::<smoltcp::socket::tcp::Socket>(listener_handle);
                if tcp_socket.state() == State::Established {
                    if let Some(ep) = tcp_socket.remote_endpoint() {
                        remote_ep = Some(ep);
                        break;
                    }
                }
            }
        crate::net::poll();
    }

    let remote_endpoint = remote_ep.unwrap();

    // Create a new TCP socket that will take over the listening port
    let new_listener_handle = {
        let mut stack_guard = crate::net::NET_STACK.lock();
        let stack = match stack_guard.as_mut() {
            Some(s) => s,
            None => return to_continue(Err(Error::InvalidArgs)),
        };
        let sockets = &mut stack.sockets;
        let rx_data = alloc::vec![0u8; 8192];
        let tx_data = alloc::vec![0u8; 8192];
        let rx_buffer = smoltcp::socket::tcp::SocketBuffer::new(rx_data);
        let tx_buffer = smoltcp::socket::tcp::SocketBuffer::new(tx_data);
        let mut new_tcp = smoltcp::socket::tcp::Socket::new(rx_buffer, tx_buffer);
        if let Err(_) = new_tcp.listen(local_endpoint) {
            return to_continue(Err(Error::InvalidArgs));
        }
        sockets.add(new_tcp)
    };

    // Swap the handles: the existing listener socket file gets the new handle,
    // and the original listener socket (which is now established) is returned as the new fd!
    let connected_handle = listener_handle;
    *socket_file.handle.lock() = new_listener_handle;

    // Create the new SocketFile for the connected socket
    let connected_socket_file = SocketFile {
        handle: SpinLock::new(connected_handle),
        domain: socket_file.domain,
        socket_type: socket_file.socket_type,
        protocol: socket_file.protocol,
        local: SpinLock::new(Some(local_endpoint)),
        remote: SpinLock::new(Some(remote_endpoint)),
    };

    // Drop locks to avoid deadlock when modifying the fd table
    drop(open_file);
    drop(fd_table);

    let mut fd_table = process.fd_table.lock();
    let new_fd = match fd_table.alloc_fd(0) {
        Ok(fd) => fd,
        Err(err) => {
            // Cleanup the connected socket from smoltcp if we can't allocate fd
            let mut stack_guard = crate::net::NET_STACK.lock();
            if let Some(stack) = stack_guard.as_mut() {
                stack.sockets.remove(connected_handle);
            }
            return to_continue(Err(err));
        }
    };

    let fd_entry = FileDescriptor::new(Box::new(connected_socket_file), 0);
    fd_table.insert(new_fd, fd_entry);

    // Copy peer address to user space if requested
    if addr_ptr != 0 && addrlen_ptr != 0 {
        let mut user_addrlen = 0usize;
        let mut addrlen_buf = [0u8; core::mem::size_of::<usize>()];
        if vm.copy_from_user(addrlen_ptr, &mut addrlen_buf).is_ok() {
            user_addrlen = usize::from_ne_bytes(addrlen_buf);
        }

        if user_addrlen > 0 {
            let port_be = remote_endpoint.port.to_be();
            match remote_endpoint.addr {
                smoltcp::wire::IpAddress::Ipv4(ipv4) => {
                    let mut buf = [0u8; 16];
                    buf[0..2].copy_from_slice(&2u16.to_ne_bytes()); // sin_family (AF_INET)
                    buf[2..4].copy_from_slice(&port_be.to_ne_bytes()); // sin_port
                    buf[4..8].copy_from_slice(&ipv4.0); // sin_addr

                    let write_len = core::cmp::min(user_addrlen, 16);
                    let _ = vm.copy_to_user(addr_ptr, &buf[..write_len]);
                    let _ = vm.copy_to_user(addrlen_ptr, &16usize.to_ne_bytes());
                }
                smoltcp::wire::IpAddress::Ipv6(ipv6) => {
                    let mut buf = [0u8; 28];
                    buf[0..2].copy_from_slice(&10u16.to_ne_bytes()); // sin6_family (AF_INET6)
                    buf[2..4].copy_from_slice(&port_be.to_ne_bytes()); // sin6_port
                    buf[4..8].copy_from_slice(&0u32.to_ne_bytes()); // sin6_flowinfo
                    buf[8..24].copy_from_slice(&ipv6.0); // sin6_addr
                    buf[24..28].copy_from_slice(&0u32.to_ne_bytes()); // sin6_scope_id

                    let write_len = core::cmp::min(user_addrlen, 28);
                    let _ = vm.copy_to_user(addr_ptr, &buf[..write_len]);
                    let _ = vm.copy_to_user(addrlen_ptr, &28usize.to_ne_bytes());
                }
                _ => {}
            }
        }
    }

    to_continue(Ok(new_fd as usize))
}
