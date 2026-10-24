// SPDX-License-Identifier: GPL-2.0

//! UNIX Socket Abstraction wrapping smoltcp Sockets, Unix domain sockets, and Netlink sockets.

use alloc::{collections::VecDeque, string::String, sync::Arc};
use ostd::{sync::WaitQueue, task::Task};
use smoltcp::{
    iface::SocketHandle,
    socket::{tcp::Socket as TcpSocket, tcp::State as TcpState, udp::Socket as UdpSocket},
    wire::{IpAddress, IpEndpoint, IpListenEndpoint, Ipv4Address},
};
use spin::Mutex;

use crate::{
    net::interface::NET_MANAGER,
    uapi::errno::{Errno, Result},
};

// Standard socket domains
pub const AF_UNIX: i32 = 1;
pub const AF_INET: i32 = 2;
pub const AF_INET6: i32 = 10;
pub const AF_NETLINK: i32 = 16;

// Standard socket types
pub const SOCK_STREAM: i32 = 1;
pub const SOCK_DGRAM: i32 = 2;
pub const SOCK_RAW: i32 = 3;

// Standard protocols
pub const IPPROTO_IP: i32 = 0;
pub const IPPROTO_TCP: i32 = 6;
pub const IPPROTO_UDP: i32 = 17;
pub const NETLINK_ROUTE: i32 = 0;

pub const UNIX_SOCKET_CAPACITY: usize = 65536;

/// Universal socket endpoint address representation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SocketAddress {
    Inet(IpEndpoint),
    Unix(String),
    Netlink { pid: u32, groups: u32 },
}

impl SocketAddress {
    pub fn as_inet(&self) -> Option<IpEndpoint> {
        match self {
            Self::Inet(ep) => Some(*ep),
            _ => None,
        }
    }
}

static NEXT_EPHEMERAL_PORT: core::sync::atomic::AtomicU16 =
    core::sync::atomic::AtomicU16::new(49152);

