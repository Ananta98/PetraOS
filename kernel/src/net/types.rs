//! Network and Socket POSIX ABI Definitions
//!
//! Provides standard Linux/POSIX socket address structures, constants, options,
//! and conversion helpers for IPv4, IPv6, and UNIX domain sockets.

use core::mem::size_of;

// ===== Address Families =====

pub const AF_UNSPEC: u16 = 0;
pub const AF_UNIX: u16 = 1;
pub const AF_LOCAL: u16 = 1;
pub const AF_INET: u16 = 2;
pub const AF_INET6: u16 = 10;
pub const AF_NETLINK: u16 = 16;
pub const AF_PACKET: u16 = 17;

// ===== Socket Types & Flags =====

pub const SOCK_STREAM: i32 = 1;
pub const SOCK_DGRAM: i32 = 2;
pub const SOCK_RAW: i32 = 3;
pub const SOCK_RDM: i32 = 4;
pub const SOCK_SEQPACKET: i32 = 5;
pub const SOCK_DCCP: i32 = 6;
pub const SOCK_PACKET: i32 = 10;

pub const SOCK_TYPE_MASK: i32 = 0xFF;
pub const SOCK_NONBLOCK: i32 = 0o4000; // 0x800
pub const SOCK_CLOEXEC: i32 = 0o2000000; // 0x80000

// ===== IP Protocols =====

pub const IPPROTO_IP: i32 = 0;
pub const IPPROTO_ICMP: i32 = 1;
pub const IPPROTO_IGMP: i32 = 2;
pub const IPPROTO_TCP: i32 = 6;
pub const IPPROTO_UDP: i32 = 17;
pub const IPPROTO_IPV6: i32 = 41;
pub const IPPROTO_ICMPV6: i32 = 58;
pub const IPPROTO_RAW: i32 = 255;

// ===== Socket Option Levels & Options =====

pub const SOL_SOCKET: i32 = 1;
pub const SOL_IP: i32 = 0;
pub const SOL_IPV6: i32 = 41;
pub const SOL_TCP: i32 = 6;
pub const SOL_UDP: i32 = 17;

pub const SO_DEBUG: i32 = 1;
pub const SO_REUSEADDR: i32 = 2;
pub const SO_TYPE: i32 = 3;
pub const SO_ERROR: i32 = 4;
pub const SO_DONTROUTE: i32 = 5;
pub const SO_BROADCAST: i32 = 6;
pub const SO_SNDBUF: i32 = 7;
pub const SO_RCVBUF: i32 = 8;
pub const SO_KEEPALIVE: i32 = 9;
pub const SO_OOBINLINE: i32 = 10;
pub const SO_NO_CHECK: i32 = 11;
pub const SO_PRIORITY: i32 = 12;
pub const SO_LINGER: i32 = 13;
pub const SO_BSDCOMPAT: i32 = 14;
pub const SO_REUSEPORT: i32 = 15;
pub const SO_PASSCRED: i32 = 16;
pub const SO_PEERCRED: i32 = 17;
pub const SO_RCVLOWAT: i32 = 18;
pub const SO_SNDLOWAT: i32 = 19;
pub const SO_RCVTIMEO: i32 = 20;
pub const SO_SNDTIMEO: i32 = 21;
pub const SO_BINDTODEVICE: i32 = 25;
pub const SO_ACCEPTCONN: i32 = 30;

pub const TCP_NODELAY: i32 = 1;
pub const TCP_MAXSEG: i32 = 2;
pub const TCP_CORK: i32 = 3;
pub const TCP_KEEPIDLE: i32 = 4;
pub const TCP_KEEPINTVL: i32 = 5;
pub const TCP_KEEPCNT: i32 = 6;

// ===== Shutdown Flags =====

pub const SHUT_RD: i32 = 0;
pub const SHUT_WR: i32 = 1;
pub const SHUT_RDWR: i32 = 2;

// ===== Message Flags (send/recv) =====

pub const MSG_OOB: i32 = 0x01;
pub const MSG_PEEK: i32 = 0x02;
pub const MSG_DONTROUTE: i32 = 0x04;
pub const MSG_CTRUNC: i32 = 0x08;
pub const MSG_TRUNC: i32 = 0x20;
pub const MSG_DONTWAIT: i32 = 0x40;
pub const MSG_EOR: i32 = 0x80;
pub const MSG_WAITALL: i32 = 0x100;
pub const MSG_NOSIGNAL: i32 = 0x4000;
pub const MSG_MORE: i32 = 0x8000;

// ===== Socket Address Structures =====

pub type SockLen = u32;

/// Generic socket address structure (`struct sockaddr`).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SockAddr {
    pub sa_family: u16,
    pub sa_data: [u8; 14],
}

/// IPv4 Internet address (`struct in_addr`).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InAddr {
    /// 32-bit IPv4 address in network byte order (big-endian).
    pub s_addr: u32,
}

impl InAddr {
    pub const ANY: Self = Self { s_addr: 0 };
    pub const LOOPBACK: Self = Self {
        s_addr: u32::from_be(0x7F000001),
    };
    pub const BROADCAST: Self = Self {
        s_addr: 0xFFFF_FFFF,
    };

    pub const fn new(a: u8, b: u8, c: u8, d: u8) -> Self {
        Self {
            s_addr: u32::from_ne_bytes([a, b, c, d]),
        }
    }

    pub fn octets(&self) -> [u8; 4] {
        self.s_addr.to_ne_bytes()
    }
}

/// IPv4 Socket Address (`struct sockaddr_in`).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SockAddrIn {
    pub sin_family: u16,
    /// Port number in network byte order (big-endian).
    pub sin_port: u16,
    pub sin_addr: InAddr,
    pub sin_zero: [u8; 8],
}

