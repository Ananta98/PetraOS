use crate::proc::process::Process;
use crate::syscall::SyscallResult;
use crate::syscall::net::{SocketFile, parse_sockaddr};
use crate::syscall::to_continue;
use crate::vm::vma::VmaManager;
use ostd::Error;

pub fn syscall_sendto(
    arg0: usize, // sockfd
    arg1: usize, // buf
    arg2: usize, // len
    arg3: usize, // flags
    arg4: usize, // dest_addr
    arg5: usize, // addrlen
    vm: &VmaManager,
    _: &mut ostd::arch::cpu::context::UserContext,
) -> SyscallResult {
    let sockfd = arg0 as i32;
    let buf_ptr = arg1;
    let len = arg2;
    let _flags = arg3;
    let dest_addr_ptr = arg4;
    let addrlen = arg5;

    let mut kbuf = alloc::vec![0u8; len];
    if let Err(err) = vm.copy_from_user(buf_ptr, &mut kbuf) {
        return to_continue(Err(err));
    }

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

    if socket_file.socket_type == 1 {
        // TCP
        if dest_addr_ptr != 0 {
            return to_continue(Err(Error::InvalidArgs));
        }
        let tcp = sockets.get_mut::<smoltcp::socket::tcp::Socket>(handle);
        if !tcp.is_active() && !tcp.may_send() {
            return to_continue(Err(Error::IoError));
        }
        let written = match tcp.send_slice(&kbuf) {
            Ok(w) => w,
            Err(_) => return to_continue(Err(Error::IoError)),
        };
        to_continue(Ok(written))
    } else {
        // UDP
        let dest = if dest_addr_ptr != 0 {
            match parse_sockaddr(vm, dest_addr_ptr, addrlen) {
                Ok(ep) => Some(ep),
                Err(err) => return to_continue(Err(err)),
            }
        } else {
            *socket_file.remote.lock()
        };

        let dest_ep = match dest {
            Some(ep) => ep,
            None => return to_continue(Err(Error::InvalidArgs)),
        };

        let udp = sockets.get_mut::<smoltcp::socket::udp::Socket>(handle);

        if let Err(_) = udp.send_slice(&kbuf, dest_ep) {
            return to_continue(Err(Error::IoError));
        }
        to_continue(Ok(len))
    }
}
