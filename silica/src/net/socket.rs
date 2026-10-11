// SPDX-License-Identifier: GPL-2.0

//! UNIX Socket Abstraction wrapping smoltcp Sockets and Unix domain socket pairs.

use alloc::{collections::VecDeque, sync::Arc};
use ostd::{sync::WaitQueue, task::Task};
use smoltcp::{
    iface::SocketHandle,
    socket::{tcp::Socket as TcpSocket, tcp::State as TcpState, udp::Socket as UdpSocket},
    wire::IpEndpoint,
};
use spin::Mutex;

use crate::{
    api::errno::{Errno, Result},
    net::interface::NET_MANAGER,
};

// Standard socket domains
pub const AF_UNIX: i32 = 1;
pub const AF_INET: i32 = 2;
pub const AF_INET6: i32 = 10;

// Standard socket types
pub const SOCK_STREAM: i32 = 1;
pub const SOCK_DGRAM: i32 = 2;
pub const SOCK_RAW: i32 = 3;

// Standard protocols
pub const IPPROTO_IP: i32 = 0;
pub const IPPROTO_TCP: i32 = 6;
pub const IPPROTO_UDP: i32 = 17;

pub const UNIX_SOCKET_CAPACITY: usize = 65536;

/// Allocates a randomized ephemeral port in the dynamic range [49152, 65535] per RFC 6056.
pub fn alloc_ephemeral_port() -> u16 {
    let mut bytes = [0u8; 2];
    crate::crypto::get_random_bytes(&mut bytes);
    let offset = u16::from_ne_bytes(bytes) % 16384;
    49152 + offset
}

/// Unidirectional byte channel used for Unix socket pairs.
pub struct UnixChannel {
    pub buf: Mutex<VecDeque<u8>>,
    pub closed: Mutex<bool>,
    pub wq: WaitQueue,
}

impl UnixChannel {
    pub fn new() -> Self {
        Self {
            buf: Mutex::new(VecDeque::new()),
            closed: Mutex::new(false),
            wq: WaitQueue::new(),
        }
    }
}

impl Default for UnixChannel {
    fn default() -> Self {
        Self::new()
    }
}

/// Endpoint of a bidirectional Unix socket pair.
pub struct UnixSocketEnd {
    pub tx: Arc<UnixChannel>,
    pub rx: Arc<UnixChannel>,
}

impl core::fmt::Debug for UnixSocketEnd {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("UnixSocketEnd").finish()
    }
}

impl PartialEq for UnixSocketEnd {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.tx, &other.tx) && Arc::ptr_eq(&self.rx, &other.rx)
    }
}

impl Eq for UnixSocketEnd {}

/// Type of underlying socket implementation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SocketKind {
    Tcp(SocketHandle),
    Udp(SocketHandle),
    Unix(Arc<UnixSocketEnd>),
}

/// In-kernel representation of an open network socket.
pub struct KernelSocket {
    pub domain: i32,
    pub sock_type: i32,
    pub protocol: i32,
    pub kind: Mutex<SocketKind>,
    pub nonblocking: Mutex<bool>,
    pub local_endpoint: Mutex<Option<IpEndpoint>>,
    pub remote_endpoint: Mutex<Option<IpEndpoint>>,
}

impl KernelSocket {
    /// Creates a new TCP socket.
    pub fn new_tcp(domain: i32) -> Result<Arc<Self>> {
        let handle = NET_MANAGER.lock().add_tcp_socket();
        Ok(Arc::new(Self {
            domain,
            sock_type: SOCK_STREAM,
            protocol: IPPROTO_TCP,
            kind: Mutex::new(SocketKind::Tcp(handle)),
            nonblocking: Mutex::new(false),
            local_endpoint: Mutex::new(None),
            remote_endpoint: Mutex::new(None),
        }))
    }

    /// Creates a new UDP socket.
    pub fn new_udp(domain: i32) -> Result<Arc<Self>> {
        let handle = NET_MANAGER.lock().add_udp_socket();
        Ok(Arc::new(Self {
            domain,
            sock_type: SOCK_DGRAM,
            protocol: IPPROTO_UDP,
            kind: Mutex::new(SocketKind::Udp(handle)),
            nonblocking: Mutex::new(false),
            local_endpoint: Mutex::new(None),
            remote_endpoint: Mutex::new(None),
        }))
    }