impl SockAddrIn {
    pub fn new(addr: InAddr, port: u16) -> Self {
        Self {
            sin_family: AF_INET,
            sin_port: port.to_be(),
            sin_addr: addr,
            sin_zero: [0; 8],
        }
    }

    pub fn port(&self) -> u16 {
        u16::from_be(self.sin_port)
    }
}

/// IPv6 Internet address (`struct in6_addr`).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct In6Addr {
    pub s6_addr: [u8; 16],
}

impl In6Addr {
    pub const ANY: Self = Self { s6_addr: [0; 16] };
    pub const LOOPBACK: Self = Self {
        s6_addr: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1],
    };

    pub const fn new(octets: [u8; 16]) -> Self {
        Self { s6_addr: octets }
    }
}

/// IPv6 Socket Address (`struct sockaddr_in6`).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SockAddrIn6 {
    pub sin6_family: u16,
    /// Port in network byte order.
    pub sin6_port: u16,
    pub sin6_flowinfo: u32,
    pub sin6_addr: In6Addr,
    pub sin6_scope_id: u32,
}

impl SockAddrIn6 {
    pub fn new(addr: In6Addr, port: u16) -> Self {
        Self {
            sin6_family: AF_INET6,
            sin6_port: port.to_be(),
            sin6_flowinfo: 0,
            sin6_addr: addr,
            sin6_scope_id: 0,
        }
    }

    pub fn port(&self) -> u16 {
        u16::from_be(self.sin6_port)
    }
}

/// UNIX Domain Socket Address (`struct sockaddr_un`).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SockAddrUn {
    pub sun_family: u16,
    pub sun_path: [u8; 108],
}

impl Default for SockAddrUn {
    fn default() -> Self {
        Self {
            sun_family: AF_UNIX,
            sun_path: [0; 108],
        }
    }
}

impl SockAddrUn {
    pub fn new(path: &str) -> Self {
        let mut sa = Self::default();
        let bytes = path.as_bytes();
        let len = core::cmp::min(bytes.len(), 107);
        sa.sun_path[..len].copy_from_slice(&bytes[..len]);
        sa.sun_path[len] = 0;
        sa
    }

    pub fn path(&self) -> &str {
        let mut end = 0;
        while end < self.sun_path.len() && self.sun_path[end] != 0 {
            end += 1;
        }
        core::str::from_utf8(&self.sun_path[..end]).unwrap_or("")
    }
}

/// Large generic storage buffer for any socket address (`struct sockaddr_storage`).
#[repr(C, align(8))]
#[derive(Clone, Copy)]
pub struct SockAddrStorage {
    pub ss_family: u16,
    pub __ss_padding: [u8; 118],
    pub __ss_align: u64,
}

impl Default for SockAddrStorage {
    fn default() -> Self {
        Self {
            ss_family: AF_UNSPEC,
            __ss_padding: [0; 118],
            __ss_align: 0,
        }
    }
}

/// I/O Vector (`struct iovec`).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct IoVec {
    pub iov_base: u64,
    pub iov_len: usize,
}

/// Message header for `sendmsg`/`recvmsg` (`struct msghdr`).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct MsgHdr {
    pub msg_name: u64,
    pub msg_namelen: u32,
    pub __pad0: u32,
    pub msg_iov: u64,
    pub msg_iovlen: usize,
    pub msg_control: u64,
    pub msg_controllen: usize,
    pub msg_flags: i32,
    pub __pad1: i32,
}

/// `struct ucred` for `SO_PEERCRED`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UCred {
    pub pid: u32,
    pub uid: u32,
    pub gid: u32,
}

/// Helper conversions between smoltcp and POSIX IP addresses.
pub mod conv {
    use super::*;
    use smoltcp::wire::{IpAddress, IpEndpoint, Ipv4Address, Ipv6Address};

    pub fn in_addr_to_smoltcp(addr: InAddr) -> Ipv4Address {
        let octets = addr.octets();
        Ipv4Address::new(octets[0], octets[1], octets[2], octets[3])
    }

    pub fn smoltcp_to_in_addr(addr: Ipv4Address) -> InAddr {
        let octets = addr.octets();
        InAddr::new(octets[0], octets[1], octets[2], octets[3])
    }

    pub fn in6_addr_to_smoltcp(addr: In6Addr) -> Ipv6Address {
        Ipv6Address::from_octets(addr.s6_addr)
    }

    pub fn smoltcp_to_in6_addr(addr: Ipv6Address) -> In6Addr {
        In6Addr::new(addr.octets())
    }

    pub fn endpoint_to_sockaddr_storage(endpoint: IpEndpoint) -> (SockAddrStorage, usize) {
        let mut storage = SockAddrStorage::default();
        match endpoint.addr {
            IpAddress::Ipv4(v4) => {
                let sin = SockAddrIn::new(smoltcp_to_in_addr(v4), endpoint.port);
                // SAFETY: SockAddrIn is strictly smaller than SockAddrStorage.
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        &sin as *const _ as *const u8,
                        &mut storage as *mut _ as *mut u8,
                        size_of::<SockAddrIn>(),
                    );
                }
                (storage, size_of::<SockAddrIn>())
            }
            IpAddress::Ipv6(v6) => {
                let sin6 = SockAddrIn6::new(smoltcp_to_in6_addr(v6), endpoint.port);
                // SAFETY: SockAddrIn6 is strictly smaller than SockAddrStorage.
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        &sin6 as *const _ as *const u8,
                        &mut storage as *mut _ as *mut u8,
                        size_of::<SockAddrIn6>(),
                    );
                }
                (storage, size_of::<SockAddrIn6>())
            }
        }
    }
}
