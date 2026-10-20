// SPDX-License-Identifier: GPL-2.0

//! Network socket address helpers and conversions.

use alloc::{string::String, sync::Arc};
use ostd::mm::io::{FallibleVmRead, FallibleVmWrite};
use smoltcp::wire::{IpAddress, IpEndpoint, Ipv4Address, Ipv6Address};

use crate::{
    api::errno::{Errno, Result},
    net::socket::{AF_INET, AF_INET6, AF_NETLINK, AF_UNIX, KernelSocket, SocketAddress},
    proc::{Proc, thread::Thread},
};

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct SockAddrIn {
    pub sin_family: u16,
    pub sin_port: u16, // big-endian
    pub sin_addr: [u8; 4],
    pub sin_zero: [u8; 8],
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct SockAddrIn6 {
    pub sin6_family: u16,
    pub sin6_port: u16, // big-endian
    pub sin6_flowinfo: u32,
    pub sin6_addr: [u8; 16],
    pub sin6_scope_id: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct SockAddrUn {
    pub sun_family: u16,
    pub sun_path: [u8; 108],
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct SockAddrNl {
    pub nl_family: u16,
    pub nl_pad: u16,
    pub nl_pid: u32,
    pub nl_groups: u32,
}

pub fn get_socket(fd: i32) -> Result<Arc<KernelSocket>> {
    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let file = proc.fd_table.lock().get(fd)?;
    file.as_socket().ok_or(Errno::ENOTSOCK)
}

pub fn read_sockaddr(proc: &Arc<Proc>, addr_ptr: usize, addrlen: u32) -> Result<SocketAddress> {
    if addr_ptr == 0 || addrlen < 2 {
        return Err(Errno::EINVAL);
    }
    let vmar = proc.vmspace();

    let mut family_bytes = [0u8; 2];
    let mut reader = vmar
        .vm_space()
        .reader(addr_ptr, 2)
        .map_err(|_| Errno::EFAULT)?;
    let mut writer = ostd::mm::VmWriter::from(&mut family_bytes[..]);
    reader
        .read_fallible(&mut writer)
        .map_err(|_| Errno::EFAULT)?;
    let family = u16::from_ne_bytes(family_bytes) as i32;

    match family {
        AF_INET => {
            if addrlen < 16 {
                return Err(Errno::EINVAL);
            }
            let mut buf = [0u8; 16];
            let mut reader = vmar
                .vm_space()
                .reader(addr_ptr, 16)
                .map_err(|_| Errno::EFAULT)?;
            let mut writer = ostd::mm::VmWriter::from(&mut buf[..]);
            reader
                .read_fallible(&mut writer)
                .map_err(|_| Errno::EFAULT)?;

            let port = u16::from_be_bytes([buf[2], buf[3]]);
            let ip = Ipv4Address::from_octets([buf[4], buf[5], buf[6], buf[7]]);
            Ok(SocketAddress::Inet(IpEndpoint::new(
                IpAddress::Ipv4(ip),
                port,
            )))
        }
        AF_INET6 => {
            if addrlen < 28 {
                return Err(Errno::EINVAL);
            }
            let mut buf = [0u8; 28];
            let mut reader = vmar
                .vm_space()
                .reader(addr_ptr, 28)
                .map_err(|_| Errno::EFAULT)?;
            let mut writer = ostd::mm::VmWriter::from(&mut buf[..]);
            reader
                .read_fallible(&mut writer)
                .map_err(|_| Errno::EFAULT)?;

            let port = u16::from_be_bytes([buf[2], buf[3]]);
            let mut ip_bytes = [0u8; 16];
            ip_bytes.copy_from_slice(&buf[8..24]);
            let ip = Ipv6Address::from_octets(ip_bytes);
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
            let mut reader = vmar
                .vm_space()
                .reader(addr_ptr + 2, path_len)
                .map_err(|_| Errno::EFAULT)?;
            let mut writer = ostd::mm::VmWriter::from(&mut buf[..]);
            reader
                .read_fallible(&mut writer)
                .map_err(|_| Errno::EFAULT)?;

            // If null-terminated, slice before null byte
            let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
            let path = String::from_utf8_lossy(&buf[..end]).into_owned();
            Ok(SocketAddress::Unix(path))
        }
        AF_NETLINK => {
            if addrlen < 12 {
                return Err(Errno::EINVAL);
            }
            let mut buf = [0u8; 12];
            let mut reader = vmar
                .vm_space()
                .reader(addr_ptr, 12)
                .map_err(|_| Errno::EFAULT)?;
            let mut writer = ostd::mm::VmWriter::from(&mut buf[..]);
            reader
                .read_fallible(&mut writer)
                .map_err(|_| Errno::EFAULT)?;

            let pid = u32::from_ne_bytes(buf[4..8].try_into().unwrap());
            let groups = u32::from_ne_bytes(buf[8..12].try_into().unwrap());
            Ok(SocketAddress::Netlink { pid, groups })
        }
        _ => Err(Errno::EAFNOSUPPORT),
    }
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

    let mut user_len_bytes = [0u8; 4];
    let mut reader = vmar
        .vm_space()
        .reader(addrlen_ptr, 4)
        .map_err(|_| Errno::EFAULT)?;
    let mut writer = ostd::mm::VmWriter::from(&mut user_len_bytes[..]);
    reader
        .read_fallible(&mut writer)
        .map_err(|_| Errno::EFAULT)?;
    let user_len = u32::from_ne_bytes(user_len_bytes) as usize;

    match endpoint {
        SocketAddress::Inet(ep) => match ep.addr {
            IpAddress::Ipv4(v4) => {
                let mut sin = [0u8; 16];
                sin[0..2].copy_from_slice(&(AF_INET as u16).to_ne_bytes());
                sin[2..4].copy_from_slice(&ep.port.to_be_bytes());
                sin[4..8].copy_from_slice(&v4.octets());

                let copy_len = user_len.min(16);
                if copy_len > 0 {
                    let mut writer = vmar
                        .vm_space()
                        .writer(addr_ptr, copy_len)
                        .map_err(|_| Errno::EFAULT)?;
                    let mut reader = ostd::mm::VmReader::from(&sin[..copy_len]);
                    writer
                        .write_fallible(&mut reader)
                        .map_err(|_| Errno::EFAULT)?;
                }

                let ret_len = 16u32.to_ne_bytes();
                let mut writer = vmar
                    .vm_space()
                    .writer(addrlen_ptr, 4)
                    .map_err(|_| Errno::EFAULT)?;
                let mut reader = ostd::mm::VmReader::from(&ret_len[..]);
                writer
                    .write_fallible(&mut reader)
                    .map_err(|_| Errno::EFAULT)?;
            }
            IpAddress::Ipv6(v6) => {
                let mut sin6 = [0u8; 28];
                sin6[0..2].copy_from_slice(&(AF_INET6 as u16).to_ne_bytes());
                sin6[2..4].copy_from_slice(&ep.port.to_be_bytes());
                sin6[8..24].copy_from_slice(&v6.octets());

                let copy_len = user_len.min(28);
                if copy_len > 0 {
                    let mut writer = vmar
                        .vm_space()
                        .writer(addr_ptr, copy_len)
                        .map_err(|_| Errno::EFAULT)?;
                    let mut reader = ostd::mm::VmReader::from(&sin6[..copy_len]);
                    writer
                        .write_fallible(&mut reader)
                        .map_err(|_| Errno::EFAULT)?;
                }

                let ret_len = 28u32.to_ne_bytes();
                let mut writer = vmar
                    .vm_space()
                    .writer(addrlen_ptr, 4)
                    .map_err(|_| Errno::EFAULT)?;
                let mut reader = ostd::mm::VmReader::from(&ret_len[..]);
                writer
                    .write_fallible(&mut reader)
                    .map_err(|_| Errno::EFAULT)?;
            }
        },
        SocketAddress::Unix(path) => {
            let mut sun = [0u8; 110];
            sun[0..2].copy_from_slice(&(AF_UNIX as u16).to_ne_bytes());
            let path_bytes = path.as_bytes();
            let copy_path_len = path_bytes.len().min(107);
            sun[2..2 + copy_path_len].copy_from_slice(&path_bytes[..copy_path_len]);
            if 2 + copy_path_len < 110 {
                sun[2 + copy_path_len] = 0;
            }

            let full_len = if path.is_empty() {
                2usize
            } else {
                (2 + path_bytes.len() + 1).min(110)
            };
            let copy_len = user_len.min(full_len);
            if copy_len > 0 {
                let mut writer = vmar
                    .vm_space()
                    .writer(addr_ptr, copy_len)
                    .map_err(|_| Errno::EFAULT)?;
                let mut reader = ostd::mm::VmReader::from(&sun[..copy_len]);
                writer
                    .write_fallible(&mut reader)
                    .map_err(|_| Errno::EFAULT)?;
            }

            let ret_len = (full_len as u32).to_ne_bytes();
            let mut writer = vmar
                .vm_space()
                .writer(addrlen_ptr, 4)
                .map_err(|_| Errno::EFAULT)?;
            let mut reader = ostd::mm::VmReader::from(&ret_len[..]);
            writer
                .write_fallible(&mut reader)
                .map_err(|_| Errno::EFAULT)?;
        }
        SocketAddress::Netlink { pid, groups } => {
            let mut nl = [0u8; 12];
            nl[0..2].copy_from_slice(&(AF_NETLINK as u16).to_ne_bytes());
            nl[2..4].copy_from_slice(&0u16.to_ne_bytes());
            nl[4..8].copy_from_slice(&pid.to_ne_bytes());
            nl[8..12].copy_from_slice(&groups.to_ne_bytes());

            let copy_len = user_len.min(12);
            if copy_len > 0 {
                let mut writer = vmar
                    .vm_space()
                    .writer(addr_ptr, copy_len)
                    .map_err(|_| Errno::EFAULT)?;
                let mut reader = ostd::mm::VmReader::from(&nl[..copy_len]);
                writer
                    .write_fallible(&mut reader)
                    .map_err(|_| Errno::EFAULT)?;
            }

            let ret_len = 12u32.to_ne_bytes();
            let mut writer = vmar
                .vm_space()
                .writer(addrlen_ptr, 4)
                .map_err(|_| Errno::EFAULT)?;
            let mut reader = ostd::mm::VmReader::from(&ret_len[..]);
            writer
                .write_fallible(&mut reader)
                .map_err(|_| Errno::EFAULT)?;
        }
    }

    Ok(())
}