/// Allocates an ephemeral port in the dynamic range [49152, 65535] per RFC 6056.
///
/// Uses an atomic monotonic counter to prevent port collisions between consecutive connections.
pub fn alloc_ephemeral_port() -> u16 {
    let port = NEXT_EPHEMERAL_PORT.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    if port >= 65530 {
        NEXT_EPHEMERAL_PORT.store(49152, core::sync::atomic::Ordering::Relaxed);
    }
    if port < 49152 { 49152 } else { port }
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
#[derive(Clone, Debug)]
pub enum SocketKind {
    Tcp(SocketHandle),
    Udp(SocketHandle),
    Unix(Arc<UnixSocketEnd>),
    UnixUnconnected { sock_type: i32 },
    UnixListener(Arc<crate::net::unix::UnixListenerState>),
    UnixDgram(Arc<crate::net::unix::UnixDgramState>),
    Netlink(Arc<crate::net::netlink::NetlinkSocketState>),
}

/// In-kernel representation of an open network socket.
pub struct KernelSocket {
    pub domain: i32,
    pub sock_type: i32,
    pub protocol: i32,
    pub kind: Arc<Mutex<SocketKind>>,
    pub nonblocking: Mutex<bool>,
    pub local_endpoint: Mutex<Option<IpEndpoint>>,
    pub remote_endpoint: Mutex<Option<IpEndpoint>>,
    pub local_addr: Mutex<Option<SocketAddress>>,
    pub remote_addr: Mutex<Option<SocketAddress>>,
}

impl KernelSocket {
    /// Creates a new TCP socket.
    pub fn new_tcp(domain: i32) -> Result<Arc<Self>> {
        let handle = NET_MANAGER.lock().add_tcp_socket();
        Ok(Arc::new(Self {
            domain,
            sock_type: SOCK_STREAM,
            protocol: IPPROTO_TCP,
            kind: Arc::new(Mutex::new(SocketKind::Tcp(handle))),
            nonblocking: Mutex::new(false),
            local_endpoint: Mutex::new(None),
            remote_endpoint: Mutex::new(None),
            local_addr: Mutex::new(None),
            remote_addr: Mutex::new(None),
        }))
    }

    /// Creates a new UDP socket.
    pub fn new_udp(domain: i32) -> Result<Arc<Self>> {
        let handle = NET_MANAGER.lock().add_udp_socket();
        Ok(Arc::new(Self {
            domain,
            sock_type: SOCK_DGRAM,
            protocol: IPPROTO_UDP,
            kind: Arc::new(Mutex::new(SocketKind::Udp(handle))),
            nonblocking: Mutex::new(false),
            local_endpoint: Mutex::new(None),
            remote_endpoint: Mutex::new(None),
            local_addr: Mutex::new(None),
            remote_addr: Mutex::new(None),
        }))
    }

    /// Creates a new, unconnected UNIX domain socket.
    pub fn new_unix(sock_type: i32) -> Result<Arc<Self>> {
        let kind = match sock_type {
            SOCK_STREAM => SocketKind::UnixUnconnected { sock_type },
            SOCK_DGRAM => SocketKind::UnixDgram(Arc::new(crate::net::unix::UnixDgramState::new())),
            _ => crate::return_errno!(ESOCKTNOSUPPORT, "unsupported unix socket type"),
        };
        Ok(Arc::new(Self {
            domain: AF_UNIX,
            sock_type,
            protocol: 0,
            kind: Arc::new(Mutex::new(kind)),
            nonblocking: Mutex::new(false),
            local_endpoint: Mutex::new(None),
            remote_endpoint: Mutex::new(None),
            local_addr: Mutex::new(None),
            remote_addr: Mutex::new(None),
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
            kind: Arc::new(Mutex::new(SocketKind::Unix(end_a))),
            nonblocking: Mutex::new(false),
            local_endpoint: Mutex::new(None),
            remote_endpoint: Mutex::new(None),
            local_addr: Mutex::new(None),
            remote_addr: Mutex::new(None),
        });

        let sock_b = Arc::new(Self {
            domain: AF_UNIX,
            sock_type,
            protocol: 0,
            kind: Arc::new(Mutex::new(SocketKind::Unix(end_b))),
            nonblocking: Mutex::new(false),
            local_endpoint: Mutex::new(None),
            remote_endpoint: Mutex::new(None),
            local_addr: Mutex::new(None),
            remote_addr: Mutex::new(None),
        });

        Ok((sock_a, sock_b))
    }

    /// Creates a new Netlink route socket.
    pub fn new_netlink(protocol: i32) -> Result<Arc<Self>> {
        let state = Arc::new(crate::net::netlink::NetlinkSocketState::new(protocol));
        Ok(Arc::new(Self {
            domain: AF_NETLINK,
            sock_type: SOCK_RAW,
            protocol,
            kind: Arc::new(Mutex::new(SocketKind::Netlink(state))),
            nonblocking: Mutex::new(false),
            local_endpoint: Mutex::new(None),
            remote_endpoint: Mutex::new(None),
            local_addr: Mutex::new(Some(SocketAddress::Netlink { pid: 0, groups: 0 })),
            remote_addr: Mutex::new(None),
        }))
    }

    /// Clones internal state reference for registering into the UNIX socket table.
    fn clone_socket_ref(&self) -> Self {
        Self {
            domain: self.domain,
            sock_type: self.sock_type,
            protocol: self.protocol,
            kind: self.kind.clone(),
            nonblocking: Mutex::new(*self.nonblocking.lock()),
            local_endpoint: Mutex::new(*self.local_endpoint.lock()),
            remote_endpoint: Mutex::new(*self.remote_endpoint.lock()),
            local_addr: Mutex::new(self.local_addr.lock().clone()),
            remote_addr: Mutex::new(self.remote_addr.lock().clone()),
        }
    }

    /// Binds the socket to a local address.
    pub fn bind(&self, addr: SocketAddress) -> Result<()> {
        let kind = self.kind.lock().clone();
        match (kind, addr) {
            (SocketKind::Tcp(_), SocketAddress::Inet(ep)) => {
                let port = if ep.port == 0 {
                    alloc_ephemeral_port()
                } else {
                    ep.port
                };
                let effective_ep = IpEndpoint::new(ep.addr, port);
                *self.local_endpoint.lock() = Some(effective_ep);
                *self.local_addr.lock() = Some(SocketAddress::Inet(effective_ep));
                Ok(())
            }
            (SocketKind::Udp(handle), SocketAddress::Inet(ep)) => {
                let port = if ep.port == 0 {
                    alloc_ephemeral_port()
                } else {
                    ep.port
                };
                let bind_ep = if ep.addr.is_unspecified() {
                    IpListenEndpoint { addr: None, port }
                } else {
                    IpListenEndpoint {
                        addr: Some(ep.addr),
                        port,
                    }
                };
                let mut net = NET_MANAGER.lock();
                let socket = net.get_sockets().get_mut::<UdpSocket>(handle);
                socket.bind(bind_ep).map_err(|_| Errno::EADDRINUSE)?;
                let effective_ep = IpEndpoint::new(ep.addr, port);
                *self.local_endpoint.lock() = Some(effective_ep);
                *self.local_addr.lock() = Some(SocketAddress::Inet(effective_ep));
                Ok(())
            }
            (SocketKind::UnixUnconnected { .. }, SocketAddress::Unix(ref path))
            | (SocketKind::UnixListener(_), SocketAddress::Unix(ref path))
            | (SocketKind::Unix(_), SocketAddress::Unix(ref path))
            | (SocketKind::UnixDgram(_), SocketAddress::Unix(ref path)) => {
                if !path.is_empty() {
                    let registered_sock = Arc::new(self.clone_socket_ref());
                    crate::net::unix::bind_unix_socket(path, registered_sock)?;
                }
                *self.local_addr.lock() = Some(SocketAddress::Unix(path.clone()));
                Ok(())
            }
            (SocketKind::Netlink(ref state), SocketAddress::Netlink { pid, groups }) => {
                *state.pid.lock() = pid;
                *state.groups.lock() = groups;
                *self.local_addr.lock() = Some(SocketAddress::Netlink { pid, groups });
                Ok(())
            }
            _ => Err(Errno::EINVAL),
        }
    }

    /// Listens for incoming connections (TCP and Unix domain stream sockets).
    pub fn listen(&self, backlog: i32) -> Result<()> {
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
            SocketKind::UnixUnconnected { .. } => {
                let listener = Arc::new(crate::net::unix::UnixListenerState::new(
                    backlog.max(1) as usize
                ));
                *self.kind.lock() = SocketKind::UnixListener(listener);
                Ok(())
            }
            SocketKind::UnixListener(ref listener) => {
                *listener.backlog.lock() = backlog.max(1) as usize;
                Ok(())
            }
            SocketKind::Unix(_) | SocketKind::UnixDgram(_) | SocketKind::Netlink(_) => Ok(()),
        }
    }

    /// Connects to a remote address.
    pub fn connect(&self, addr: SocketAddress) -> Result<()> {
        let kind = self.kind.lock().clone();
        match (kind, addr) {
            (SocketKind::Tcp(handle), SocketAddress::Inet(endpoint)) => {
                let local_port = self
                    .local_endpoint
                    .lock()
                    .map(|ep| ep.port)
                    .unwrap_or_else(alloc_ephemeral_port);

                ostd::info!("connect: TCP connecting to {:?}", endpoint);
                let mut net = NET_MANAGER.lock();
                net.connect_tcp(handle, endpoint, local_port)?;
                net.poll();
                *self.remote_endpoint.lock() = Some(endpoint);
                *self.remote_addr.lock() = Some(SocketAddress::Inet(endpoint));
                drop(net);

                // If blocking, wait until connection is established
                if !*self.nonblocking.lock() {
                    loop {
                        let mut net = NET_MANAGER.lock();
                        net.poll();
                        let socket = net.get_sockets().get_mut::<TcpSocket>(handle);
                        match socket.state() {
                            TcpState::Established => {
                                ostd::info!("connect: TCP established");
                                break;
                            }
                            TcpState::Closed | TcpState::TimeWait => {
                                ostd::info!("connect: TCP closed/refused");
                                return Err(Errno::ECONNREFUSED);
                            }
                            _ => {}
                        }
                        drop(net);
                        Task::yield_now();
                    }
                } else {
                    let mut net = NET_MANAGER.lock();
                    net.poll();
                    let socket = net.get_sockets().get_mut::<TcpSocket>(handle);
                    ostd::info!("connect: nonblocking state={:?}", socket.state());
                    if socket.state() == TcpState::Closed || socket.state() == TcpState::TimeWait {
                        return Err(Errno::ECONNREFUSED);
                    } else if socket.state() != TcpState::Established {
                        return Err(Errno::EINPROGRESS);
                    }
                }
                Ok(())
            }
            (SocketKind::Udp(handle), SocketAddress::Inet(endpoint)) => {
                let mut net = NET_MANAGER.lock();
                let socket = net.get_sockets().get_mut::<UdpSocket>(handle);
                if !socket.is_open() {
                    let port = alloc_ephemeral_port();
                    socket.bind(port).map_err(|_| Errno::EADDRINUSE)?;
                    let effective_ep =
                        IpEndpoint::new(IpAddress::Ipv4(Ipv4Address::UNSPECIFIED), port);
                    *self.local_endpoint.lock() = Some(effective_ep);
                    *self.local_addr.lock() = Some(SocketAddress::Inet(effective_ep));
                }
                *self.remote_endpoint.lock() = Some(endpoint);
                *self.remote_addr.lock() = Some(SocketAddress::Inet(endpoint));
                Ok(())
            }
            (SocketKind::UnixUnconnected { .. }, SocketAddress::Unix(ref path))
            | (SocketKind::UnixDgram(_), SocketAddress::Unix(ref path)) => {
                let target_sock =
                    crate::net::unix::lookup_unix_socket(path).ok_or(Errno::ECONNREFUSED)?;
                let target_kind = target_sock.kind.lock().clone();
                match target_kind {
                    SocketKind::UnixListener(ref listener) => {
                        let chan_c2s = Arc::new(UnixChannel::new());
                        let chan_s2c = Arc::new(UnixChannel::new());
                        let client_end = Arc::new(UnixSocketEnd {
                            tx: chan_c2s.clone(),
                            rx: chan_s2c.clone(),
                        });
                        let server_end = Arc::new(UnixSocketEnd {
                            tx: chan_s2c,
                            rx: chan_c2s,
                        });
                        let my_path = self.local_addr.lock().clone().and_then(|a| match a {
                            SocketAddress::Unix(p) => Some(p),
                            _ => None,
                        });
                        let server_sock = Arc::new(KernelSocket {
                            domain: AF_UNIX,
                            sock_type: SOCK_STREAM,
                            protocol: 0,
                            kind: Arc::new(Mutex::new(SocketKind::Unix(server_end))),
                            nonblocking: Mutex::new(false),
                            local_endpoint: Mutex::new(None),
                            remote_endpoint: Mutex::new(None),
                            local_addr: Mutex::new(Some(SocketAddress::Unix(path.clone()))),
                            remote_addr: Mutex::new(
                                my_path.as_ref().map(|p| SocketAddress::Unix(p.clone())),
                            ),
                        });
                        *self.kind.lock() = SocketKind::Unix(client_end);
                        *self.remote_addr.lock() = Some(SocketAddress::Unix(path.clone()));

                        listener
                            .accept_queue
                            .lock()
                            .push_back((server_sock, my_path));
                        listener.wq.wake_all();
                        crate::fs::poll::notify();
                        Ok(())
                    }
                    SocketKind::UnixDgram(_) => {
                        *self.remote_addr.lock() = Some(SocketAddress::Unix(path.clone()));
                        Ok(())
                    }
                    _ => Err(Errno::ECONNREFUSED),
                }
            }
            (SocketKind::Netlink(_), SocketAddress::Netlink { pid, groups }) => {
                *self.remote_addr.lock() = Some(SocketAddress::Netlink { pid, groups });
                Ok(())
            }
            _ => Err(Errno::EINVAL),
        }
    }

    /// Accepts an incoming connection on a listening TCP or Unix domain socket.
    pub fn accept(&self) -> Result<(Arc<KernelSocket>, SocketAddress)> {
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
                        let new_listen_handle = net.add_tcp_socket();
                        let new_socket = net.get_sockets().get_mut::<TcpSocket>(new_listen_handle);
                        let _ = new_socket.listen(local);

                        let accepted_sock = Arc::new(KernelSocket {
                            domain: self.domain,
                            sock_type: SOCK_STREAM,
                            protocol: IPPROTO_TCP,
                            kind: Arc::new(Mutex::new(SocketKind::Tcp(listen_handle))),
                            nonblocking: Mutex::new(false),
                            local_endpoint: Mutex::new(Some(local)),
                            remote_endpoint: Mutex::new(Some(remote)),
                            local_addr: Mutex::new(Some(SocketAddress::Inet(local))),
                            remote_addr: Mutex::new(Some(SocketAddress::Inet(remote))),
                        });

                        *self.kind.lock() = SocketKind::Tcp(new_listen_handle);

                        return Ok((accepted_sock, SocketAddress::Inet(remote)));
                    }

                    if *self.nonblocking.lock() {
                        return Err(Errno::EWOULDBLOCK);
                    }

                    drop(net);
                    Task::yield_now();
                }
            }
            SocketKind::UnixListener(ref listener) => loop {
                if let Some((client_sock, remote_path)) = listener.accept_queue.lock().pop_front() {
                    let addr = SocketAddress::Unix(remote_path.unwrap_or_default());
                    return Ok((client_sock, addr));
                }

                if *self.nonblocking.lock() {
                    return Err(Errno::EWOULDBLOCK);
                }

                let step = || listener.accept_queue.lock().pop_front();
                let (client_sock, remote_path) = listener.wq.wait_until(step);
                let addr = SocketAddress::Unix(remote_path.unwrap_or_default());
                return Ok((client_sock, addr));
            },
            SocketKind::Udp(_) => {
                crate::return_errno!(EOPNOTSUPP, "accept not supported on UDP socket")
            }
            _ => crate::return_errno!(EINVAL, "accept not supported on this socket"),
        }
    }

    /// Gets local socket address.
    pub fn getsockname(&self) -> Result<SocketAddress> {
        if let Some(addr) = self.local_addr.lock().clone() {
            return Ok(addr);
        }
        let kind = self.kind.lock().clone();
        match kind {
            SocketKind::Tcp(handle) => {
                let mut net = NET_MANAGER.lock();
                let socket = net.get_sockets().get_mut::<TcpSocket>(handle);
                if let Some(ep) = socket.local_endpoint() {
                    Ok(SocketAddress::Inet(ep))
                } else if let Some(ep) = *self.local_endpoint.lock() {
                    Ok(SocketAddress::Inet(ep))
                } else {
                    Err(Errno::ENOTCONN)
                }
            }
            SocketKind::Udp(handle) => {
                let mut net = NET_MANAGER.lock();
                let socket = net.get_sockets().get_mut::<UdpSocket>(handle);
                let ep = socket.endpoint();
                if ep.port != 0 {
                    let addr = ep.addr.unwrap_or(IpAddress::Ipv4(Ipv4Address::UNSPECIFIED));
                    Ok(SocketAddress::Inet(IpEndpoint::new(addr, ep.port)))
                } else if let Some(ep) = *self.local_endpoint.lock() {
                    Ok(SocketAddress::Inet(ep))
                } else {
                    Ok(SocketAddress::Inet(IpEndpoint::new(
                        IpAddress::Ipv4(Ipv4Address::UNSPECIFIED),
                        0,
                    )))
                }
            }
            _ => Err(Errno::ENOTCONN),
        }
    }

    /// Gets peer socket address.
    pub fn getpeername(&self) -> Result<SocketAddress> {
        if let Some(addr) = self.remote_addr.lock().clone() {
            return Ok(addr);
        }
        let kind = self.kind.lock().clone();
        match kind {
            SocketKind::Tcp(handle) => {
                let mut net = NET_MANAGER.lock();
                let socket = net.get_sockets().get_mut::<TcpSocket>(handle);
                if let Some(ep) = socket.remote_endpoint() {
                    Ok(SocketAddress::Inet(ep))
                } else if let Some(ep) = *self.remote_endpoint.lock() {
                    Ok(SocketAddress::Inet(ep))
                } else {
                    Err(Errno::ENOTCONN)
                }
            }
            SocketKind::Udp(_) => self
                .remote_endpoint
                .lock()
                .map(SocketAddress::Inet)
                .ok_or(Errno::ENOTCONN),
            _ => Err(Errno::ENOTCONN),
        }
    }

    /// Sends data through the socket.
    pub fn send(&self, data: &[u8], flags: i32) -> Result<usize> {
        let nonblocking = *self.nonblocking.lock() || (flags & 0x40) != 0;
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

                if nonblocking {
                    return Err(Errno::EWOULDBLOCK);
                }

                drop(net);
                Task::yield_now();
            },
            SocketKind::Udp(handle) => {
                let remote = self.remote_endpoint.lock().ok_or(Errno::EDESTADDRREQ)?;
                let mut net = NET_MANAGER.lock();
                let socket = net.get_sockets().get_mut::<UdpSocket>(handle);
                if !socket.is_open() {
                    let port = alloc_ephemeral_port();
                    socket.bind(port).map_err(|_| Errno::EADDRINUSE)?;
                    let effective_ep =
                        IpEndpoint::new(IpAddress::Ipv4(Ipv4Address::UNSPECIFIED), port);
                    *self.local_endpoint.lock() = Some(effective_ep);
                    *self.local_addr.lock() = Some(SocketAddress::Inet(effective_ep));
                }
                if socket.can_send() {
                    socket
                        .send_slice(data, remote)
                        .map_err(|_| Errno::ENOBUFS)?;
                    net.poll();
                    Ok(data.len())
                } else if nonblocking {
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
                        let _ = proc.send_signal(
                            crate::uapi::signal::SigInfo::from_user(
                                crate::uapi::signal::Signal::SIGPIPE,
                                0,
                            ),
                            0,
                        );
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
                        None if nonblocking => {
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
                                let _ = proc.send_signal(
                                    crate::uapi::signal::SigInfo::from_user(
                                        crate::uapi::signal::Signal::SIGPIPE,
                                        0,
                                    ),
                                    0,
                                );
                            }
                            return Err(e);
                        }
                    }
                }
                Ok(written)
            }
            SocketKind::UnixDgram(_) => {
                if let Some(target) = self.remote_addr.lock().clone() {
                    self.sendto(data, Some(target), flags)
                } else {
                    Err(Errno::EDESTADDRREQ)
                }
            }
            SocketKind::Netlink(ref state) => {
                state.handle_request(data);
                Ok(data.len())
            }
            _ => Err(Errno::ENOTCONN),
        }
    }

    /// Receives data from the socket.
    pub fn recv(&self, buf: &mut [u8], flags: i32) -> Result<usize> {
        let nonblocking = *self.nonblocking.lock() || (flags & 0x40) != 0;
        let kind = self.kind.lock().clone();
        match kind {
            SocketKind::Tcp(handle) => loop {
                let mut net = NET_MANAGER.lock();
                net.poll();
                let socket = net.get_sockets().get_mut::<TcpSocket>(handle);

                if socket.can_recv() {
                    let n = socket.recv_slice(buf).map_err(|_| Errno::EIO)?;
                    net.poll();
                    crate::fs::poll::notify();
                    return Ok(n);
                }

                match socket.state() {
                    TcpState::Closed
                    | TcpState::CloseWait
                    | TcpState::TimeWait
                    | TcpState::LastAck
                    | TcpState::Closing => {
                        return Ok(0); // EOF
                    }
                    _ => {}
                }

                if nonblocking {
                    return Err(Errno::EWOULDBLOCK);
                }

                drop(net);
                Task::yield_now();
            },
            SocketKind::Udp(handle) => loop {
                let mut net = NET_MANAGER.lock();
                let socket = net.get_sockets().get_mut::<UdpSocket>(handle);
                if !socket.is_open() {
                    let port = alloc_ephemeral_port();
                    socket.bind(port).map_err(|_| Errno::EADDRINUSE)?;
                    let effective_ep =
                        IpEndpoint::new(IpAddress::Ipv4(Ipv4Address::UNSPECIFIED), port);
                    *self.local_endpoint.lock() = Some(effective_ep);
                    *self.local_addr.lock() = Some(SocketAddress::Inet(effective_ep));
                }
                net.poll();
                let socket = net.get_sockets().get_mut::<UdpSocket>(handle);

                if socket.can_recv() {
                    let (n, meta) = socket.recv_slice(buf).map_err(|_| Errno::EIO)?;
                    *self.remote_endpoint.lock() = Some(meta.endpoint);
                    *self.remote_addr.lock() = Some(SocketAddress::Inet(meta.endpoint));
                    return Ok(n);
                }

                if nonblocking {
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
                    None if nonblocking => return Err(Errno::EWOULDBLOCK),
                    None => end.rx.wq.wait_until(try_read),
                };
                if n > 0 {
                    end.rx.wq.wake_all();
                    crate::fs::poll::notify();
                }
                Ok(n)
            }
            SocketKind::UnixDgram(_) => {
                let (n, _) = self.recvfrom(buf, flags)?;
                Ok(n)
            }
            SocketKind::Netlink(ref state) => loop {
                let mut rx = state.rx_buf.lock();
                if !rx.is_empty() {
                    let n = buf.len().min(rx.len());
                    for (slot, byte) in buf.iter_mut().zip(rx.drain(..n)) {
                        *slot = byte;
                    }
                    return Ok(n);
                }
                if nonblocking {
                    return Err(Errno::EWOULDBLOCK);
                }
                drop(rx);
                state.wq.wait_until(|| {
                    let rx = state.rx_buf.lock();
                    if !rx.is_empty() { Some(()) } else { None }
                });
            },
            _ => Err(Errno::ENOTCONN),
        }
    }

    /// Sends a datagram to a specific destination endpoint.
    pub fn sendto(&self, data: &[u8], dest: Option<SocketAddress>, flags: i32) -> Result<usize> {
        let nonblocking = *self.nonblocking.lock() || (flags & 0x40) != 0;
        let kind = self.kind.lock().clone();
        match kind {
            SocketKind::Udp(handle) => {
                let target = dest
                    .and_then(|a| a.as_inet())
                    .or(*self.remote_endpoint.lock())
                    .ok_or(Errno::EDESTADDRREQ)?;
                let mut net = NET_MANAGER.lock();
                let socket = net.get_sockets().get_mut::<UdpSocket>(handle);
                if !socket.is_open() {
                    let port = alloc_ephemeral_port();
                    socket.bind(port).map_err(|_| Errno::EADDRINUSE)?;
                    let effective_ep =
                        IpEndpoint::new(IpAddress::Ipv4(Ipv4Address::UNSPECIFIED), port);
                    *self.local_endpoint.lock() = Some(effective_ep);
                    *self.local_addr.lock() = Some(SocketAddress::Inet(effective_ep));
                }
                if socket.can_send() {
                    socket
                        .send_slice(data, target)
                        .map_err(|_| Errno::ENOBUFS)?;
                    net.poll();
                    Ok(data.len())
                } else if nonblocking {
                    Err(Errno::EWOULDBLOCK)
                } else {
                    Err(Errno::ENOBUFS)
                }
            }
            SocketKind::UnixDgram(_) => {
                let target = dest
                    .or_else(|| self.remote_addr.lock().clone())
                    .ok_or(Errno::EDESTADDRREQ)?;
                if let SocketAddress::Unix(ref path) = target {
                    let target_sock =
                        crate::net::unix::lookup_unix_socket(path).ok_or(Errno::ECONNREFUSED)?;
                    let my_path = self.local_addr.lock().clone().and_then(|a| match a {
                        SocketAddress::Unix(p) => Some(p),
                        _ => None,
                    });
                    if let SocketKind::UnixDgram(ref dgram) = *target_sock.kind.lock() {
                        dgram.rx_queue.lock().push_back((data.to_vec(), my_path));
                        dgram.wq.wake_all();
                        crate::fs::poll::notify();
                        Ok(data.len())
                    } else {
                        Err(Errno::ECONNREFUSED)
                    }
                } else {
                    Err(Errno::EINVAL)
                }
            }
            SocketKind::Netlink(ref state) => {
                state.handle_request(data);
                Ok(data.len())
            }
            SocketKind::Tcp(_) | SocketKind::Unix(_) => self.send(data, flags),
            _ => Err(Errno::ENOTCONN),
        }
    }

    /// Receives a datagram along with the sender endpoint.
    pub fn recvfrom(&self, buf: &mut [u8], flags: i32) -> Result<(usize, Option<SocketAddress>)> {
        let nonblocking = *self.nonblocking.lock() || (flags & 0x40) != 0;
        let kind = self.kind.lock().clone();
        match kind {
            SocketKind::Udp(handle) => loop {
                let mut net = NET_MANAGER.lock();
                let socket = net.get_sockets().get_mut::<UdpSocket>(handle);
                if !socket.is_open() {
                    let port = alloc_ephemeral_port();
                    socket.bind(port).map_err(|_| Errno::EADDRINUSE)?;
                    let effective_ep =
                        IpEndpoint::new(IpAddress::Ipv4(Ipv4Address::UNSPECIFIED), port);
                    *self.local_endpoint.lock() = Some(effective_ep);
                    *self.local_addr.lock() = Some(SocketAddress::Inet(effective_ep));
                }
                net.poll();
                let socket = net.get_sockets().get_mut::<UdpSocket>(handle);

                if socket.can_recv() {
                    let (n, meta) = socket.recv_slice(buf).map_err(|_| Errno::EIO)?;
                    *self.remote_endpoint.lock() = Some(meta.endpoint);
                    *self.remote_addr.lock() = Some(SocketAddress::Inet(meta.endpoint));
                    return Ok((n, Some(SocketAddress::Inet(meta.endpoint))));
                }

                if nonblocking {
                    return Err(Errno::EWOULDBLOCK);
                }

                drop(net);
                Task::yield_now();
            },
            SocketKind::UnixDgram(ref dgram) => loop {
                let mut rx = dgram.rx_queue.lock();
                if let Some((packet, sender_path)) = rx.pop_front() {
                    let n = buf.len().min(packet.len());
                    buf[..n].copy_from_slice(&packet[..n]);
                    let addr = sender_path.map(SocketAddress::Unix);
                    return Ok((n, addr));
                }
                if nonblocking {
                    return Err(Errno::EWOULDBLOCK);
                }
                drop(rx);
                dgram.wq.wait_until(|| {
                    let rx = dgram.rx_queue.lock();
                    if !rx.is_empty() { Some(()) } else { None }
                });
            },
            SocketKind::Netlink(ref state) => {
                let n = self.recv(buf, flags)?;
                let pid = *state.pid.lock();
                let groups = *state.groups.lock();
                Ok((n, Some(SocketAddress::Netlink { pid, groups })))
            }
            SocketKind::Tcp(_) => {
                let n = self.recv(buf, flags)?;
                let peer = self.remote_addr.lock().clone();
                Ok((n, peer))
            }
            SocketKind::Unix(_) => {
                let n = self.recv(buf, flags)?;
                let peer = self.remote_addr.lock().clone();
                Ok((n, peer))
            }
            _ => Err(Errno::ENOTCONN),
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
            SocketKind::Udp(_) | SocketKind::UnixDgram(_) | SocketKind::Netlink(_) => Ok(()),
            _ => Ok(()),
        }
    }

    /// Closes the socket and removes it from the global socket set.
    pub fn close(&self) {
        if let Some(SocketAddress::Unix(ref path)) = *self.local_addr.lock() {
            crate::net::unix::unbind_unix_socket(path);
        }

        // Take the kind out for smoltcp sockets so a second close() (explicit
        // close followed by Drop) never touches an already-removed handle.
        let kind = {
            let mut guard = self.kind.lock();
            let old = guard.clone();
            if matches!(old, SocketKind::Tcp(_) | SocketKind::Udp(_)) {
                *guard = SocketKind::UnixUnconnected {
                    sock_type: self.sock_type,
                };
            }
            old
        };
        match kind {
            SocketKind::Tcp(handle) => {
                let mut net = NET_MANAGER.lock();
                let socket = net.get_sockets().get_mut::<TcpSocket>(handle);
                socket.abort();
                net.poll();
                net.remove_socket(handle);
            }
            SocketKind::Udp(handle) => {
                let mut net = NET_MANAGER.lock();
                let socket = net.get_sockets().get_mut::<UdpSocket>(handle);
                socket.close();
                net.poll();
                net.remove_socket(handle);
            }
            SocketKind::Unix(ref end) => {
                *end.tx.closed.lock() = true;
                *end.rx.closed.lock() = true;
                end.tx.wq.wake_all();
                end.rx.wq.wake_all();
                crate::fs::poll::notify();
            }
            SocketKind::UnixListener(ref listener) => {
                listener.wq.wake_all();
            }
            SocketKind::UnixDgram(ref dgram) => {
                dgram.wq.wake_all();
            }
            SocketKind::Netlink(ref state) => {
                state.wq.wake_all();
            }
            SocketKind::UnixUnconnected { .. } => {}
        }
    }
}

impl Drop for KernelSocket {
    fn drop(&mut self) {
        self.close();
    }
}