    /// Creates a new connected Unix domain socket pair.
    pub fn new_unix_pair(sock_type: i32) -> Result<(Arc<Self>, Arc<Self>)> {
        let chan_a = Arc::new(UnixChannel::new());
        let chan_b = Arc::new(UnixChannel::new());

        let end_a = Arc::new(UnixSocketEnd {
            tx: chan_a.clone(),
            rx: chan_b.clone(),
        });
        let end_b = Arc::new(UnixSocketEnd {
            tx: chan_b,
            rx: chan_a,
        });

        let sock_a = Arc::new(Self {
            domain: AF_UNIX,
            sock_type,
            protocol: 0,
            kind: Mutex::new(SocketKind::Unix(end_a)),
            nonblocking: Mutex::new(false),
            local_endpoint: Mutex::new(None),
            remote_endpoint: Mutex::new(None),
        });

        let sock_b = Arc::new(Self {
            domain: AF_UNIX,
            sock_type,
            protocol: 0,
            kind: Mutex::new(SocketKind::Unix(end_b)),
            nonblocking: Mutex::new(false),
            local_endpoint: Mutex::new(None),
            remote_endpoint: Mutex::new(None),
        });

        Ok((sock_a, sock_b))
    }

    /// Binds the socket to a local endpoint.
    pub fn bind(&self, endpoint: IpEndpoint) -> Result<()> {
        let kind = self.kind.lock().clone();
        match kind {
            SocketKind::Tcp(_) => {
                *self.local_endpoint.lock() = Some(endpoint);
                Ok(())
            }
            SocketKind::Udp(handle) => {
                let mut net = NET_MANAGER.lock();
                let socket = net.get_sockets().get_mut::<UdpSocket>(handle);
                socket.bind(endpoint).map_err(|_| Errno::EADDRINUSE)?;
                *self.local_endpoint.lock() = Some(endpoint);
                Ok(())
            }
            SocketKind::Unix(_) => Ok(()),
        }
    }

    /// Listens for incoming connections (TCP only).
    pub fn listen(&self, _backlog: i32) -> Result<()> {
        let kind = self.kind.lock().clone();
        match kind {
            SocketKind::Tcp(handle) => {
                let local = self.local_endpoint.lock().ok_or(Errno::EDESTADDRREQ)?;
                let mut net = NET_MANAGER.lock();
                let socket = net.get_sockets().get_mut::<TcpSocket>(handle);
                socket.listen(local).map_err(|_| Errno::EADDRINUSE)?;
                Ok(())
            }
            SocketKind::Udp(_) => {
                crate::return_errno!(EOPNOTSUPP, "listen not supported on UDP socket")
            }
            SocketKind::Unix(_) => Ok(()),
        }
    }

    /// Connects to a remote endpoint.
    pub fn connect(&self, endpoint: IpEndpoint) -> Result<()> {
        let kind = self.kind.lock().clone();
        match kind {
            SocketKind::Tcp(handle) => {
                let local_port = self
                    .local_endpoint
                    .lock()
                    .map(|ep| ep.port)
                    .unwrap_or_else(alloc_ephemeral_port);

                let mut net = NET_MANAGER.lock();
                net.connect_tcp(handle, endpoint, local_port)?;
                *self.remote_endpoint.lock() = Some(endpoint);
                drop(net);

                // If blocking, wait until connection is established
                if !*self.nonblocking.lock() {
                    loop {
                        let mut net = NET_MANAGER.lock();
                        net.poll();
                        let socket = net.get_sockets().get_mut::<TcpSocket>(handle);
                        match socket.state() {
                            TcpState::Established => break,
                            TcpState::Closed | TcpState::TimeWait => {
                                return Err(Errno::ECONNREFUSED);
                            }
                            _ => {}
                        }
                        drop(net);
                        Task::yield_now();
                    }
                }
                Ok(())
            }
            SocketKind::Udp(_) => {
                *self.remote_endpoint.lock() = Some(endpoint);
                Ok(())
            }
            SocketKind::Unix(_) => Ok(()),
        }
    }

