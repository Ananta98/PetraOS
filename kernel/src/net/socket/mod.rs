//! Socket Subsystem Core Dispatcher
//!
//! Provides the top-level `Socket` enum combining TCP, UDP, RAW, and UNIX domain sockets
//! with unified POSIX interface methods.

pub mod raw;
pub mod tcp;
pub mod udp;
pub mod unix;

pub use raw::RawSocket;
pub use tcp::TcpSocket;
pub use udp::UdpSocket;
pub use unix::UnixSocket;

use alloc::sync::Arc;
use core::mem::size_of;
use smoltcp::wire::IpEndpoint;

use crate::net::types::*;
use crate::sync::spinlock::Spinlock;
use crate::syscalls::SyscallError;

/// Unified Socket representation for PetraOS.
pub enum Socket {
    Tcp(TcpSocket),
    Udp(UdpSocket),
    Raw(RawSocket),
    Unix(Arc<Spinlock<UnixSocket>>),
}

impl Socket {
    /// Create a new socket based on family, type, and protocol.
    pub fn new(domain: i32, socket_type: i32, protocol: i32) -> Result<Self, SyscallError> {
        let actual_type = socket_type & SOCK_TYPE_MASK;
        let nonblocking = (socket_type & SOCK_NONBLOCK) != 0;

        match domain as u16 {
            AF_UNIX => match actual_type {
                SOCK_STREAM | SOCK_DGRAM | SOCK_SEQPACKET => {
                    let unix_sock = Arc::new(Spinlock::new(UnixSocket::new(
                        actual_type,
                        nonblocking,
                    )));
                    Ok(Socket::Unix(unix_sock))
                }
                _ => Err(SyscallError::ESOCKTNOSUPPORT),
            },
            AF_INET | AF_INET6 => match actual_type {
                SOCK_STREAM => {
                    if protocol != 0 && protocol != IPPROTO_TCP {
                        return Err(SyscallError::EPROTONOSUPPORT);
                    }
                    Ok(Socket::Tcp(TcpSocket::new(nonblocking)))
                }
                SOCK_DGRAM => {
                    if protocol != 0 && protocol != IPPROTO_UDP {
                        return Err(SyscallError::EPROTONOSUPPORT);
                    }
                    Ok(Socket::Udp(UdpSocket::new(nonblocking)))
                }
                SOCK_RAW => {
                    let proto = if protocol == 0 {
                        IPPROTO_ICMP as u8
                    } else {
                        protocol as u8
                    };
                    Ok(Socket::Raw(RawSocket::new(proto, nonblocking)))
                }
                _ => Err(SyscallError::ESOCKTNOSUPPORT),
            },
            _ => Err(SyscallError::EAFNOSUPPORT),
        }
    }

    /// Bind this socket to a local address.
    pub fn bind(
        &mut self,
        _self_arc: &Arc<Spinlock<Socket>>,
        addr: &SockAddrStorage,
        addr_len: usize,
    ) -> Result<(), SyscallError> {
        match self {
            Socket::Tcp(s) => {
                let ep = parse_ip_endpoint(addr, addr_len)?;
                s.bind(ep)
            }
            Socket::Udp(s) => {
                let ep = parse_ip_endpoint(addr, addr_len)?;
                s.bind(ep)
            }
            Socket::Raw(_) => Ok(()),
            Socket::Unix(u) => {
                if addr_len < size_of::<u16>() {
                    return Err(SyscallError::EINVAL);
                }
                // SAFETY: addr is a valid SockAddrStorage.
                let un = unsafe { &*(addr as *const _ as *const SockAddrUn) };
                let path = un.path();
                u.lock().bind(u, path)
            }
        }
    }

    /// Listen for incoming connections.
    pub fn listen(&mut self, backlog: usize) -> Result<(), SyscallError> {
        match self {
            Socket::Tcp(s) => s.listen(backlog),
            Socket::Unix(u) => u.lock().listen(backlog),
            _ => Err(SyscallError::EOPNOTSUPP),
        }
    }

    /// Accept a newly incoming connection.
    pub fn accept(
        &mut self,
        flags: i32,
    ) -> Result<(Arc<Spinlock<Socket>>, SockAddrStorage, usize), SyscallError> {
        let nonblocking = (flags & SOCK_NONBLOCK) != 0;

        match self {
            Socket::Tcp(s) => {
                let (conn, remote_ep) = s.accept(nonblocking)?;
                let (storage, len) = conv::endpoint_to_sockaddr_storage(remote_ep);
                Ok((conn, storage, len))
            }
            Socket::Unix(u) => {
                let conn = u.lock().accept(nonblocking)?;
                let mut storage = SockAddrStorage::default();
                storage.ss_family = AF_UNIX;
                Ok((conn, storage, size_of::<SockAddrUn>()))
            }
            _ => Err(SyscallError::EOPNOTSUPP),
        }
    }

