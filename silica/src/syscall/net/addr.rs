// SPDX-License-Identifier: GPL-2.0

//! Network socket address helpers and conversions.

use alloc::sync::Arc;
use ostd::mm::io::{FallibleVmRead, FallibleVmWrite};
use smoltcp::wire::{IpAddress, IpEndpoint, Ipv4Address, Ipv6Address};

use crate::{
    api::errno::{Errno, Result},
    net::socket::{KernelSocket, AF_INET, AF_INET6},
    proc::{thread::Thread, Proc},
};

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct SockAddrIn {
    pub sin_family: u16,
    pub sin_port: u16,       // big-endian
    pub sin_addr: [u8; 4],
    pub sin_zero: [u8; 8],
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct SockAddrIn6 {
    pub sin6_family: u16,
    pub sin6_port: u16,      // big-endian
    pub sin6_flowinfo: u32,
    pub sin6_addr: [u8; 16],
    pub sin6_scope_id: u32,
}

pub fn get_socket(fd: i32) -> Result<Arc<KernelSocket>> {
    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let file = proc.fd_table.lock().get(fd)?;
    file.as_socket().ok_or(Errno::ENOTSOCK)
}

pub fn read_sockaddr(proc: &Arc<Proc>, addr_ptr: usize, addrlen: u32) -> Result<IpEndpoint> {
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
    reader.read_fallible(&mut writer).map_err(|_| Errno::EFAULT)?;
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
            reader.read_fallible(&mut writer).map_err(|_| Errno::EFAULT)?;

            let port = u16::from_be_bytes([buf[2], buf[3]]);
            let ip = Ipv4Address::from_octets([buf[4], buf[5], buf[6], buf[7]]);
            Ok(IpEndpoint::new(IpAddress::Ipv4(ip), port))
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
            reader.read_fallible(&mut writer).map_err(|_| Errno::EFAULT)?;

            let port = u16::from_be_bytes([buf[2], buf[3]]);
            let mut ip_bytes = [0u8; 16];
            ip_bytes.copy_from_slice(&buf[8..24]);
            let ip = Ipv6Address::from_octets(ip_bytes);
            Ok(IpEndpoint::new(IpAddress::Ipv6(ip), port))
        }
        _ => Err(Errno::EAFNOSUPPORT),
    }
}

pub fn write_sockaddr(
    proc: &Arc<Proc>,
    endpoint: IpEndpoint,
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
    reader.read_fallible(&mut writer).map_err(|_| Errno::EFAULT)?;
    let user_len = u32::from_ne_bytes(user_len_bytes) as usize;

    match endpoint.addr {
        IpAddress::Ipv4(v4) => {
            let mut sin = [0u8; 16];
            sin[0..2].copy_from_slice(&(AF_INET as u16).to_ne_bytes());
            sin[2..4].copy_from_slice(&endpoint.port.to_be_bytes());
            sin[4..8].copy_from_slice(&v4.octets());

            let copy_len = user_len.min(16);
            if copy_len > 0 {
                let mut writer = vmar
                    .vm_space()
                    .writer(addr_ptr, copy_len)
                    .map_err(|_| Errno::EFAULT)?;
                let mut reader = ostd::mm::VmReader::from(&sin[..copy_len]);
                writer.write_fallible(&mut reader).map_err(|_| Errno::EFAULT)?;
            }

            let ret_len = 16u32.to_ne_bytes();
            let mut writer = vmar
                .vm_space()
                .writer(addrlen_ptr, 4)
                .map_err(|_| Errno::EFAULT)?;
            let mut reader = ostd::mm::VmReader::from(&ret_len[..]);
            writer.write_fallible(&mut reader).map_err(|_| Errno::EFAULT)?;
        }
        IpAddress::Ipv6(v6) => {
            let mut sin6 = [0u8; 28];
            sin6[0..2].copy_from_slice(&(AF_INET6 as u16).to_ne_bytes());
            sin6[2..4].copy_from_slice(&endpoint.port.to_be_bytes());
            sin6[8..24].copy_from_slice(&v6.octets());

            let copy_len = user_len.min(28);
            if copy_len > 0 {
                let mut writer = vmar
                    .vm_space()
                    .writer(addr_ptr, copy_len)
                    .map_err(|_| Errno::EFAULT)?;
                let mut reader = ostd::mm::VmReader::from(&sin6[..copy_len]);
                writer.write_fallible(&mut reader).map_err(|_| Errno::EFAULT)?;
            }

            let ret_len = 28u32.to_ne_bytes();
            let mut writer = vmar
                .vm_space()
                .writer(addrlen_ptr, 4)
                .map_err(|_| Errno::EFAULT)?;
            let mut reader = ostd::mm::VmReader::from(&ret_len[..]);
            writer.write_fallible(&mut reader).map_err(|_| Errno::EFAULT)?;
        }
    }

    Ok(())
}
