//! Socket filesystem abstraction.
//!
//! Provides [`SocketFile`], a [`FileOps`] implementation that bridges
//! the VFS layer with the smoltcp network stack. Each open socket fd
//! is backed by one `SocketFile` instance; when the instance is dropped
//! the underlying smoltcp socket handle is removed from the global
//! [`crate::net::NET_STACK`].

use crate::fs::vfs::{FileOps, SeekFrom};
use ostd::Error;
use ostd::sync::SpinLock;
use smoltcp::iface::SocketHandle;
use smoltcp::wire::IpEndpoint;

/// A file-descriptor wrapper around a smoltcp socket handle.
///
/// Stores the socket `handle` inside the global [`crate::net::NET_STACK`]
/// together with addressing metadata required for connectionless (UDP)
/// sends.
pub struct SocketFile {
    /// Handle into the smoltcp [`SocketSet`].
    pub handle: SpinLock<SocketHandle>,
    /// Address family (e.g. `AF_INET = 2`, `AF_INET6 = 10`).
    pub domain: i32,
    /// Socket type (e.g. `SOCK_STREAM = 1`, `SOCK_DGRAM = 2`).
    pub socket_type: i32,
    /// Protocol number (usually `0` for auto-selection).
    pub protocol: i32,
    /// Locally bound endpoint, set by `bind` or auto-assigned on `connect`.
    pub local: SpinLock<Option<IpEndpoint>>,
    /// Remote peer endpoint, set by `connect` or `sendto`.
    pub remote: SpinLock<Option<IpEndpoint>>,
}

impl FileOps for SocketFile {
    fn read(&mut self, buf: &mut [u8], _offset: &mut usize) -> Result<usize, Error> {
        let mut stack_guard = crate::net::NET_STACK.lock();
        let stack = stack_guard.as_mut().ok_or(Error::InvalidArgs)?;
        let sockets = &mut stack.sockets;

        let handle = *self.handle.lock();
        if self.socket_type == 1 {
            let tcp_socket = sockets.get_mut::<smoltcp::socket::tcp::Socket>(handle);
            if !tcp_socket.is_active() && !tcp_socket.may_recv() {
                return Err(Error::IoError);
            }
            tcp_socket.recv_slice(buf).map_err(|_| Error::IoError)
        } else if self.socket_type == 2 {
            let udp_socket = sockets.get_mut::<smoltcp::socket::udp::Socket>(handle);
            udp_socket
                .recv_slice(buf)
                .map(|(len, _)| len)
                .map_err(|_| Error::IoError)
        } else {
            Err(Error::InvalidArgs)
        }
    }

    fn write(&mut self, buf: &[u8], _offset: &mut usize) -> Result<usize, Error> {
        let mut stack_guard = crate::net::NET_STACK.lock();
        let stack = stack_guard.as_mut().ok_or(Error::InvalidArgs)?;
        let sockets = &mut stack.sockets;

        let handle = *self.handle.lock();
        if self.socket_type == 1 {
            let tcp_socket = sockets.get_mut::<smoltcp::socket::tcp::Socket>(handle);
            if !tcp_socket.is_active() && !tcp_socket.may_send() {
                return Err(Error::IoError);
            }
            tcp_socket.send_slice(buf).map_err(|_| Error::IoError)
        } else if self.socket_type == 2 {
            let udp_socket = sockets.get_mut::<smoltcp::socket::udp::Socket>(handle);
            let remote = self.remote.lock();
            if let Some(dest) = *remote {
                udp_socket
                    .send_slice(buf, dest)
                    .map(|_| buf.len())
                    .map_err(|_| Error::IoError)
            } else {
                Err(Error::InvalidArgs)
            }
        } else {
            Err(Error::InvalidArgs)
        }
    }

    fn seek(&mut self, _pos: SeekFrom, _offset: &mut usize) -> Result<usize, Error> {
        // Sockets are not seekable.
        Err(Error::InvalidArgs)
    }

    fn readdir(&mut self) -> Result<alloc::vec::Vec<crate::fs::vfs::DirEntry>, Error> {
        // Sockets do not have directory entries.
        Err(Error::InvalidArgs)
    }

    fn as_any(&self) -> Option<&dyn core::any::Any> {
        Some(self)
    }
}

impl Drop for SocketFile {
    /// Removes the smoltcp socket handle from the global [`crate::net::NET_STACK`]
    /// when the last file descriptor referencing this socket is closed.
    fn drop(&mut self) {
        let mut stack_guard = crate::net::NET_STACK.lock();
        if let Some(stack) = stack_guard.as_mut() {
            stack.sockets.remove(*self.handle.lock());
        }
    }
}
