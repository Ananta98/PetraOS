//! TCP Socket Implementation (IPv4 & IPv6)
//!
//! Wraps `smoltcp::socket::tcp::Socket` to provide full POSIX-compliant TCP
//! socket lifecycle, connecting, listening, accept queue, send, receive, and polling.

use alloc::collections::VecDeque;
use alloc::sync::Arc;
use alloc::vec::Vec;
use smoltcp::iface::SocketHandle;
use smoltcp::socket::tcp::{self, State};
use smoltcp::wire::IpEndpoint;

use crate::net::stack::{NET_STACK, current_time};
use crate::net::types::*;
use crate::sync::spinlock::Spinlock;
use crate::syscalls::SyscallError;

use super::Socket;

const TCP_RX_BUF_SIZE: usize = 65536;
const TCP_TX_BUF_SIZE: usize = 65536;

pub struct TcpSocket {
    pub handle: Option<SocketHandle>,
    pub local_endpoint: Option<IpEndpoint>,
    pub remote_endpoint: Option<IpEndpoint>,
    pub is_listening: bool,
    pub backlog: usize,
    pub pending_conns: VecDeque<Arc<Spinlock<Socket>>>,
    pub nonblocking: bool,
    pub shutdown_read: bool,
    pub shutdown_write: bool,
}

impl TcpSocket {
    /// Create a new unconnected TCP socket.
    pub fn new(nonblocking: bool) -> Self {
        let rx_buffer = tcp::SocketBuffer::new(alloc::vec![0u8; TCP_RX_BUF_SIZE]);
        let tx_buffer = tcp::SocketBuffer::new(alloc::vec![0u8; TCP_TX_BUF_SIZE]);
        let tcp_sock = tcp::Socket::new(rx_buffer, tx_buffer);

        let handle = if let Some(ref mut stack) = *NET_STACK.lock() {
            Some(stack.add_socket(tcp_sock))
        } else {
            None
        };

        Self {
            handle,
            local_endpoint: None,
            remote_endpoint: None,
            is_listening: false,
            backlog: 128,
            pending_conns: VecDeque::new(),
            nonblocking,
            shutdown_read: false,
            shutdown_write: false,
        }
    }

    /// Bind this socket to a local IP endpoint (port and address).
    pub fn bind(&mut self, endpoint: IpEndpoint) -> Result<(), SyscallError> {
        if self.local_endpoint.is_some() {
            return Err(SyscallError::EINVAL);
        }
        self.local_endpoint = Some(endpoint);
        Ok(())
    }

    /// Transition to listening mode with the given backlog limit.
    pub fn listen(&mut self, backlog: usize) -> Result<(), SyscallError> {
        let endpoint = self.local_endpoint.ok_or(SyscallError::EDESTADDRREQ)?;
        let handle = self.handle.ok_or(SyscallError::ENETDOWN)?;

        let mut stack_guard = NET_STACK.lock();
        let stack = stack_guard.as_mut().ok_or(SyscallError::ENETDOWN)?;

        let socket = stack.sockets.get_mut::<tcp::Socket>(handle);
        socket
            .listen(endpoint)
            .map_err(|_| SyscallError::EADDRINUSE)?;

        self.is_listening = true;
        self.backlog = if backlog == 0 { 128 } else { backlog };

        Ok(())
    }

