//! DHCPv4 client socket wrapper.
//! Enforces safety guidelines and denies unsafe code.

use crate::net::ipv4::{Ipv4Address, Ipv4Cidr};
use smoltcp::socket::dhcpv4::{
    Config as SmoltcpDhcpv4Config, Event as SmoltcpDhcpv4Event, Socket as SmoltcpDhcpv4Socket,
};
use smoltcp::time::Duration;

/// IPv4 configuration data provided by the DHCP server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DhcpConfig {
    /// Assigned IP address and subnet mask.
    pub address: Ipv4Cidr,
    /// Default gateway / router IP address.
    pub router: Option<Ipv4Address>,
    /// List of DNS server IP addresses.
    pub dns_servers: alloc::vec::Vec<Ipv4Address>,
}

impl<'a> From<SmoltcpDhcpv4Config<'a>> for DhcpConfig {
    fn from(config: SmoltcpDhcpv4Config<'a>) -> Self {
        let mut dns = alloc::vec![];
        for dns_server in &config.dns_servers {
            dns.push((*dns_server).into());
        }
        Self {
            address: config.address.into(),
            router: config.router.map(Into::into),
            dns_servers: dns,
        }
    }
}

/// DHCP event resulting from client polling.
#[derive(Debug, PartialEq, Eq)]
pub enum DhcpEvent {
    /// Configuration has been lost (for example, the lease has expired).
    Deconfigured,
    /// Configuration has been newly acquired or modified.
    Configured(DhcpConfig),
}

impl<'a> From<SmoltcpDhcpv4Event<'a>> for DhcpEvent {
    fn from(event: SmoltcpDhcpv4Event<'a>) -> Self {
        match event {
            SmoltcpDhcpv4Event::Deconfigured => DhcpEvent::Deconfigured,
            SmoltcpDhcpv4Event::Configured(config) => DhcpEvent::Configured(config.into()),
        }
    }
}

/// A DHCPv4 client socket that autonomously acquires network configuration.
#[derive(Debug)]
pub struct DhcpSocket<'a> {
    inner: SmoltcpDhcpv4Socket<'a>,
}

impl<'a> DhcpSocket<'a> {
    /// Create a new DHCPv4 socket.
    pub fn new() -> Self {
        Self {
            inner: SmoltcpDhcpv4Socket::new(),
        }
    }

    /// Query the socket for configuration changes.
    pub fn poll(&mut self) -> Option<DhcpEvent> {
        self.inner.poll().map(Into::into)
    }

    /// Set the buffer into which incoming DHCP packets are copied.
    pub fn set_receive_packet_buffer(&mut self, buffer: &'a mut [u8]) {
        self.inner.set_receive_packet_buffer(buffer);
    }

    /// Set the max lease duration (capping the server-provided lease duration).
    pub fn set_max_lease_duration(&mut self, max_lease_duration: Option<core::time::Duration>) {
        let smoltcp_duration =
            max_lease_duration.map(|d| Duration::from_micros(d.as_micros() as u64));
        self.inner.set_max_lease_duration(smoltcp_duration);
    }

    /// Get whether to ignore NAKs.
    pub fn ignore_naks(&self) -> bool {
        self.inner.ignore_naks()
    }

    /// Set whether to ignore NAKs.
    pub fn set_ignore_naks(&mut self, ignore_naks: bool) {
        self.inner.set_ignore_naks(ignore_naks);
    }

    /// Return a reference to the underlying socket.
    pub fn inner(&self) -> &SmoltcpDhcpv4Socket<'a> {
        &self.inner
    }

    /// Return a mutable reference to the underlying socket.
    pub fn inner_mut(&mut self) -> &mut SmoltcpDhcpv4Socket<'a> {
        &mut self.inner
    }
}

impl<'a> Default for DhcpSocket<'a> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(ktest)]
mod tests {
    use super::*;
    use ostd::prelude::ktest;

    #[ktest]
    fn test_dhcp_socket_creation() {
        let mut socket = DhcpSocket::new();
        assert!(!socket.ignore_naks());
        socket.set_ignore_naks(true);
        assert!(socket.ignore_naks());
    }
}