    /// Accepts an incoming connection on a listening TCP socket.
    pub fn accept(&self) -> Result<(Arc<KernelSocket>, IpEndpoint)> {
        let kind = self.kind.lock().clone();
        match kind {
            SocketKind::Tcp(listen_handle) => {
                let local = self.local_endpoint.lock().ok_or(Errno::EINVAL)?;
                loop {
                    let mut net = NET_MANAGER.lock();
                    net.poll();
                    let socket = net.get_sockets().get_mut::<TcpSocket>(listen_handle);

                    if socket.state() == TcpState::Established {
                        let remote = socket.remote_endpoint().ok_or(Errno::ENOTCONN)?;
                        // Allocate a new handle to continue listening
                        let new_listen_handle = net.add_tcp_socket();
                        let new_socket = net.get_sockets().get_mut::<TcpSocket>(new_listen_handle);
                        let _ = new_socket.listen(local);

                        // The current listen_handle is now the connected socket
                        let accepted_sock = Arc::new(KernelSocket {
                            domain: self.domain,
                            sock_type: SOCK_STREAM,
                            protocol: IPPROTO_TCP,
                            kind: Mutex::new(SocketKind::Tcp(listen_handle)),
                            nonblocking: Mutex::new(false),
                            local_endpoint: Mutex::new(Some(local)),
                            remote_endpoint: Mutex::new(Some(remote)),
                        });

                        // Swap listening socket to the new handle
                        *self.kind.lock() = SocketKind::Tcp(new_listen_handle);

                        return Ok((accepted_sock, remote));
                    }

                    if *self.nonblocking.lock() {
                        return Err(Errno::EWOULDBLOCK);
                    }

                    drop(net);
                    Task::yield_now();
                }
            }
            SocketKind::Udp(_) => crate::return_errno!(EOPNOTSUPP, "accept not supported on UDP socket"),
            SocketKind::Unix(_) => crate::return_errno!(EINVAL, "accept not supported on unix socket pair"),
        }
    }

    /// Gets local socket address endpoint.
    pub fn getsockname(&self) -> Result<IpEndpoint> {
        let kind = self.kind.lock().clone();
        match kind {
            SocketKind::Tcp(handle) => {
                let mut net = NET_MANAGER.lock();
                let socket = net.get_sockets().get_mut::<TcpSocket>(handle);
                if let Some(ep) = socket.local_endpoint() {
                    Ok(ep)
                } else if let Some(ep) = *self.local_endpoint.lock() {
                    Ok(ep)
                } else {
                    Err(Errno::ENOTCONN)
                }
            }
            SocketKind::Udp(_) => self.local_endpoint.lock().ok_or(Errno::ENOTCONN),
            SocketKind::Unix(_) => Err(Errno::ENOTCONN),
        }
    }

    /// Gets peer socket address endpoint.
    pub fn getpeername(&self) -> Result<IpEndpoint> {
        let kind = self.kind.lock().clone();
        match kind {
            SocketKind::Tcp(handle) => {
                let mut net = NET_MANAGER.lock();
                let socket = net.get_sockets().get_mut::<TcpSocket>(handle);
                if let Some(ep) = socket.remote_endpoint() {
                    Ok(ep)
                } else if let Some(ep) = *self.remote_endpoint.lock() {
                    Ok(ep)
                } else {
                    Err(Errno::ENOTCONN)
                }
            }
            SocketKind::Udp(_) => self.remote_endpoint.lock().ok_or(Errno::ENOTCONN),
            SocketKind::Unix(_) => Err(Errno::ENOTCONN),
        }
    }

