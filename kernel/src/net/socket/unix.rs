//! UNIX Domain Socket Subsystem (AF_UNIX / AF_LOCAL)
//!
//! Provides local inter-process communication via bidirectional stream channels,
//! datagram queues, socketpair allocation, and filesystem path-based socket binding.

use alloc::collections::{BTreeMap, VecDeque};
use alloc::string::String;
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;

use crate::net::types::*;
use crate::sync::Mutex;
use crate::syscalls::SyscallError;

use super::Socket;

const UNIX_STREAM_BUF_CAPACITY: usize = 65536;

/// Global path registry for filesystem-bound UNIX domain listening sockets.
pub static UNIX_REGISTRY: Mutex<BTreeMap<String, Weak<Mutex<UnixSocket>>>> =
    Mutex::new(BTreeMap::new());

/// Shared FIFO ring buffer for local stream sockets.
pub struct UnixStreamBuffer {
    pub data: VecDeque<u8>,
    pub capacity: usize,
    pub closed: bool,
}

impl UnixStreamBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            data: VecDeque::with_capacity(core::cmp::min(capacity, 4096)),
            capacity,
            closed: false,
        }
    }
}

/// Datagram item in a UNIX domain datagram socket.
pub struct UnixDatagram {
    pub sender_path: Option<String>,
    pub payload: Vec<u8>,
}

/// UNIX Domain Socket representation.
pub struct UnixSocket {
    pub socket_type: i32, // SOCK_STREAM or SOCK_DGRAM
    pub bound_path: Option<String>,
    pub is_listening: bool,
    pub backlog: usize,
    pub pending_conns: VecDeque<Arc<Mutex<UnixSocket>>>,
    // For connected stream sockets:
    pub rx_buffer: Arc<Mutex<UnixStreamBuffer>>,
    pub tx_buffer: Arc<Mutex<UnixStreamBuffer>>,
    pub peer: Option<Weak<Mutex<UnixSocket>>>,
    // For datagram sockets:
    pub dgram_queue: VecDeque<UnixDatagram>,
    pub nonblocking: bool,
    pub shutdown_read: bool,
    pub shutdown_write: bool,
}

impl UnixSocket {
    /// Create a new unconnected UNIX domain socket.
    pub fn new(socket_type: i32, nonblocking: bool) -> Self {
        let rx_buf = Arc::new(Mutex::new(UnixStreamBuffer::new(
            UNIX_STREAM_BUF_CAPACITY,
        )));
        let tx_buf = Arc::new(Mutex::new(UnixStreamBuffer::new(
            UNIX_STREAM_BUF_CAPACITY,
        )));

        Self {
            socket_type,
            bound_path: None,
            is_listening: false,
            backlog: 128,
            pending_conns: VecDeque::new(),
            rx_buffer: rx_buf,
            tx_buffer: tx_buf,
            peer: None,
            dgram_queue: VecDeque::new(),
            nonblocking,
            shutdown_read: false,
            shutdown_write: false,
        }
    }

    /// Allocate a pair of connected bidirectional stream UNIX sockets (`socketpair`).
    pub fn create_pair(
        socket_type: i32,
        nonblocking: bool,
    ) -> (Arc<Mutex<Self>>, Arc<Mutex<Self>>) {
        let buf_a_to_b = Arc::new(Mutex::new(UnixStreamBuffer::new(
            UNIX_STREAM_BUF_CAPACITY,
        )));
        let buf_b_to_a = Arc::new(Mutex::new(UnixStreamBuffer::new(
            UNIX_STREAM_BUF_CAPACITY,
        )));

        let sock_a = Arc::new(Mutex::new(Self {
            socket_type,
            bound_path: None,
            is_listening: false,
            backlog: 0,
            pending_conns: VecDeque::new(),
            rx_buffer: buf_b_to_a.clone(),
            tx_buffer: buf_a_to_b.clone(),
            peer: None,
            dgram_queue: VecDeque::new(),
            nonblocking,
            shutdown_read: false,
            shutdown_write: false,
        }));

        let sock_b = Arc::new(Mutex::new(Self {
            socket_type,
            bound_path: None,
            is_listening: false,
            backlog: 0,
            pending_conns: VecDeque::new(),
            rx_buffer: buf_a_to_b,
            tx_buffer: buf_b_to_a,
            peer: Some(Arc::downgrade(&sock_a)),
            dgram_queue: VecDeque::new(),
            nonblocking,
            shutdown_read: false,
            shutdown_write: false,
        }));

        sock_a.lock().peer = Some(Arc::downgrade(&sock_b));

        (sock_a, sock_b)
    }

