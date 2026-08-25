//! RAW Socket Implementation (ICMP / IP)
//!
//! Wraps `smoltcp::socket::raw::Socket` for raw packet injection, ICMP echo/ping,
//! and protocol-level packet filtering.

use smoltcp::iface::SocketHandle;
use smoltcp::socket::raw::{self, PacketBuffer, PacketMetadata};
use smoltcp::wire::{IpProtocol, IpVersion};

use crate::net::stack::NET_STACK;
use crate::net::types::*;
use crate::syscalls::SyscallError;

const RAW_PACKET_COUNT: usize = 16;
const RAW_PAYLOAD_SIZE: usize = 65536;

pub struct RawSocket {
    pub handle: Option<SocketHandle>,
    pub protocol: u8,
    pub nonblocking: bool,
}

impl RawSocket {
    pub fn new(protocol: u8, nonblocking: bool) -> Self {
        let rx_buffer = PacketBuffer::new(
            alloc::vec![PacketMetadata::EMPTY; RAW_PACKET_COUNT],
            alloc::vec![0u8; RAW_PAYLOAD_SIZE],
        );
        let tx_buffer = PacketBuffer::new(
            alloc::vec![PacketMetadata::EMPTY; RAW_PACKET_COUNT],
            alloc::vec![0u8; RAW_PAYLOAD_SIZE],
        );

        let ip_version = IpVersion::Ipv4;
        let ip_protocol = IpProtocol::from(protocol);

        let raw_sock = raw::Socket::new(
            Some(ip_version),
            Some(ip_protocol),
            rx_buffer,
            tx_buffer,
        );

        let handle = if let Some(ref mut stack) = *NET_STACK.lock() {
            Some(stack.add_socket(raw_sock))
        } else {
            None
        };

        Self {
            handle,
            protocol,
            nonblocking,
        }
    }

    pub fn send(&mut self, buf: &[u8], flags: i32) -> Result<usize, SyscallError> {
        let handle = self.handle.ok_or(SyscallError::ENETDOWN)?;
        let nonblock = self.nonblocking || ((flags & MSG_DONTWAIT) != 0);

        loop {
            let mut stack_guard = NET_STACK.lock();
            let stack = stack_guard.as_mut().ok_or(SyscallError::ENETDOWN)?;
            stack.poll();

            let socket = stack.sockets.get_mut::<raw::Socket>(handle);
            if socket.can_send() {
                socket
                    .send_slice(buf)
                    .map_err(|_| SyscallError::EMSGSIZE)?;
                stack.poll();
                return Ok(buf.len());
            }

            if nonblock {
                return Err(SyscallError::EAGAIN);
            }

            drop(stack_guard);
            crate::proc::thread::Thread::yield_cpu();
        }
    }

    pub fn recv(&mut self, buf: &mut [u8], flags: i32) -> Result<usize, SyscallError> {
        let handle = self.handle.ok_or(SyscallError::ENETDOWN)?;
        let nonblock = self.nonblocking || ((flags & MSG_DONTWAIT) != 0);

        loop {
            let mut stack_guard = NET_STACK.lock();
            let stack = stack_guard.as_mut().ok_or(SyscallError::ENETDOWN)?;
            stack.poll();

            let socket = stack.sockets.get_mut::<raw::Socket>(handle);
            if socket.can_recv() {
                let packet = socket.recv().map_err(|_| SyscallError::EIO)?;
                let len = core::cmp::min(buf.len(), packet.len());
                buf[..len].copy_from_slice(&packet[..len]);
                return Ok(len);
            }

            if nonblock {
                return Err(SyscallError::EAGAIN);
            }

            drop(stack_guard);
            crate::proc::thread::Thread::yield_cpu();
        }
    }

    pub fn poll_read_ready(&self) -> bool {
        if let Some(handle) = self.handle {
            if let Some(ref mut stack) = *NET_STACK.lock() {
                stack.poll();
                let sock = stack.sockets.get_mut::<raw::Socket>(handle);
                return sock.can_recv();
            }
        }
        false
    }

    pub fn poll_write_ready(&self) -> bool {
        if let Some(handle) = self.handle {
            if let Some(ref mut stack) = *NET_STACK.lock() {
                let sock = stack.sockets.get_mut::<raw::Socket>(handle);
                return sock.can_send();
            }
        }
        false
    }
}

impl Drop for RawSocket {
    fn drop(&mut self) {
        if let Some(handle) = self.handle {
            if let Some(ref mut stack) = *NET_STACK.lock() {
                stack.remove_socket(handle);
            }
        }
    }
}
