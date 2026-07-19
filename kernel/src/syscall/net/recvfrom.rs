use crate::proc::process::Process;
use crate::syscall::SyscallResult;
use crate::syscall::net::SocketFile;
use crate::syscall::to_continue;
use crate::vm::vma::VmaManager;
use ostd::Error;

pub fn syscall_recvfrom(
    arg0: usize, // sockfd
    arg1: usize, // buf
    arg2: usize, // len
    arg3: usize, // flags
    arg4: usize, // src_addr
    arg5: usize, // addrlen
    vm: &VmaManager,
    _: &mut ostd::arch::cpu::context::UserContext,
) -> SyscallResult {
    let sockfd = arg0 as i32;
    let buf_ptr = arg1;
    let len = arg2;
    let _flags = arg3;
    let src_addr_ptr = arg4;
    let addrlen_ptr = arg5;

    let mut kbuf = alloc::vec![0u8; len];

    let process = Process::current();
    let fd_table = process.fd_table.lock();
    let fd_entry = match fd_table.get_fd(sockfd) {
        Ok(entry) => entry,
        Err(err) => return to_continue(Err(err)),
    };

    let open_file = fd_entry.open_file.lock();
    let socket_file = match open_file
        .file_ops
        .as_any()
        .and_then(|any| any.downcast_ref::<SocketFile>())
    {
        Some(sf) => sf,
        None => return to_continue(Err(Error::InvalidArgs)),
    };

    let handle = *socket_file.handle.lock();

    let mut stack_guard = crate::net::NET_STACK.lock();
    let stack = match stack_guard.as_mut() {
        Some(s) => s,
        None => return to_continue(Err(Error::InvalidArgs)),
    };
    let sockets = &mut stack.sockets;

    let mut remote_endpoint = None;
    let bytes_read = if socket_file.socket_type == 1 {
        // TCP
        let tcp = sockets.get_mut::<smoltcp::socket::tcp::Socket>(handle);
        if !tcp.is_active() && !tcp.may_recv() {
            return to_continue(Err(Error::IoError));
        }
        let r = match tcp.recv_slice(&mut kbuf) {
            Ok(read) => read,
            Err(_) => return to_continue(Err(Error::IoError)),
        };
        if tcp.is_active() {
            remote_endpoint = tcp.remote_endpoint();
        }
        r
    } else {
        // UDP
        let udp = sockets.get_mut::<smoltcp::socket::udp::Socket>(handle);
        let (r, rx_meta) = match udp.recv_slice(&mut kbuf) {
            Ok(res) => res,
            Err(_) => return to_continue(Err(Error::IoError)),
        };
        remote_endpoint = Some(rx_meta.endpoint);
        r
    };

    if vm.copy_to_user(buf_ptr, &kbuf[..bytes_read]).is_err() {
        return to_continue(Err(Error::AccessDenied));
    }

    // Write source address to user space if requested
    if src_addr_ptr != 0 && addrlen_ptr != 0 {
        if let Some(endpoint) = remote_endpoint {
            let mut user_addrlen = 0usize;
            let mut addrlen_buf = [0u8; core::mem::size_of::<usize>()];
            if vm.copy_from_user(addrlen_ptr, &mut addrlen_buf).is_ok() {
                user_addrlen = usize::from_ne_bytes(addrlen_buf);
            }

            if user_addrlen > 0 {
                let port_be = endpoint.port.to_be();
                match endpoint.addr {
                    smoltcp::wire::IpAddress::Ipv4(ipv4) => {
                        let mut buf = [0u8; 16];
                        buf[0..2].copy_from_slice(&2u16.to_ne_bytes()); // sin_family (AF_INET)
                        buf[2..4].copy_from_slice(&port_be.to_ne_bytes()); // sin_port
                        buf[4..8].copy_from_slice(&ipv4.0); // sin_addr

                        let write_len = core::cmp::min(user_addrlen, 16);
                        let _ = vm.copy_to_user(src_addr_ptr, &buf[..write_len]);
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
                        let _ = vm.copy_to_user(src_addr_ptr, &buf[..write_len]);
                        let _ = vm.copy_to_user(addrlen_ptr, &28usize.to_ne_bytes());
                    }
                    _ => {}
                }
            }
        }
    }

    to_continue(Ok(bytes_read))
}