    /// Bind socket to a filesystem path.
    pub fn bind(&mut self, self_arc: &Arc<Mutex<Self>>, path: &str) -> Result<(), SyscallError> {
        if self.bound_path.is_some() {
            return Err(SyscallError::EINVAL);
        }

        let mut reg = UNIX_REGISTRY.lock();
        if reg.contains_key(path) {
            // Check if existing socket is alive
            if let Some(weak) = reg.get(path) {
                if weak.upgrade().is_some() {
                    return Err(SyscallError::EADDRINUSE);
                }
            }
        }

        let path_string = String::from(path);
        reg.insert(path_string.clone(), Arc::downgrade(self_arc));
        self.bound_path = Some(path_string);

        Ok(())
    }

    /// Mark socket as listening for stream connections.
    pub fn listen(&mut self, backlog: usize) -> Result<(), SyscallError> {
        if self.socket_type != SOCK_STREAM {
            return Err(SyscallError::EOPNOTSUPP);
        }
        if self.bound_path.is_none() {
            return Err(SyscallError::EDESTADDRREQ);
        }

        self.is_listening = true;
        self.backlog = if backlog == 0 { 128 } else { backlog };
        Ok(())
    }

    /// Connect to a listening UNIX domain socket at `path`.
    pub fn connect(
        &mut self,
        self_arc: &Arc<Mutex<Self>>,
        path: &str,
    ) -> Result<(), SyscallError> {
        let listener_arc = {
            let mut reg = UNIX_REGISTRY.lock();
            let weak = reg.get(path).ok_or(SyscallError::ECONNREFUSED)?;
            weak.upgrade().ok_or_else(|| {
                reg.remove(path);
                SyscallError::ECONNREFUSED
            })?
        };

        if self.socket_type == SOCK_STREAM {
            let mut listener = listener_arc.lock();
            if !listener.is_listening {
                return Err(SyscallError::ECONNREFUSED);
            }

            // Create client <-> server stream buffers
            let client_to_server = Arc::new(Mutex::new(UnixStreamBuffer::new(
                UNIX_STREAM_BUF_CAPACITY,
            )));
            let server_to_client = Arc::new(Mutex::new(UnixStreamBuffer::new(
                UNIX_STREAM_BUF_CAPACITY,
            )));

            let server_end = Arc::new(Mutex::new(UnixSocket {
                socket_type: SOCK_STREAM,
                bound_path: None,
                is_listening: false,
                backlog: 0,
                pending_conns: VecDeque::new(),
                rx_buffer: client_to_server.clone(),
                tx_buffer: server_to_client.clone(),
                peer: Some(Arc::downgrade(self_arc)),
                dgram_queue: VecDeque::new(),
                nonblocking: false,
                shutdown_read: false,
                shutdown_write: false,
            }));

            self.rx_buffer = server_to_client;
            self.tx_buffer = client_to_server;
            self.peer = Some(Arc::downgrade(&server_end));

            listener.pending_conns.push_back(server_end);
            Ok(())
        } else {
            // DGRAM connect just sets peer
            self.peer = Some(Arc::downgrade(&listener_arc));
            Ok(())
        }
    }

    /// Accept an incoming client connection on a listening stream socket.
    pub fn accept(
        &mut self,
        nonblocking: bool,
    ) -> Result<Arc<Mutex<Socket>>, SyscallError> {
        if !self.is_listening {
            return Err(SyscallError::EINVAL);
        }

        loop {
            if let Some(conn) = self.pending_conns.pop_front() {
                return Ok(Arc::new(Mutex::new(Socket::Unix(conn))));
            }

            if self.nonblocking || nonblocking {
                return Err(SyscallError::EAGAIN);
            }

            crate::proc::thread::Thread::yield_cpu();
        }
    }