    /// Connect to a remote address.
    pub fn connect(
        &mut self,
        _self_arc: &Arc<Spinlock<Socket>>,
        addr: &SockAddrStorage,
        addr_len: usize,
    ) -> Result<(), SyscallError> {
        match self {
            Socket::Tcp(s) => {
                let ep = parse_ip_endpoint(addr, addr_len)?;
                s.connect(ep)
            }
            Socket::Udp(s) => {
                let ep = parse_ip_endpoint(addr, addr_len)?;
                s.connect(ep)
            }
            Socket::Raw(_) => Ok(()),
            Socket::Unix(u) => {
                if addr_len < size_of::<u16>() {
                    return Err(SyscallError::EINVAL);
                }
                // SAFETY: addr is a valid SockAddrStorage.
                let un = unsafe { &*(addr as *const _ as *const SockAddrUn) };
                let path = un.path();
                u.lock().connect(u, path)
            }
        }
    }

    /// Send bytes over the socket.
    pub fn send(&mut self, buf: &[u8], flags: i32) -> Result<usize, SyscallError> {
        match self {
            Socket::Tcp(s) => s.send(buf, flags),
            Socket::Udp(s) => s.sendto(buf, None, flags),
            Socket::Raw(s) => s.send(buf, flags),
            Socket::Unix(u) => u.lock().send(buf, flags),
        }
    }

    /// Send a datagram or message to a specific remote address.
    pub fn sendto(
        &mut self,
        buf: &[u8],
        dest: Option<&SockAddrStorage>,
        dest_len: usize,
        flags: i32,
    ) -> Result<usize, SyscallError> {
        match self {
            Socket::Tcp(s) => s.send(buf, flags),
            Socket::Udp(s) => {
                let target_ep = if let Some(addr) = dest {
                    Some(parse_ip_endpoint(addr, dest_len)?)
                } else {
                    None
                };
                s.sendto(buf, target_ep, flags)
            }
            Socket::Raw(s) => s.send(buf, flags),
            Socket::Unix(u) => u.lock().send(buf, flags),
        }
    }

    /// Receive bytes from the socket.
    pub fn recv(&mut self, buf: &mut [u8], flags: i32) -> Result<usize, SyscallError> {
        match self {
            Socket::Tcp(s) => s.recv(buf, flags),
            Socket::Udp(s) => s.recvfrom(buf, flags).map(|(len, _)| len),
            Socket::Raw(s) => s.recv(buf, flags),
            Socket::Unix(u) => u.lock().recv(buf, flags),
        }
    }

    /// Receive datagram and return sender address metadata.
    pub fn recvfrom(
        &mut self,
        buf: &mut [u8],
        flags: i32,
    ) -> Result<(usize, SockAddrStorage, usize), SyscallError> {
        match self {
            Socket::Tcp(s) => {
                let bytes = s.recv(buf, flags)?;
                let remote = s.remote_endpoint.unwrap_or_else(|| {
                    IpEndpoint::new(
                        smoltcp::wire::IpAddress::Ipv4(smoltcp::wire::Ipv4Address::UNSPECIFIED),
                        0,
                    )
                });
                let (storage, len) = conv::endpoint_to_sockaddr_storage(remote);
                Ok((bytes, storage, len))
            }
            Socket::Udp(s) => {
                let (bytes, remote) = s.recvfrom(buf, flags)?;
                let (storage, len) = conv::endpoint_to_sockaddr_storage(remote);
                Ok((bytes, storage, len))
            }
            Socket::Raw(s) => {
                let bytes = s.recv(buf, flags)?;
                let mut storage = SockAddrStorage::default();
                storage.ss_family = AF_INET;
                Ok((bytes, storage, size_of::<SockAddrIn>()))
            }
            Socket::Unix(u) => {
                let bytes = u.lock().recv(buf, flags)?;
                let mut storage = SockAddrStorage::default();
                storage.ss_family = AF_UNIX;
                Ok((bytes, storage, size_of::<SockAddrUn>()))
            }
        }
    }

    /// Shutdown channels.
    pub fn shutdown(&mut self, how: i32) -> Result<(), SyscallError> {
        match self {
            Socket::Tcp(s) => s.shutdown(how),
            Socket::Unix(u) => u.lock().shutdown(how),
            _ => Ok(()),
        }
    }

