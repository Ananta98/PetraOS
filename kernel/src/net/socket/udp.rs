//! UDP Socket Implementation (IPv4 & IPv6)
//!
//! Wraps `smoltcp::socket::udp::Socket` for UDP datagram sending, receiving,
//! connection filtering, and polling.

use smoltcp::iface::SocketHandle;
use smoltcp::socket::udp::{self, PacketBuffer, PacketMetadata};
use smoltcp::wire::IpEndpoint;

use crate::net::stack::NET_STACK;
use crate::net::types::*;
use crate::syscalls::SyscallError;

const UDP_PACKET_COUNT: usize = 32;
const UDP_PAYLOAD_SIZE: usize = 65536;

pub struct UdpSocket {
    pub handle: Option<SocketHandle>,
    pub local_endpoint: Option<IpEndpoint>,
    pub remote_endpoint: Option<IpEndpoint>,
    pub nonblocking: bool,
}

impl UdpSocket {
    /// Create a new unbound UDP socket.
    pub fn new(nonblocking: bool) -> Self {
        let rx_buffer = PacketBuffer::new(
            alloc::vec![PacketMetadata::EMPTY; UDP_PACKET_COUNT],
            alloc::vec![0u8; UDP_PAYLOAD_SIZE],
        );
        let tx_buffer = PacketBuffer::new(
            alloc::vec![PacketMetadata::EMPTY; UDP_PACKET_COUNT],
            alloc::vec![0u8; UDP_PAYLOAD_SIZE],
        );
        let udp_sock = udp::Socket::new(rx_buffer, tx_buffer);

        let handle = if let Some(ref mut stack) = *NET_STACK.lock() {
            Some(stack.add_socket(udp_sock))
        } else {
            None
        };

        Self {
            handle,
            local_endpoint: None,
            remote_endpoint: None,
            nonblocking,
        }
    }

    /// Bind this UDP socket to a local IP endpoint.
    pub fn bind(&mut self, endpoint: IpEndpoint) -> Result<(), SyscallError> {
        let handle = self.handle.ok_or(SyscallError::ENETDOWN)?;
        let mut stack_guard = NET_STACK.lock();
        let stack = stack_guard.as_mut().ok_or(SyscallError::ENETDOWN)?;

        let socket = stack.sockets.get_mut::<udp::Socket>(handle);
        socket
            .bind(endpoint)
            .map_err(|_| SyscallError::EADDRINUSE)?;

        self.local_endpoint = Some(endpoint);
        Ok(())
    }

    /// Set default remote endpoint for connected-UDP mode.
    pub fn connect(&mut self, endpoint: IpEndpoint) -> Result<(), SyscallError> {
        self.remote_endpoint = Some(endpoint);
        Ok(())
    }

    /// Send a UDP datagram to `dest` (or default connected remote).
    pub fn sendto(
        &mut self,
        buf: &[u8],
        dest: Option<IpEndpoint>,
        flags: i32,
    ) -> Result<usize, SyscallError> {
        let target = dest
            .or(self.remote_endpoint)
            .ok_or(SyscallError::EDESTADDRREQ)?;

        let handle = self.handle.ok_or(SyscallError::ENETDOWN)?;
        let nonblock = self.nonblocking || ((flags & MSG_DONTWAIT) != 0);

        // Auto-bind local port if not yet bound
        if self.local_endpoint.is_none() {
            let ephemeral_port =
                ((crate::arch::timer::hpet::elapsed_ns() % 16384) + 49152) as u16;
            let ep = IpEndpoint::new(smoltcp::wire::IpAddress::Ipv4(smoltcp::wire::Ipv4Address::UNSPECIFIED), ephemeral_port);
            self.bind(ep)?;
        }

        loop {
            let mut stack_guard = NET_STACK.lock();
            let stack = stack_guard.as_mut().ok_or(SyscallError::ENETDOWN)?;
            stack.poll();

            let socket = stack.sockets.get_mut::<udp::Socket>(handle);
            if socket.can_send() {
                socket
                    .send_slice(buf, target)
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

    /// Receive a UDP datagram and return received length + sender endpoint.
    pub fn recvfrom(
        &mut self,
        buf: &mut [u8],
        flags: i32,
    ) -> Result<(usize, IpEndpoint), SyscallError> {
        let handle = self.handle.ok_or(SyscallError::ENETDOWN)?;
        let nonblock = self.nonblocking || ((flags & MSG_DONTWAIT) != 0);
        let peek = (flags & MSG_PEEK) != 0;

        loop {
            let mut stack_guard = NET_STACK.lock();
            let stack = stack_guard.as_mut().ok_or(SyscallError::ENETDOWN)?;
            stack.poll();

            let socket = stack.sockets.get_mut::<udp::Socket>(handle);
            if socket.can_recv() {
                if peek {
                    let (data, meta) = socket.peek().map_err(|_| SyscallError::EIO)?;
                    let len = core::cmp::min(buf.len(), data.len());
                    buf[..len].copy_from_slice(&data[..len]);
                    return Ok((len, meta.endpoint));
                } else {
                    let (data, meta) = socket.recv().map_err(|_| SyscallError::EIO)?;
                    let len = core::cmp::min(buf.len(), data.len());
                    buf[..len].copy_from_slice(&data[..len]);
                    return Ok((len, meta.endpoint));
                }
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
                let sock = stack.sockets.get_mut::<udp::Socket>(handle);
                return sock.can_recv();
            }
        }
        false
    }

    pub fn poll_write_ready(&self) -> bool {
        if let Some(handle) = self.handle {
            if let Some(ref mut stack) = *NET_STACK.lock() {
                let sock = stack.sockets.get_mut::<udp::Socket>(handle);
                return sock.can_send();
            }
        }
        false
    }
}

impl Drop for UdpSocket {
    fn drop(&mut self) {
        if let Some(handle) = self.handle {
            if let Some(ref mut stack) = *NET_STACK.lock() {
                stack.remove_socket(handle);
            }
        }
    }
}
