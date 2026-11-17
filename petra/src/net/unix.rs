// SPDX-License-Identifier: GPL-2.0

//! UNIX Domain Socket Subsystem.
//!
//! Provides support for filesystem and abstract namespace UNIX domain sockets,
//! connection routing, listening accept queues, and datagram queuing.

use alloc::{
    collections::VecDeque,
    string::String,
    sync::Arc,
    vec::Vec,
};
use ostd::sync::WaitQueue;
use spin::Mutex;

use crate::{
    net::socket::{KernelSocket, SocketKind},
    uapi::errno::{Errno, Result},
    utils::hashmap::HashMap,
};

/// Global registry of bound UNIX domain sockets keyed by filesystem path or abstract name.
pub static UNIX_REGISTRY: Mutex<HashMap<String, Arc<KernelSocket>>> = Mutex::new(HashMap::new());

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

/// A queued UNIX datagram packet with optional sender path and attached file descriptors.
pub struct UnixDgramPacket {
    pub data: Vec<u8>,
    pub sender: Option<String>,
    pub cred: Option<crate::uapi::cred::PeerCred>,
    pub files: Vec<Arc<crate::fs::vfs::File>>,
}

/// State for a UNIX datagram socket (`SOCK_DGRAM`).
pub struct UnixDgramState {
    pub rx_queue: Mutex<VecDeque<UnixDgramPacket>>,
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

/// Unbinds a UNIX domain socket from the global registry if the registered entry
/// shares the same underlying `kind` state, preventing accepted connection sockets
/// from removing their listener's registry entry on close.
pub fn unbind_unix_socket(path: &str, kind: &Arc<Mutex<SocketKind>>) {
    let removed = {
        let mut reg = UNIX_REGISTRY.lock();
        if reg.get(path).is_some_and(|sock| Arc::ptr_eq(&sock.kind, kind)) {
            reg.remove(path)
        } else {
            None
        }
    };
    drop(removed);
}

/// Looks up an active UNIX domain socket by path.
pub fn lookup_unix_socket(path: &str) -> Option<Arc<KernelSocket>> {
    let reg = UNIX_REGISTRY.lock();
    reg.get(path).cloned()
}

#[cfg(ktest)]
mod tests {
    use super::*;
    use crate::net::socket::{SOCK_DGRAM, SOCK_SEQPACKET, SOCK_STREAM, SocketAddress};
    use ostd::prelude::ktest;

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

    #[ktest]
    fn unix_socket_ancillary_fd_passing() {
        let (s1, s2) = KernelSocket::new_unix_pair(SOCK_STREAM).expect("pair");
        assert!(s1.peer_cred.lock().is_some());
        assert!(s2.peer_cred.lock().is_some());

        let (pipe_r, _pipe_w) = crate::fs::pipe::create_pipe(false);
        let sent = s1
            .send_with_ancillary(b"with_fd", alloc::vec![pipe_r.clone()], 0)
            .expect("send_with_ancillary");
        assert_eq!(sent, 7);

        let mut buf = [0u8; 16];
        let (n, files, _) = s2.recv_with_ancillary(&mut buf, 0).expect("recv_with_ancillary");
        assert_eq!(&buf[..n], b"with_fd");
        assert_eq!(files.len(), 1);

        s1.close();
        s2.close();
    }

    #[ktest]
    fn unix_seqpacket_socket_listen_connect_and_pair() {
        let sock_path = "/tmp/test_seqpacket_ktest.sock";

        let server = KernelSocket::new_unix(SOCK_SEQPACKET).expect("seqpacket server");
        assert_eq!(server.sock_type, SOCK_SEQPACKET);
        server
            .bind(SocketAddress::Unix(sock_path.into()))
            .expect("seqpacket bind");
        server.listen(5).expect("seqpacket listen");

        let client = KernelSocket::new_unix(SOCK_SEQPACKET).expect("seqpacket client");
        assert_eq!(client.sock_type, SOCK_SEQPACKET);
        client
            .connect(SocketAddress::Unix(sock_path.into()))
            .expect("seqpacket connect");

        let (conn, _peer_addr) = server.accept().expect("seqpacket accept");
        assert_eq!(conn.sock_type, SOCK_SEQPACKET);

        let msg = b"seqpacket_packet_payload";
        assert_eq!(client.send(msg, 0).expect("client send"), msg.len());

        let mut buf = [0u8; 32];
        let n = conn.recv(&mut buf, 0).expect("conn recv");
        assert_eq!(&buf[..n], msg);

        server.close();
        client.close();
        conn.close();
        assert!(lookup_unix_socket(sock_path).is_none());

        // Test socketpair with SOCK_SEQPACKET
        let (p1, p2) = KernelSocket::new_unix_pair(SOCK_SEQPACKET).expect("seqpacket pair");
        assert_eq!(p1.sock_type, SOCK_SEQPACKET);
        assert_eq!(p2.sock_type, SOCK_SEQPACKET);
        assert_eq!(p1.send(b"pair_seq", 0).expect("pair send"), 8);
        let n_pair = p2.recv(&mut buf, 0).expect("pair recv");
        assert_eq!(&buf[..n_pair], b"pair_seq");
        p1.close();
        p2.close();
    }

    #[ktest]
    fn unix_listener_survives_accepted_connection_close() {
        let sock_path = "/tmp/test_listener_reopen_ktest.sock";

        let server = KernelSocket::new_unix(SOCK_STREAM).expect("server socket");
        server
            .bind(SocketAddress::Unix(sock_path.into()))
            .expect("server bind");
        server.listen(8).expect("server listen");

        let client1 = KernelSocket::new_unix(SOCK_STREAM).expect("client1");
        client1
            .connect(SocketAddress::Unix(sock_path.into()))
            .expect("client1 connect");
        let (conn1, _) = server.accept().expect("accept1");

        // Closing an accepted connection must not unbind the listener path.
        conn1.close();
        client1.close();
        assert!(lookup_unix_socket(sock_path).is_some());

        let client2 = KernelSocket::new_unix(SOCK_STREAM).expect("client2");
        client2
            .connect(SocketAddress::Unix(sock_path.into()))
            .expect("client2 connect");
        let (conn2, _) = server.accept().expect("accept2");

        assert_eq!(client2.send(b"reopened", 0).expect("client2 send"), 8);
        let mut buf = [0u8; 16];
        let n = conn2.recv(&mut buf, 0).expect("conn2 recv");
        assert_eq!(&buf[..n], b"reopened");

        conn2.close();
        client2.close();
        server.close();
        assert!(lookup_unix_socket(sock_path).is_none());
    }
}