    /// Send bytes over the stream channel or datagram queue.
    pub fn send(&mut self, buf: &[u8], flags: i32) -> Result<usize, SyscallError> {
        if self.shutdown_write {
            return Err(SyscallError::EPIPE);
        }

        let nonblock = self.nonblocking || ((flags & MSG_DONTWAIT) != 0);

        if self.socket_type == SOCK_STREAM {
            if self.peer.is_none() {
                return Err(SyscallError::ENOTCONN);
            }

            let peer_alive = self.peer.as_ref().map(|w| w.upgrade().is_some()).unwrap_or(false);
            if !peer_alive {
                return Err(SyscallError::EPIPE);
            }

            loop {
                let mut tx = self.tx_buffer.lock();
                let available = tx.capacity.saturating_sub(tx.data.len());
                if available > 0 {
                    let to_write = core::cmp::min(buf.len(), available);
                    for &b in &buf[..to_write] {
                        tx.data.push_back(b);
                    }
                    return Ok(to_write);
                }

                if nonblock {
                    return Err(SyscallError::EAGAIN);
                }

                drop(tx);
                crate::proc::thread::Thread::yield_cpu();
            }
        } else {
            // Datagram send
            let target_arc = self
                .peer
                .as_ref()
                .and_then(|w| w.upgrade())
                .ok_or(SyscallError::EDESTADDRREQ)?;

            let mut target = target_arc.lock();
            let mut payload = Vec::with_capacity(buf.len());
            payload.extend_from_slice(buf);

            target.dgram_queue.push_back(UnixDatagram {
                sender_path: self.bound_path.clone(),
                payload,
            });

            Ok(buf.len())
        }
    }

    /// Receive bytes from the stream buffer or datagram queue.
    pub fn recv(&mut self, buf: &mut [u8], flags: i32) -> Result<usize, SyscallError> {
        if self.shutdown_read {
            return Ok(0); // EOF
        }

        let nonblock = self.nonblocking || ((flags & MSG_DONTWAIT) != 0);
        let peek = (flags & MSG_PEEK) != 0;

        if self.socket_type == SOCK_STREAM {
            loop {
                let mut rx = self.rx_buffer.lock();
                if !rx.data.is_empty() {
                    let to_read = core::cmp::min(buf.len(), rx.data.len());
                    if peek {
                        for (i, &b) in rx.data.iter().take(to_read).enumerate() {
                            buf[i] = b;
                        }
                    } else {
                        for i in 0..to_read {
                            buf[i] = rx.data.pop_front().unwrap_or(0);
                        }
                    }
                    return Ok(to_read);
                }

                // If buffer empty and peer closed or disconnected -> EOF (0)
                let peer_closed = self.peer.as_ref().map(|w| w.upgrade().is_none()).unwrap_or(true);
                if rx.closed || peer_closed {
                    return Ok(0);
                }

                if nonblock {
                    return Err(SyscallError::EAGAIN);
                }

                drop(rx);
                crate::proc::thread::Thread::yield_cpu();
            }
        } else {
            // Datagram recv
            loop {
                if let Some(dgram) = self.dgram_queue.pop_front() {
                    let len = core::cmp::min(buf.len(), dgram.payload.len());
                    buf[..len].copy_from_slice(&dgram.payload[..len]);
                    return Ok(len);
                }

                if nonblock {
                    return Err(SyscallError::EAGAIN);
                }

                crate::proc::thread::Thread::yield_cpu();
            }
        }
    }

    /// Shutdown read/write channels.
    pub fn shutdown(&mut self, how: i32) -> Result<(), SyscallError> {
        match how {
            SHUT_RD => self.shutdown_read = true,
            SHUT_WR => {
                self.shutdown_write = true;
                self.tx_buffer.lock().closed = true;
            }
            SHUT_RDWR => {
                self.shutdown_read = true;
                self.shutdown_write = true;
                self.tx_buffer.lock().closed = true;
            }
            _ => return Err(SyscallError::EINVAL),
        }
        Ok(())
    }

    pub fn poll_read_ready(&self) -> bool {
        if self.is_listening && !self.pending_conns.is_empty() {
            return true;
        }
        if self.socket_type == SOCK_STREAM {
            let rx = self.rx_buffer.lock();
            !rx.data.is_empty() || rx.closed
        } else {
            !self.dgram_queue.is_empty()
        }
    }

    pub fn poll_write_ready(&self) -> bool {
        if self.socket_type == SOCK_STREAM {
            let tx = self.tx_buffer.lock();
            tx.data.len() < tx.capacity
        } else {
            true
        }
    }
}

impl Drop for UnixSocket {
    fn drop(&mut self) {
        if let Some(ref path) = self.bound_path {
            let mut reg = UNIX_REGISTRY.lock();
            reg.remove(path);
        }
        self.tx_buffer.lock().closed = true;
    }
}