    /// Sends data through the socket.
    pub fn send(&self, data: &[u8], _flags: i32) -> Result<usize> {
        let kind = self.kind.lock().clone();
        match kind {
            SocketKind::Tcp(handle) => loop {
                let mut net = NET_MANAGER.lock();
                net.poll();
                let socket = net.get_sockets().get_mut::<TcpSocket>(handle);

                if !socket.is_active() && socket.state() == TcpState::Closed {
                    return Err(Errno::EPIPE);
                }

                if socket.can_send() {
                    let sent = socket.send_slice(data).map_err(|_| Errno::ENOBUFS)?;
                    net.poll();
                    return Ok(sent);
                }

                if *self.nonblocking.lock() {
                    return Err(Errno::EWOULDBLOCK);
                }

                drop(net);
                Task::yield_now();
            },
            SocketKind::Udp(handle) => {
                let remote = self.remote_endpoint.lock().ok_or(Errno::EDESTADDRREQ)?;
                let mut net = NET_MANAGER.lock();
                let socket = net.get_sockets().get_mut::<UdpSocket>(handle);
                if socket.can_send() {
                    socket
                        .send_slice(data, remote)
                        .map_err(|_| Errno::ENOBUFS)?;
                    net.poll();
                    Ok(data.len())
                } else if *self.nonblocking.lock() {
                    Err(Errno::EWOULDBLOCK)
                } else {
                    Err(Errno::ENOBUFS)
                }
            }
            SocketKind::Unix(ref end) => {
                if data.is_empty() {
                    return Ok(0);
                }
                if *end.tx.closed.lock() {
                    if let Some(proc) = crate::proc::thread::Thread::current_proc() {
                        let _ = proc.send_signal(crate::api::signal::SigInfo::from_user(crate::api::signal::Signal::SIGPIPE, 0), 0);
                    }
                    return Err(Errno::EPIPE);
                }
                let mut written = 0;
                while written < data.len() {
                    let remaining = data.len() - written;
                    let step = || -> Option<Result<usize>> {
                        let mut buf = end.tx.buf.lock();
                        if *end.tx.closed.lock() {
                            return Some(Err(Errno::EPIPE));
                        }
                        let free = UNIX_SOCKET_CAPACITY.saturating_sub(buf.len());
                        if free > 0 {
                            let n = free.min(remaining);
                            buf.extend(&data[written..written + n]);
                            Some(Ok(n))
                        } else {
                            None
                        }
                    };

                    let res = match step() {
                        Some(r) => r,
                        None if *self.nonblocking.lock() => {
                            return if written > 0 {
                                Ok(written)
                            } else {
                                Err(Errno::EWOULDBLOCK)
                            };
                        }
                        None => end.tx.wq.wait_until(step),
                    };

                    match res {
                        Ok(n) => {
                            written += n;
                            end.tx.wq.wake_all();
                            crate::fs::poll::notify();
                        }
                        Err(e) => {
                            if written > 0 {
                                return Ok(written);
                            }
                            if let Some(proc) = crate::proc::thread::Thread::current_proc() {
                                let _ = proc.send_signal(crate::api::signal::SigInfo::from_user(crate::api::signal::Signal::SIGPIPE, 0), 0);
                            }
                            return Err(e);
                        }
                    }
                }
                Ok(written)
            }
        }
    }

    /// Receives data from the socket.
    pub fn recv(&self, buf: &mut [u8], _flags: i32) -> Result<usize> {
        let kind = self.kind.lock().clone();
        match kind {
            SocketKind::Tcp(handle) => {
                loop {
                    let mut net = NET_MANAGER.lock();
                    net.poll();
                    let socket = net.get_sockets().get_mut::<TcpSocket>(handle);

                    if socket.can_recv() {
                        let n = socket.recv_slice(buf).map_err(|_| Errno::EIO)?;
                        return Ok(n);
                    }

                    match socket.state() {
                        TcpState::Closed | TcpState::CloseWait | TcpState::TimeWait => {
                            return Ok(0); // EOF
                        }
                        _ => {}
                    }

                    if *self.nonblocking.lock() {
                        return Err(Errno::EWOULDBLOCK);
                    }

                    drop(net);
                    Task::yield_now();
                }
            }
            SocketKind::Udp(handle) => loop {
                let mut net = NET_MANAGER.lock();
                net.poll();
                let socket = net.get_sockets().get_mut::<UdpSocket>(handle);

                if socket.can_recv() {
                    let (n, meta) = socket.recv_slice(buf).map_err(|_| Errno::EIO)?;
                    *self.remote_endpoint.lock() = Some(meta.endpoint);
                    return Ok(n);
                }

                if *self.nonblocking.lock() {
                    return Err(Errno::EWOULDBLOCK);
                }

                drop(net);
                Task::yield_now();
            },
            SocketKind::Unix(ref end) => {
                if buf.is_empty() {
                    return Ok(0);
                }
                let mut try_read = || -> Option<usize> {
                    let mut rx_buf = end.rx.buf.lock();
                    if !rx_buf.is_empty() {
                        let n = buf.len().min(rx_buf.len());
                        for (slot, byte) in buf.iter_mut().zip(rx_buf.drain(..n)) {
                            *slot = byte;
                        }
                        Some(n)
                    } else if *end.rx.closed.lock() {
                        Some(0) // EOF
                    } else {
                        None
                    }
                };

                let n = match try_read() {
                    Some(n) => n,
                    None if *self.nonblocking.lock() => return Err(Errno::EWOULDBLOCK),
                    None => end.rx.wq.wait_until(try_read),
                };
                if n > 0 {
                    end.rx.wq.wake_all();
                    crate::fs::poll::notify();
                }
                Ok(n)
            }
        }
    }

