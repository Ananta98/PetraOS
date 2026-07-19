//! Networking socket abstractions (TCP and UDP) wrapping smoltcp sockets.
//! Enforces safety guidelines and denies unsafe code.

use alloc::vec::Vec;
use smoltcp::socket::tcp::{Socket as SmoltcpTcpSocket, SocketBuffer as TcpSocketBuffer};
use smoltcp::socket::udp::{
    PacketBuffer as UdpPacketBuffer, PacketMetadata as UdpPacketMetadata,
    Socket as SmoltcpUdpSocket,
};

/// Unified networking socket enum.
#[derive(Debug)]
pub enum Socket<'a> {
    /// TCP connection socket.
    Tcp(TcpSocket<'a>),
    /// UDP datagram socket.
    Udp(UdpSocket<'a>),
}

/// A TCP socket wrapper that manages its own buffers.
#[derive(Debug)]
pub struct TcpSocket<'a> {
    inner: SmoltcpTcpSocket<'a>,
}

impl<'a> TcpSocket<'a> {
    /// Create a new TCP socket with default buffer allocations.
    pub fn new(rx_buffer_size: usize, tx_buffer_size: usize) -> Self {
        let rx_data = alloc::vec![0u8; rx_buffer_size];
        let tx_data = alloc::vec![0u8; tx_buffer_size];
        let rx_buffer = TcpSocketBuffer::new(rx_data);
        let tx_buffer = TcpSocketBuffer::new(tx_data);
        Self {
            inner: SmoltcpTcpSocket::new(rx_buffer, tx_buffer),
        }
    }

    /// Return a reference to the underlying smoltcp TCP socket.
    pub fn inner(&self) -> &SmoltcpTcpSocket<'a> {
        &self.inner
    }

    /// Return a mutable reference to the underlying smoltcp TCP socket.
    pub fn inner_mut(&mut self) -> &mut SmoltcpTcpSocket<'a> {
        &mut self.inner
    }
}

/// A UDP socket wrapper that manages its own packet buffers and metadata.
#[derive(Debug)]
pub struct UdpSocket<'a> {
    inner: SmoltcpUdpSocket<'a>,
}

impl<'a> UdpSocket<'a> {
    /// Create a new UDP socket with specified buffer and packet count capacity.
    pub fn new(
        rx_buffer_size: usize,
        rx_packet_count: usize,
        tx_buffer_size: usize,
        tx_packet_count: usize,
    ) -> Self {
        let rx_meta = alloc::vec![UdpPacketMetadata::EMPTY; rx_packet_count];
        let rx_data = alloc::vec![0u8; rx_buffer_size];
        let tx_meta = alloc::vec![UdpPacketMetadata::EMPTY; tx_packet_count];
        let tx_data = alloc::vec![0u8; tx_buffer_size];
        let rx_buffer = UdpPacketBuffer::new(rx_meta, rx_data);
        let tx_buffer = UdpPacketBuffer::new(tx_meta, tx_data);
        Self {
            inner: SmoltcpUdpSocket::new(rx_buffer, tx_buffer),
        }
    }

    /// Return a reference to the underlying smoltcp UDP socket.
    pub fn inner(&self) -> &SmoltcpUdpSocket<'a> {
        &self.inner
    }

    /// Return a mutable reference to the underlying smoltcp UDP socket.
    pub fn inner_mut(&mut self) -> &mut SmoltcpUdpSocket<'a> {
        &mut self.inner
    }
}

#[cfg(ktest)]
mod tests {
    use super::*;
    use ostd::prelude::ktest;

    #[ktest]
    fn test_tcp_socket_creation() {
        let socket = TcpSocket::new(1024, 1024);
        assert!(!socket.inner().is_open());
    }

    #[ktest]
    fn test_udp_socket_creation() {
        let socket = UdpSocket::new(1024, 4, 1024, 4);
        assert!(!socket.inner().is_open());
    }
}
