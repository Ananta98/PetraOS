pub mod accept;
pub mod bind;
pub mod connect;
pub mod listen;
pub mod recvfrom;
pub mod sendto;
pub mod socket;

pub use accept::{syscall_accept, syscall_accept4};
pub use bind::syscall_bind;
pub use connect::syscall_connect;
pub use listen::syscall_listen;
pub use recvfrom::syscall_recvfrom;
pub use sendto::syscall_sendto;
pub use socket::syscall_socket;

// Re-export SocketFile from the fs layer where it logically belongs.
pub use crate::fs::socketfs::SocketFile;

use ostd::Error;
use ostd::sync::SpinLock;
use smoltcp::wire::{IpAddress, IpEndpoint, Ipv4Address, Ipv6Address};

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
