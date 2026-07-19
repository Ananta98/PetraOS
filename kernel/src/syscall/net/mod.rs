pub mod socket;
pub mod connect;
pub mod accept;
pub mod bind;
pub mod listen;
pub mod sendto;
pub mod recvfrom;

pub use socket::syscall_socket;
pub use connect::syscall_connect;
pub use accept::syscall_accept;
pub use bind::syscall_bind;
pub use listen::syscall_listen;
pub use sendto::syscall_sendto;
pub use recvfrom::syscall_recvfrom;

use crate::fs::vfs::{FileOps, SeekFrom};
use smoltcp::iface::SocketHandle;
use ostd::sync::SpinLock;
use smoltcp::wire::{IpAddress, IpEndpoint, Ipv4Address, Ipv6Address};
use ostd::Error;

pub struct SocketFile {
    pub handle: SpinLock<SocketHandle>,
    pub domain: i32,
    pub socket_type: i32,
    pub protocol: i32,
    pub local: SpinLock<Option<IpEndpoint>>,
    pub remote: SpinLock<Option<IpEndpoint>>,
}

impl FileOps for SocketFile {
    fn read(&mut self, buf: &mut [u8], _offset: &mut usize) -> Result<usize, Error> {
        let mut stack_guard = crate::net::NET_STACK.lock();
        let stack = stack_guard.as_mut().ok_or(Error::InvalidArgs)?;
        let sockets = &mut stack.sockets;

        let handle = *self.handle.lock();
        if self.socket_type == 1 {
            let tcp_socket = sockets.get_mut::<smoltcp::socket::tcp::Socket>(handle);
            if !tcp_socket.is_active() && !tcp_socket.may_recv() {
                return Err(Error::IoError);
            }
            tcp_socket.recv_slice(buf).map_err(|_| Error::IoError)
        } else if self.socket_type == 2 {
            let udp_socket = sockets.get_mut::<smoltcp::socket::udp::Socket>(handle);
            udp_socket.recv_slice(buf)
                .map(|(len, _)| len)
                .map_err(|_| Error::IoError)
        } else {
            Err(Error::InvalidArgs)
        }
    }

    fn write(&mut self, buf: &[u8], _offset: &mut usize) -> Result<usize, Error> {
        let mut stack_guard = crate::net::NET_STACK.lock();
        let stack = stack_guard.as_mut().ok_or(Error::InvalidArgs)?;
        let sockets = &mut stack.sockets;

        let handle = *self.handle.lock();
        if self.socket_type == 1 {
            let tcp_socket = sockets.get_mut::<smoltcp::socket::tcp::Socket>(handle);
            if !tcp_socket.is_active() && !tcp_socket.may_send() {
                return Err(Error::IoError);
            }
            tcp_socket.send_slice(buf).map_err(|_| Error::IoError)
        } else if self.socket_type == 2 {
            let udp_socket = sockets.get_mut::<smoltcp::socket::udp::Socket>(handle);
            let remote = self.remote.lock();
            if let Some(dest) = *remote {
                udp_socket.send_slice(buf, dest)
                    .map(|_| buf.len())
                    .map_err(|_| Error::IoError)
            } else {
                Err(Error::InvalidArgs)
            }
        } else {
            Err(Error::InvalidArgs)
        }
    }

    fn seek(&mut self, _pos: SeekFrom, _offset: &mut usize) -> Result<usize, Error> {
        Err(Error::InvalidArgs)
    }

    fn readdir(&mut self) -> Result<alloc::vec::Vec<crate::fs::vfs::DirEntry>, Error> {
        Err(Error::InvalidArgs)
    }

    fn as_any(&self) -> Option<&dyn core::any::Any> {
        Some(self)
    }
}

impl Drop for SocketFile {
    fn drop(&mut self) {
        let mut stack_guard = crate::net::NET_STACK.lock();
        if let Some(stack) = stack_guard.as_mut() {
            stack.sockets.remove(*self.handle.lock());
        }
    }
}

static EPHEMERAL_PORT: SpinLock<u16> = SpinLock::new(49152);

pub fn allocate_ephemeral_port() -> u16 {
    let mut port = EPHEMERAL_PORT.lock();
    let res = *port;
    *port = if *port == 65535 { 49152 } else { *port + 1 };
    res
}

pub fn parse_sockaddr(
    vm: &crate::vm::vma::VmaManager,
    addr_ptr: usize,
    addrlen: usize,
) -> Result<IpEndpoint, Error> {
    if addr_ptr == 0 || addrlen < 2 {
        return Err(Error::InvalidArgs);
    }
    let mut family_buf = [0u8; 2];
    vm.copy_from_user(addr_ptr, &mut family_buf)?;
    let family = u16::from_ne_bytes(family_buf);

    if family == 2 {
        // AF_INET
        if addrlen < 16 {
            return Err(Error::InvalidArgs);
        }
        let mut buf = [0u8; 16];
        vm.copy_from_user(addr_ptr, &mut buf)?;
        
        let mut port_bytes = [0u8; 2];
        port_bytes.copy_from_slice(&buf[2..4]);
        let port = u16::from_be_bytes(port_bytes);

        let mut ip_bytes = [0u8; 4];
        ip_bytes.copy_from_slice(&buf[4..8]);
        let ip = IpAddress::Ipv4(Ipv4Address::from_bytes(&ip_bytes));
        
        Ok(IpEndpoint::new(ip, port))
    } else if family == 10 {
        // AF_INET6
        if addrlen < 28 {
            return Err(Error::InvalidArgs);
        }
        let mut buf = [0u8; 28];
        vm.copy_from_user(addr_ptr, &mut buf)?;

        let mut port_bytes = [0u8; 2];
        port_bytes.copy_from_slice(&buf[2..4]);
        let port = u16::from_be_bytes(port_bytes);

        let mut ip_bytes = [0u8; 16];
        ip_bytes.copy_from_slice(&buf[8..24]);
        let ip = IpAddress::Ipv6(Ipv6Address::from_bytes(&ip_bytes));

        Ok(IpEndpoint::new(ip, port))
    } else {
        Err(Error::InvalidArgs)
    }
}