    /// Accept a newly established incoming connection.
    pub fn accept(&mut self, nonblocking: bool) -> Result<(Arc<Spinlock<Socket>>, IpEndpoint), SyscallError> {
        if !self.is_listening {
            return Err(SyscallError::EINVAL);
        }

        loop {
            // First check if we have queued accepted sockets
            if let Some(conn) = self.pending_conns.pop_front() {
                let remote = conn.lock().getpeername()?.0;
                return Ok((conn, remote));
            }

            // Check the current active listening socket
            let handle = self.handle.ok_or(SyscallError::ENETDOWN)?;
            let mut stack_guard = NET_STACK.lock();
            let stack = stack_guard.as_mut().ok_or(SyscallError::ENETDOWN)?;

            stack.poll();

            let is_established = {
                let socket = stack.sockets.get_mut::<tcp::Socket>(handle);
                socket.state() == State::Established
            };

            if is_established {
                let (remote, local) = {
                    let socket = stack.sockets.get_mut::<tcp::Socket>(handle);
                    let remote = socket.remote_endpoint().ok_or(SyscallError::ENOTCONN)?;
                    let local = socket.local_endpoint().unwrap_or(self.local_endpoint.unwrap());
                    (remote, local)
                };

                // Create a new connected TCP socket handle
                let new_rx_buffer = tcp::SocketBuffer::new(alloc::vec![0u8; TCP_RX_BUF_SIZE]);
                let new_tx_buffer = tcp::SocketBuffer::new(alloc::vec![0u8; TCP_TX_BUF_SIZE]);
                let new_tcp = tcp::Socket::new(new_rx_buffer, new_tx_buffer);
                let new_handle = stack.add_socket(new_tcp);

                let accepted_sock = Arc::new(Spinlock::new(Socket::Tcp(TcpSocket {
                    handle: Some(new_handle),
                    local_endpoint: Some(local),
                    remote_endpoint: Some(remote),
                    is_listening: false,
                    backlog: 0,
                    pending_conns: VecDeque::new(),
                    nonblocking,
                    shutdown_read: false,
                    shutdown_write: false,
                })));

                // Re-listen on listening socket
                let socket = stack.sockets.get_mut::<tcp::Socket>(handle);
                let _ = socket.listen(local);

                return Ok((accepted_sock, remote));
            }

            if self.nonblocking || nonblocking {
                return Err(SyscallError::EAGAIN);
            }

            drop(stack_guard);
            crate::proc::thread::Thread::yield_cpu();
        }
    }

    /// Initiate a 3-way handshake to connect to `remote`.
    pub fn connect(&mut self, remote: IpEndpoint) -> Result<(), SyscallError> {
        let handle = self.handle.ok_or(SyscallError::ENETDOWN)?;
        let mut stack_guard = NET_STACK.lock();
        let stack = stack_guard.as_mut().ok_or(SyscallError::ENETDOWN)?;

        let local_port = self
            .local_endpoint
            .map(|e| e.port)
            .unwrap_or_else(|| (crate::arch::timer::hpet::elapsed_ns() % 16384 + 49152) as u16);

        let local_ep = IpEndpoint::new(
            self.local_endpoint
                .map(|e| e.addr)
                .unwrap_or(smoltcp::wire::IpAddress::Ipv4(smoltcp::wire::Ipv4Address::UNSPECIFIED)),
            local_port,
        );

        let socket = stack.sockets.get_mut::<tcp::Socket>(handle);
        let cx = stack.iface.context();
        socket
            .connect(cx, remote, local_ep)
            .map_err(|_| SyscallError::ECONNREFUSED)?;

        self.remote_endpoint = Some(remote);
        self.local_endpoint = Some(local_ep);

        if self.nonblocking {
            return Err(SyscallError::EINPROGRESS);
        }

        // Wait for connection to establish or fail
        drop(stack_guard);
        for _ in 0..5000 {
            if let Some(ref mut st) = *NET_STACK.lock() {
                st.poll();
                let sock = st.sockets.get_mut::<tcp::Socket>(handle);
                match sock.state() {
                    State::Established => return Ok(()),
                    State::Closed | State::TimeWait => return Err(SyscallError::ECONNREFUSED),
                    _ => {}
                }
            }
            crate::proc::thread::Thread::yield_cpu();
        }

        Err(SyscallError::ETIMEDOUT)
    }

