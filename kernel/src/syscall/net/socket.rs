use crate::fs::fd_table::FileDescriptor;
use crate::proc::process::Process;
use crate::syscall::SyscallResult;
use crate::syscall::net::SocketFile;
use crate::syscall::to_continue;
use crate::vm::vma::VmaManager;
use alloc::boxed::Box;
use ostd::Error;
use ostd::sync::SpinLock;

pub fn syscall_socket(
    arg0: usize,
    arg1: usize,
    arg2: usize,
    _: usize,
    _: usize,
    _: usize,
    _vm: &VmaManager,
    _: &mut ostd::arch::cpu::context::UserContext,
) -> SyscallResult {
    let domain = arg0 as i32;
    let socket_type = arg1 as i32;
    let protocol = arg2 as i32;

    if domain != 2 && domain != 10 {
        return to_continue(Err(Error::InvalidArgs));
    }

    if socket_type != 1 && socket_type != 2 {
        return to_continue(Err(Error::InvalidArgs));
    }

    let handle = {
        let mut stack_guard = crate::net::NET_STACK.lock();
        let stack = match stack_guard.as_mut() {
            Some(s) => s,
            None => return to_continue(Err(Error::InvalidArgs)),
        };
        let sockets = &mut stack.sockets;

        if socket_type == 1 {
            let rx_data = alloc::vec![0u8; 8192];
            let tx_data = alloc::vec![0u8; 8192];
            let rx_buffer = smoltcp::socket::tcp::SocketBuffer::new(rx_data);
            let tx_buffer = smoltcp::socket::tcp::SocketBuffer::new(tx_data);
            let tcp_socket = smoltcp::socket::tcp::Socket::new(rx_buffer, tx_buffer);
            sockets.add(tcp_socket)
        } else {
            let rx_meta = alloc::vec![smoltcp::socket::udp::PacketMetadata::EMPTY; 16];
            let rx_data = alloc::vec![0u8; 16384];
            let tx_meta = alloc::vec![smoltcp::socket::udp::PacketMetadata::EMPTY; 16];
            let tx_data = alloc::vec![0u8; 16384];
            let rx_buffer = smoltcp::socket::udp::PacketBuffer::new(rx_meta, rx_data);
            let tx_buffer = smoltcp::socket::udp::PacketBuffer::new(tx_meta, tx_data);
            let udp_socket = smoltcp::socket::udp::Socket::new(rx_buffer, tx_buffer);
            sockets.add(udp_socket)
        }
    };

    let socket_file = SocketFile {
        handle: SpinLock::new(handle),
        domain,
        socket_type,
        protocol,
        local: SpinLock::new(None),
        remote: SpinLock::new(None),
    };

    let process = Process::current();
    let mut fd_table = process.fd_table.lock();
    let fd = match fd_table.alloc_fd(0) {
        Ok(fd) => fd,
        Err(err) => {
            if let Some(stack) = crate::net::NET_STACK.lock().as_mut() {
                stack.sockets.remove(handle);
            }
            return to_continue(Err(err));
        }
    };

    let fd_entry = FileDescriptor::new(Box::new(socket_file), 0);
    fd_table.insert(fd, fd_entry);

    to_continue(Ok(fd as usize))
}
