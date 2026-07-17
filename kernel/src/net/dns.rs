//! Domain Name System (DNS) client socket wrapper.
//! Enforces safety guidelines and denies unsafe code.

use alloc::vec::Vec;
use smoltcp::socket::dns::{Socket as SmoltcpDnsSocket, DnsQuery as SmoltcpDnsQuery, QueryHandle as SmoltcpQueryHandle, StartQueryError as SmoltcpStartQueryError, GetQueryResultError as SmoltcpGetQueryResultError};
use smoltcp::wire::DnsQueryType as SmoltcpDnsQueryType;

/// An IP address for DNS queries (supporting IPv4 and IPv6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IpAddress {
    /// IPv4 address.
    Ipv4(crate::net::ipv4::Ipv4Address),
    /// IPv6 address.
    Ipv6(crate::net::ipv6::Ipv6Address),
}

impl From<smoltcp::wire::IpAddress> for IpAddress {
    fn from(addr: smoltcp::wire::IpAddress) -> Self {
        match addr {
            smoltcp::wire::IpAddress::Ipv4(v4) => IpAddress::Ipv4(v4.into()),
            smoltcp::wire::IpAddress::Ipv6(v6) => IpAddress::Ipv6(v6.into()),
        }
    }
}

impl From<IpAddress> for smoltcp::wire::IpAddress {
    fn from(addr: IpAddress) -> Self {
        match addr {
            IpAddress::Ipv4(v4) => smoltcp::wire::IpAddress::Ipv4(v4.into()),
            IpAddress::Ipv6(v6) => smoltcp::wire::IpAddress::Ipv6(v6.into()),
        }
    }
}

/// DNS query type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DnsQueryType {
    /// A records (IPv4 addresses).
    A,
    /// AAAA records (IPv6 addresses).
    Aaaa,
    /// Unknown / unsupported record type.
    Unknown(u16),
}

impl From<SmoltcpDnsQueryType> for DnsQueryType {
    fn from(t: SmoltcpDnsQueryType) -> Self {
        match t {
            SmoltcpDnsQueryType::A => DnsQueryType::A,
            SmoltcpDnsQueryType::Aaaa => DnsQueryType::Aaaa,
            SmoltcpDnsQueryType::Unknown(val) => DnsQueryType::Unknown(val),
            _ => DnsQueryType::Unknown(t.into()),
        }
    }
}

impl From<DnsQueryType> for SmoltcpDnsQueryType {
    fn from(t: DnsQueryType) -> Self {
        match t {
            DnsQueryType::A => SmoltcpDnsQueryType::A,
            DnsQueryType::Aaaa => SmoltcpDnsQueryType::Aaaa,
            DnsQueryType::Unknown(val) => SmoltcpDnsQueryType::Unknown(val),
        }
    }
}

/// Error returned by starting a DNS query.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum StartQueryError {
    /// No free query slots.
    NoFreeSlot,
    /// The domain name is invalid.
    InvalidName,
    /// The domain name is too long.
    NameTooLong,
}

impl From<SmoltcpStartQueryError> for StartQueryError {
    fn from(err: SmoltcpStartQueryError) -> Self {
        match err {
            SmoltcpStartQueryError::NoFreeSlot => StartQueryError::NoFreeSlot,
            SmoltcpStartQueryError::InvalidName => StartQueryError::InvalidName,
            SmoltcpStartQueryError::NameTooLong => StartQueryError::NameTooLong,
        }
    }
}

/// Error returned by retrieving a DNS query result.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum GetQueryResultError {
    /// Query is still in progress.
    Pending,
    /// Query failed (e.g. server error or name not found).
    Failed,
}

impl From<SmoltcpGetQueryResultError> for GetQueryResultError {
    fn from(err: SmoltcpGetQueryResultError) -> Self {
        match err {
            SmoltcpGetQueryResultError::Pending => GetQueryResultError::Pending,
            SmoltcpGetQueryResultError::Failed => GetQueryResultError::Failed,
        }
    }
}

/// A handle to an in-progress DNS query.
#[derive(Clone, Copy)]
pub struct QueryHandle {
    pub(crate) inner: SmoltcpQueryHandle,
}

impl core::fmt::Debug for QueryHandle {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "QueryHandle")
    }
}

/// A Domain Name System (DNS) socket.
#[derive(Debug)]
pub struct DnsSocket<'a> {
    inner: SmoltcpDnsSocket<'a>,
}

impl<'a> DnsSocket<'a> {
    /// Create a new DNS socket.
    pub fn new(servers: &[IpAddress], queries: &'a mut [Option<SmoltcpDnsQuery>]) -> Self {
        let smoltcp_servers: Vec<smoltcp::wire::IpAddress> = servers.iter().map(|s| (*s).into()).collect();
        Self {
            inner: SmoltcpDnsSocket::new(&smoltcp_servers, queries),
        }
    }

    /// Update the list of DNS servers.
    pub fn update_servers(&mut self, servers: &[IpAddress]) {
        let smoltcp_servers: Vec<smoltcp::wire::IpAddress> = servers.iter().map(|s| (*s).into()).collect();
        self.inner.update_servers(&smoltcp_servers);
    }

    /// Start a DNS query.
    pub fn start_query(
        &mut self,
        cx: &mut smoltcp::iface::Context,
        name: &str,
        query_type: DnsQueryType,
    ) -> Result<QueryHandle, StartQueryError> {
        let handle = self.inner.start_query(cx, name, query_type.into())?;
        Ok(QueryHandle { inner: handle })
    }

    /// Retrieve the query result.
    pub fn get_query_result(
        &mut self,
        handle: QueryHandle,
    ) -> Result<Vec<IpAddress>, GetQueryResultError> {
        let results = self.inner.get_query_result(handle.inner)?;
        Ok(results.iter().map(|addr| (*addr).into()).collect())
    }

    /// Cancel an in-progress query.
    pub fn cancel_query(&mut self, handle: QueryHandle) {
        self.inner.cancel_query(handle.inner);
    }

    /// Get the hop limit/TTL.
    pub fn hop_limit(&self) -> Option<u8> {
        self.inner.hop_limit()
    }

    /// Set the hop limit/TTL.
    pub fn set_hop_limit(&mut self, hop_limit: Option<u8>) {
        self.inner.set_hop_limit(hop_limit);
    }

    /// Return a reference to the underlying socket.
    pub fn inner(&self) -> &SmoltcpDnsSocket<'a> {
        &self.inner
    }

    /// Return a mutable reference to the underlying socket.
    pub fn inner_mut(&mut self) -> &mut SmoltcpDnsSocket<'a> {
        &mut self.inner
    }
}

#[cfg(ktest)]
mod tests {
    use super::*;
    use ostd::prelude::ktest;

    #[ktest]
    fn test_dns_socket_creation() {
        let servers = [IpAddress::Ipv4(crate::net::ipv4::Ipv4Address::new(8, 8, 8, 8))];
        let mut queries = [None, None];
        let socket = DnsSocket::new(&servers, &mut queries);
        assert_eq!(socket.hop_limit(), None);
    }
}