    /// Return local socket name/endpoint.
    pub fn getsockname(&self) -> Result<(IpEndpoint, SockAddrStorage, usize), SyscallError> {
        match self {
            Socket::Tcp(s) => {
                let ep = s.local_endpoint.ok_or(SyscallError::EINVAL)?;
                let (storage, len) = conv::endpoint_to_sockaddr_storage(ep);
                Ok((ep, storage, len))
            }
            Socket::Udp(s) => {
                let ep = s.local_endpoint.ok_or(SyscallError::EINVAL)?;
                let (storage, len) = conv::endpoint_to_sockaddr_storage(ep);
                Ok((ep, storage, len))
            }
            Socket::Unix(u) => {
                let path = u.lock().bound_path.clone().unwrap_or_default();
                let un = SockAddrUn::new(&path);
                let mut storage = SockAddrStorage::default();
                // SAFETY: SockAddrUn is smaller than SockAddrStorage.
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        &un as *const _ as *const u8,
                        &mut storage as *mut _ as *mut u8,
                        size_of::<SockAddrUn>(),
                    );
                }
                let ep = IpEndpoint::new(smoltcp::wire::IpAddress::Ipv4(smoltcp::wire::Ipv4Address::UNSPECIFIED), 0);
                Ok((ep, storage, size_of::<SockAddrUn>()))
            }
            Socket::Raw(_) => Err(SyscallError::EOPNOTSUPP),
        }
    }

    /// Return remote peer name/endpoint.
    pub fn getpeername(&self) -> Result<(IpEndpoint, SockAddrStorage, usize), SyscallError> {
        match self {
            Socket::Tcp(s) => {
                let ep = s.remote_endpoint.ok_or(SyscallError::ENOTCONN)?;
                let (storage, len) = conv::endpoint_to_sockaddr_storage(ep);
                Ok((ep, storage, len))
            }
            Socket::Udp(s) => {
                let ep = s.remote_endpoint.ok_or(SyscallError::ENOTCONN)?;
                let (storage, len) = conv::endpoint_to_sockaddr_storage(ep);
                Ok((ep, storage, len))
            }
            Socket::Unix(u) => {
                let mut storage = SockAddrStorage::default();
                storage.ss_family = AF_UNIX;
                let ep = IpEndpoint::new(smoltcp::wire::IpAddress::Ipv4(smoltcp::wire::Ipv4Address::UNSPECIFIED), 0);
                Ok((ep, storage, size_of::<SockAddrUn>()))
            }
            Socket::Raw(_) => Err(SyscallError::ENOTCONN),
        }
    }

    pub fn poll_read_ready(&self) -> bool {
        match self {
            Socket::Tcp(s) => s.poll_read_ready(),
            Socket::Udp(s) => s.poll_read_ready(),
            Socket::Raw(s) => s.poll_read_ready(),
            Socket::Unix(u) => u.lock().poll_read_ready(),
        }
    }

    pub fn poll_write_ready(&self) -> bool {
        match self {
            Socket::Tcp(s) => s.poll_write_ready(),
            Socket::Udp(s) => s.poll_write_ready(),
            Socket::Raw(s) => s.poll_write_ready(),
            Socket::Unix(u) => u.lock().poll_write_ready(),
        }
    }
}

/// Helper function to parse an `IpEndpoint` from `SockAddrStorage`.
pub fn parse_ip_endpoint(
    addr: &SockAddrStorage,
    addr_len: usize,
) -> Result<IpEndpoint, SyscallError> {
    if addr_len < size_of::<u16>() {
        return Err(SyscallError::EINVAL);
    }

    match addr.ss_family {
        AF_INET => {
            if addr_len < size_of::<SockAddrIn>() {
                return Err(SyscallError::EINVAL);
            }
            // SAFETY: Verified size >= SockAddrIn.
            let sin = unsafe { &*(addr as *const _ as *const SockAddrIn) };
            let ip = conv::in_addr_to_smoltcp(sin.sin_addr);
            Ok(IpEndpoint::new(smoltcp::wire::IpAddress::Ipv4(ip), sin.port()))
        }
        AF_INET6 => {
            if addr_len < size_of::<SockAddrIn6>() {
                return Err(SyscallError::EINVAL);
            }
            // SAFETY: Verified size >= SockAddrIn6.
            let sin6 = unsafe { &*(addr as *const _ as *const SockAddrIn6) };
            let ip = conv::in6_addr_to_smoltcp(sin6.sin6_addr);
            Ok(IpEndpoint::new(smoltcp::wire::IpAddress::Ipv6(ip), sin6.port()))
        }
        _ => Err(SyscallError::EAFNOSUPPORT),
    }
}
