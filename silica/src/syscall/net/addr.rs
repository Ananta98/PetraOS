// SPDX-License-Identifier: GPL-2.0

//! Network socket address helpers and conversions.

use alloc::{string::String, sync::Arc};
use smoltcp::wire::{IpAddress, IpEndpoint, Ipv4Address, Ipv6Address};
use zerocopy::{FromBytes, Immutable,IntoBytes};

use crate::{
    api::errno::{Errno, Result},
    net::socket::{AF_INET, AF_INET6, AF_NETLINK, AF_UNIX, KernelSocket, SocketAddress},
    proc::{Proc, thread::Thread},
    utils::userspace::UserPtr,
    vm::vmar::Vmar,
};

#[repr(C)]
#[derive(Clone, Copy, Default, Debug, FromBytes, IntoBytes, Immutable)]
pub struct SockAddrIn {
    pub sin_family: u16,
    pub sin_port: u16, // big-endian
    pub sin_addr: [u8; 4],
    pub sin_zero: [u8; 8],
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug, FromBytes, IntoBytes, Immutable)]
pub struct SockAddrIn6 {
    pub sin6_family: u16,
    pub sin6_port: u16, // big-endian
    pub sin6_flowinfo: u32,
    pub sin6_addr: [u8; 16],
    pub sin6_scope_id: u32,
}

#[repr(C)]
#[derive(Clone, Copy, FromBytes, IntoBytes, Immutable)]
pub struct SockAddrUn {
    pub sun_family: u16,
    pub sun_path: [u8; 108],
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug, FromBytes, IntoBytes, Immutable)]
pub struct SockAddrNl {
    pub nl_family: u16,
    pub nl_pad: u16,
    pub nl_pid: u32,
    pub nl_groups: u32,
}

pub fn get_socket(fd: i32) -> Result<Arc<KernelSocket>> {
    let proc = Thread::proc_or_esrch()?;
    let file = proc.file(fd)?;
    file.as_socket().ok_or(Errno::ENOTSOCK)
}

pub fn read_sockaddr(proc: &Arc<Proc>, addr_ptr: usize, addrlen: u32) -> Result<SocketAddress> {
    if addr_ptr == 0 || addrlen < 2 {
        return Err(Errno::EINVAL);
    }
    let vmar = proc.vmspace();
    let addr = UserPtr::new(addr_ptr);

    let mut family_bytes = [0u8; 2];
    addr.read_bytes(&vmar, &mut family_bytes)?;
    let family = u16::from_ne_bytes(family_bytes) as i32;

    match family {
        AF_INET => {
            if addrlen < 16 {
                return Err(Errno::EINVAL);
            }
            let sin: SockAddrIn = addr.read_val(&vmar)?;

            let port = u16::from_be(sin.sin_port);
            let ip = Ipv4Address::from_octets(sin.sin_addr);
            Ok(SocketAddress::Inet(IpEndpoint::new(
                IpAddress::Ipv4(ip),
                port,
            )))
        }
        AF_INET6 => {
            if addrlen < 28 {
                return Err(Errno::EINVAL);
            }
            let sin6: SockAddrIn6 = addr.read_val(&vmar)?;

            let port = u16::from_be(sin6.sin6_port);
            let ip = Ipv6Address::from_octets(sin6.sin6_addr);
            Ok(SocketAddress::Inet(IpEndpoint::new(
                IpAddress::Ipv6(ip),
                port,
            )))
        }
        AF_UNIX => {
            if addrlen == 2 {
                // Unnamed unix socket
                return Ok(SocketAddress::Unix(String::new()));
            }
            let path_len = ((addrlen - 2) as usize).min(108);
            let mut buf = alloc::vec![0u8; path_len];
            UserPtr::new(addr_ptr + 2).read_bytes(&vmar, &mut buf)?;
            // If null-terminated, slice before null byte
            let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
            let path = String::from_utf8_lossy(&buf[..end]).into_owned();
            Ok(SocketAddress::Unix(path))
        }
        AF_NETLINK => {
            if addrlen < 12 {
                return Err(Errno::EINVAL);
            }
            let nl: SockAddrNl = addr.read_val(&vmar)?;
            Ok(SocketAddress::Netlink {
                pid: nl.nl_pid,
                groups: nl.nl_groups,
            })
        }
        _ => Err(Errno::EAFNOSUPPORT),
    }
}

/// Copies `full` (truncated to the caller-provided `user_len`) to `addr_ptr`.
fn write_addr_bytes(vmar: &Vmar, ptr: usize, user_len: usize, full: &[u8]) -> Result<()> {
    UserPtr::new(ptr).write_bytes(vmar, &full[..user_len.min(full.len())])
}

/// Reports the produced address length through the user `socklen_t *`.
fn write_addr_len(vmar: &Vmar, ptr: usize, len: u32) -> Result<()> {
    UserPtr::new(ptr).write_val(vmar, len)
}

pub fn write_sockaddr(
    proc: &Arc<Proc>,
    endpoint: SocketAddress,
    addr_ptr: usize,
    addrlen_ptr: usize,
) -> Result<()> {
    if addr_ptr == 0 || addrlen_ptr == 0 {
        return Ok(());
    }
    let vmar = proc.vmspace();
    let user_len = UserPtr::new(addrlen_ptr).read_val::<u32>(&vmar)? as usize;

    // Build the family-specific address bytes once; `write_addr_bytes`
    // handles truncation to the user buffer and `write_addr_len` the
    // canonical length report.
    match endpoint {
        SocketAddress::Inet(ep) => match ep.addr {
            IpAddress::Ipv4(v4) => {
                let sin: SockAddrIn = SockAddrIn {
                    sin_family: AF_INET as u16,
                    sin_port: ep.port.to_be(),
                    sin_addr: v4.octets(),
                    sin_zero: [0; 8],
                };
                write_addr_bytes(&vmar, addr_ptr, user_len, sin.as_bytes())?;
                write_addr_len(&vmar, addrlen_ptr, 16)?;
            }
            IpAddress::Ipv6(v6) => {
                let sin6 = SockAddrIn6 {
                    sin6_family: AF_INET6 as u16,
                    sin6_port: ep.port.to_be(),
                    sin6_flowinfo: 0,
                    sin6_addr: v6.octets(),
                    sin6_scope_id: 0,
                };
                write_addr_bytes(&vmar, addr_ptr, user_len, sin6.as_bytes())?;
                write_addr_len(&vmar, addrlen_ptr, 28)?;
            }
        },
        SocketAddress::Unix(path) => {
            let mut sun: SockAddrUn = SockAddrUn {
                sun_family: AF_UNIX as u16,
                sun_path: [0; 108],
            };
            let path_bytes = path.as_bytes();
            let copy_path_len = path_bytes.len().min(107);
            sun.sun_path[..copy_path_len].copy_from_slice(&path_bytes[..copy_path_len]);

            let full_len = if path.is_empty() {
                2usize
            } else {
                (2 + path_bytes.len() + 1).min(110)
            };
            write_addr_bytes(&vmar, addr_ptr, user_len, &sun.as_bytes()[..full_len])?;
            write_addr_len(&vmar, addrlen_ptr, full_len as u32)?;
        }
        SocketAddress::Netlink { pid, groups } => {
            let nl = SockAddrNl {
                nl_family: AF_NETLINK as u16,
                nl_pad: 0,
                nl_pid: pid,
                nl_groups: groups,
            };
            write_addr_bytes(&vmar, addr_ptr, user_len, nl.as_bytes())?;
            write_addr_len(&vmar, addrlen_ptr, 12)?;
        }
    }

    Ok(())
}