    /// Sends a datagram to a specific destination endpoint.
    pub fn sendto(&self, data: &[u8], dest: Option<IpEndpoint>) -> Result<usize> {
        let kind = self.kind.lock().clone();
        match kind {
            SocketKind::Udp(handle) => {
                let target = dest
                    .or(*self.remote_endpoint.lock())
                    .ok_or(Errno::EDESTADDRREQ)?;
                let mut net = NET_MANAGER.lock();
                let socket = net.get_sockets().get_mut::<UdpSocket>(handle);
                socket
                    .send_slice(data, target)
                    .map_err(|_| Errno::ENOBUFS)?;
                net.poll();
                Ok(data.len())
            }
            SocketKind::Tcp(_) | SocketKind::Unix(_) => self.send(data, 0),
        }
    }

    /// Receives a datagram along with the sender endpoint.
    pub fn recvfrom(&self, buf: &mut [u8]) -> Result<(usize, Option<IpEndpoint>)> {
        let kind = self.kind.lock().clone();
        match kind {
            SocketKind::Udp(handle) => loop {
                let mut net = NET_MANAGER.lock();
                net.poll();
                let socket = net.get_sockets().get_mut::<UdpSocket>(handle);

                if socket.can_recv() {
                    let (n, meta) = socket.recv_slice(buf).map_err(|_| Errno::EIO)?;
                    return Ok((n, Some(meta.endpoint)));
                }

                if *self.nonblocking.lock() {
                    return Err(Errno::EWOULDBLOCK);
                }

                drop(net);
                Task::yield_now();
            },
            SocketKind::Tcp(_) => {
                let n = self.recv(buf, 0)?;
                let peer = *self.remote_endpoint.lock();
                Ok((n, peer))
            }
            SocketKind::Unix(_) => {
                let n = self.recv(buf, 0)?;
                Ok((n, None))
            }
        }
    }

    /// Shuts down all or part of a full-duplex connection.
    pub fn shutdown(&self, how: i32) -> Result<()> {
        let kind = self.kind.lock().clone();
        match kind {
            SocketKind::Tcp(handle) => {
                let mut net = NET_MANAGER.lock();
                let socket = net.get_sockets().get_mut::<TcpSocket>(handle);
                match how {
                    0 | 1 | 2 => {
                        socket.close();
                        net.poll();
                        Ok(())
                    }
                    _ => crate::return_errno!(EINVAL, "invalid shutdown mode"),
                }
            }
            SocketKind::Unix(ref end) => {
                match how {
                    0 => *end.rx.closed.lock() = true,
                    1 => *end.tx.closed.lock() = true,
                    2 => {
                        *end.rx.closed.lock() = true;
                        *end.tx.closed.lock() = true;
                    }
                    _ => return Err(Errno::EINVAL),
                }
                end.rx.wq.wake_all();
                end.tx.wq.wake_all();
                crate::fs::poll::notify();
                Ok(())
            }
            SocketKind::Udp(_) => Ok(()),
        }
    }

    /// Closes the socket and removes it from the global socket set.
    pub fn close(&self) {
        let kind = self.kind.lock().clone();
        match kind {
            SocketKind::Tcp(handle) => {
                let mut net = NET_MANAGER.lock();
                let socket = net.get_sockets().get_mut::<TcpSocket>(handle);
                socket.abort();
                net.remove_socket(handle);
            }
            SocketKind::Udp(handle) => {
                let mut net = NET_MANAGER.lock();
                let socket = net.get_sockets().get_mut::<UdpSocket>(handle);
                socket.close();
                net.remove_socket(handle);
            }
            SocketKind::Unix(ref end) => {
                *end.tx.closed.lock() = true;
                *end.rx.closed.lock() = true;
                end.tx.wq.wake_all();
                end.rx.wq.wake_all();
                crate::fs::poll::notify();
            }
        }
    }
}

impl Drop for KernelSocket {
    fn drop(&mut self) {
        self.close();
    }
}
