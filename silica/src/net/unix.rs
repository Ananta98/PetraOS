// SPDX-License-Identifier: GPL-2.0

//! UNIX Domain Socket Subsystem.
//!
//! Provides support for filesystem and abstract namespace UNIX domain sockets,
//! connection routing, listening accept queues, and datagram queuing.

use alloc::{
    collections::{BTreeMap, VecDeque},
    string::String,
    sync::Arc,
    vec::Vec,
};
use ostd::sync::WaitQueue;
use spin::Mutex;

use crate::{
    api::errno::{Errno, Result},
    net::socket::KernelSocket,
};

/// Global registry of bound UNIX domain sockets keyed by filesystem path or abstract name.
pub static UNIX_REGISTRY: Mutex<BTreeMap<String, Arc<KernelSocket>>> =
    Mutex::new(BTreeMap::new());

/// State for a listening UNIX stream socket (`SOCK_STREAM`).
pub struct UnixListenerState {
    pub backlog: Mutex<usize>,
    pub accept_queue: Mutex<VecDeque<(Arc<KernelSocket>, Option<String>)>>,
    pub wq: WaitQueue,
}

impl UnixListenerState {
    pub fn new(backlog: usize) -> Self {
        Self {
            backlog: Mutex::new(backlog),
            accept_queue: Mutex::new(VecDeque::new()),
            wq: WaitQueue::new(),
        }
    }
}

impl core::fmt::Debug for UnixListenerState {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("UnixListenerState")
            .field("backlog", &*self.backlog.lock())
            .finish()
    }
}

/// State for a UNIX datagram socket (`SOCK_DGRAM`).
pub struct UnixDgramState {
    pub rx_queue: Mutex<VecDeque<(Vec<u8>, Option<String>)>>,
    pub wq: WaitQueue,
}

impl UnixDgramState {
    pub fn new() -> Self {
        Self {
            rx_queue: Mutex::new(VecDeque::new()),
            wq: WaitQueue::new(),
        }
    }
}

impl core::fmt::Debug for UnixDgramState {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("UnixDgramState").finish()
    }
}

/// Binds a UNIX domain socket to a given path in the global registry.
pub fn bind_unix_socket(path: &str, socket: Arc<KernelSocket>) -> Result<()> {
    let mut reg = UNIX_REGISTRY.lock();
    if reg.contains_key(path) {
        return Err(Errno::EADDRINUSE);
    }
    reg.insert(String::from(path), socket);
    Ok(())
}

/// Unbinds a UNIX domain socket from the global registry.
pub fn unbind_unix_socket(path: &str) {
    let mut reg = UNIX_REGISTRY.lock();
    reg.remove(path);
}

/// Looks up an active UNIX domain socket by path.
pub fn lookup_unix_socket(path: &str) -> Option<Arc<KernelSocket>> {
    let reg = UNIX_REGISTRY.lock();
    reg.get(path).cloned()
}

#[cfg(ktest)]
mod tests {
    use super::*;
    use ostd::prelude::ktest;
    use crate::net::socket::{SOCK_STREAM, SOCK_DGRAM, SocketAddress};

    #[ktest]
    fn unix_stream_socket_listen_connect_and_exchange() {
        let sock_path = "/tmp/test_stream_ktest.sock";

        // Create server listener
        let server = KernelSocket::new_unix(SOCK_STREAM).expect("server socket");
        server
            .bind(SocketAddress::Unix(sock_path.into()))
            .expect("server bind");
        server.listen(5).expect("server listen");

        // Create client and connect
        let client = KernelSocket::new_unix(SOCK_STREAM).expect("client socket");
        client
            .connect(SocketAddress::Unix(sock_path.into()))
            .expect("client connect");

        // Server accepts connection
        let (conn, _peer_addr) = server.accept().expect("server accept");

        // Client sends to server
        let msg1 = b"ping_from_client";
        assert_eq!(client.send(msg1, 0).expect("client send"), msg1.len());

        let mut buf = [0u8; 32];
        let n1 = conn.recv(&mut buf, 0).expect("conn recv");
        assert_eq!(&buf[..n1], msg1);

        // Server sends response to client
        let msg2 = b"pong_from_server";
        assert_eq!(conn.send(msg2, 0).expect("conn send"), msg2.len());

        let n2 = client.recv(&mut buf, 0).expect("client recv");
        assert_eq!(&buf[..n2], msg2);

        // Close and verify registry cleanup
        server.close();
        assert!(lookup_unix_socket(sock_path).is_none());
    }

    #[ktest]
    fn unix_dgram_socket_sendto_and_recvfrom() {
        let srv_path = "/tmp/test_srv_dgram.sock";
        let cli_path = "/tmp/test_cli_dgram.sock";

        let server = KernelSocket::new_unix(SOCK_DGRAM).expect("srv sock");
        server
            .bind(SocketAddress::Unix(srv_path.into()))
            .expect("srv bind");

        let client = KernelSocket::new_unix(SOCK_DGRAM).expect("cli sock");
        client
            .bind(SocketAddress::Unix(cli_path.into()))
            .expect("cli bind");

        let payload = b"alpine_dgram_test";
        let sent = client
            .sendto(payload, Some(SocketAddress::Unix(srv_path.into())), 0)
            .expect("cli sendto");
        assert_eq!(sent, payload.len());

        let mut buf = [0u8; 32];
        let (n, sender) = server.recvfrom(&mut buf, 0).expect("srv recvfrom");
        assert_eq!(&buf[..n], payload);
        assert_eq!(sender, Some(SocketAddress::Unix(cli_path.into())));

        server.close();
        client.close();
        assert!(lookup_unix_socket(srv_path).is_none());
        assert!(lookup_unix_socket(cli_path).is_none());
    }
}