    /// Send bytes over the connected TCP stream.
    pub fn send(&mut self, buf: &[u8], flags: i32) -> Result<usize, SyscallError> {
        if self.shutdown_write {
            return Err(SyscallError::EPIPE);
        }
        let handle = self.handle.ok_or(SyscallError::ENOTCONN)?;

        let nonblock = self.nonblocking || ((flags & MSG_DONTWAIT) != 0);

        loop {
            let mut stack_guard = NET_STACK.lock();
            let stack = stack_guard.as_mut().ok_or(SyscallError::ENETDOWN)?;
            stack.poll();

            let socket = stack.sockets.get_mut::<tcp::Socket>(handle);
            if !socket.is_active() && socket.state() == State::Closed {
                return Err(SyscallError::ECONNRESET);
            }

            if socket.can_send() {
                let sent = socket
                    .send_slice(buf)
                    .map_err(|_| SyscallError::EIO)?;
                stack.poll();
                return Ok(sent);
            }

            if nonblock {
                return Err(SyscallError::EAGAIN);
            }

            drop(stack_guard);
            crate::proc::thread::Thread::yield_cpu();
        }
    }

    /// Receive bytes from the connected TCP stream.
    pub fn recv(&mut self, buf: &mut [u8], flags: i32) -> Result<usize, SyscallError> {
        if self.shutdown_read {
            return Ok(0); // EOF
        }
        let handle = self.handle.ok_or(SyscallError::ENOTCONN)?;

        let nonblock = self.nonblocking || ((flags & MSG_DONTWAIT) != 0);
        let peek = (flags & MSG_PEEK) != 0;

        loop {
            let mut stack_guard = NET_STACK.lock();
            let stack = stack_guard.as_mut().ok_or(SyscallError::ENETDOWN)?;
            stack.poll();

            let socket = stack.sockets.get_mut::<tcp::Socket>(handle);

            if socket.can_recv() {
                if peek {
                    let recv_res = socket.peek_slice(buf).map_err(|_| SyscallError::EIO)?;
                    return Ok(recv_res);
                } else {
                    let recv_res = socket.recv_slice(buf).map_err(|_| SyscallError::EIO)?;
                    return Ok(recv_res);
                }
            }

            // If connection closed and no data left in buffer -> EOF
            if socket.state() == State::CloseWait
                || socket.state() == State::Closed
                || socket.state() == State::TimeWait
            {
                return Ok(0);
            }

            if nonblock {
                return Err(SyscallError::EAGAIN);
            }

            drop(stack_guard);
            crate::proc::thread::Thread::yield_cpu();
        }
    }

    /// Shutdown read and/or write channels.
    pub fn shutdown(&mut self, how: i32) -> Result<(), SyscallError> {
        match how {
            SHUT_RD => self.shutdown_read = true,
            SHUT_WR => {
                self.shutdown_write = true;
                if let Some(handle) = self.handle {
                    if let Some(ref mut stack) = *NET_STACK.lock() {
                        let socket = stack.sockets.get_mut::<tcp::Socket>(handle);
                        socket.close();
                    }
                }
            }
            SHUT_RDWR => {
                self.shutdown_read = true;
                self.shutdown_write = true;
                if let Some(handle) = self.handle {
                    if let Some(ref mut stack) = *NET_STACK.lock() {
                        let socket = stack.sockets.get_mut::<tcp::Socket>(handle);
                        socket.close();
                    }
                }
            }
            _ => return Err(SyscallError::EINVAL),
        }
        Ok(())
    }

    pub fn poll_read_ready(&self) -> bool {
        if self.is_listening && !self.pending_conns.is_empty() {
            return true;
        }
        if let Some(handle) = self.handle {
            if let Some(ref mut stack) = *NET_STACK.lock() {
                stack.poll();
                let sock = stack.sockets.get_mut::<tcp::Socket>(handle);
                return sock.can_recv() || sock.state() == State::CloseWait;
            }
        }
        false
    }

    pub fn poll_write_ready(&self) -> bool {
        if let Some(handle) = self.handle {
            if let Some(ref mut stack) = *NET_STACK.lock() {
                let sock = stack.sockets.get_mut::<tcp::Socket>(handle);
                return sock.can_send() && sock.state() == State::Established;
            }
        }
        false
    }
}

impl Drop for TcpSocket {
    fn drop(&mut self) {
        if let Some(handle) = self.handle {
            if let Some(ref mut stack) = *NET_STACK.lock() {
                let socket = stack.sockets.get_mut::<tcp::Socket>(handle);
                socket.abort();
                stack.remove_socket(handle);
            }
        }
    }
}
