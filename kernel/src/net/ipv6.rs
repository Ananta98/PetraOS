//! IPv6 address, CIDR, and packet parsing/serialization wrapper.
//! Enforces memory safety and denies unsafe code.

use crate::net::ipv4::{IpError, IpProtocol};
use core::fmt;
use smoltcp::wire::Ipv6Packet as SmoltcpIpv6Packet;
use smoltcp::wire::{Ipv6Address as SmoltcpIpv6Address, Ipv6Cidr as SmoltcpIpv6Cidr};

/// A sixteen-octet IPv6 address.
#[derive(Debug, Hash, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Default)]
pub struct Ipv6Address(pub [u8; 16]);

impl Ipv6Address {
    /// An unspecified address.
    pub const UNSPECIFIED: Self = Self([0x00; 16]);

    /// The loopback address.
    pub const LOOPBACK: Self = Self([
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x01,
    ]);

    /// The link-local all nodes multicast address.
    pub const LINK_LOCAL_ALL_NODES: Self = Self([
        0xff, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x01,
    ]);

    /// The link-local all routers multicast address.
    pub const LINK_LOCAL_ALL_ROUTERS: Self = Self([
        0xff, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x02,
    ]);

    /// Construct an IPv6 address from parts.
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        a0: u16,
        a1: u16,
        a2: u16,
        a3: u16,
        a4: u16,
        a5: u16,
        a6: u16,
        a7: u16,
    ) -> Self {
        Self([
            (a0 >> 8) as u8,
            a0 as u8,
            (a1 >> 8) as u8,
            a1 as u8,
            (a2 >> 8) as u8,
            a2 as u8,
            (a3 >> 8) as u8,
            a3 as u8,
            (a4 >> 8) as u8,
            a4 as u8,
            (a5 >> 8) as u8,
            a5 as u8,
            (a6 >> 8) as u8,
            a6 as u8,
            (a7 >> 8) as u8,
            a7 as u8,
        ])
    }

    /// Construct an IPv6 address from a sequence of octets, in big-endian.
    pub fn from_bytes(data: &[u8]) -> Self {
        let mut bytes = [0; 16];
        bytes.copy_from_slice(data);
        Self(bytes)
    }

    /// Return an IPv6 address as a sequence of octets, in big-endian.
    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }

    /// Query whether the address is the loopback address.
    pub fn is_loopback(&self) -> bool {
        *self == Self::LOOPBACK
    }

    /// Query whether the address is unspecified.
    pub fn is_unspecified(&self) -> bool {
        *self == Self::UNSPECIFIED
    }

    /// Query whether the address is a multicast address.
    pub const fn is_multicast(&self) -> bool {
        self.0[0] == 0xff
    }

    /// Query whether the address is an unicast address.
    pub fn is_unicast(&self) -> bool {
        !(self.is_multicast() || self.is_unspecified())
    }
}

impl fmt::Display for Ipv6Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", SmoltcpIpv6Address::from(*self))
    }
}

impl From<[u8; 16]> for Ipv6Address {
    fn from(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }
}

impl From<Ipv6Address> for [u8; 16] {
    fn from(addr: Ipv6Address) -> Self {
        addr.0
    }
}

impl From<SmoltcpIpv6Address> for Ipv6Address {
    fn from(addr: SmoltcpIpv6Address) -> Self {
        Self(addr.0)
    }
}

impl From<Ipv6Address> for SmoltcpIpv6Address {
    fn from(addr: Ipv6Address) -> Self {
        SmoltcpIpv6Address(addr.0)
    }
}

/// A specification of an IPv6 CIDR block.
#[derive(Debug, Hash, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Default)]
pub struct Ipv6Cidr {
    address: Ipv6Address,
    prefix_len: u8,
}

impl Ipv6Cidr {
    /// Create a new IPv6 CIDR block.
    pub const fn new(address: Ipv6Address, prefix_len: u8) -> Self {
        Self {
            address,
            prefix_len,
        }
    }

    /// Return the address of this CIDR block.
    pub const fn address(&self) -> Ipv6Address {
        self.address
    }

    /// Return the prefix length of this CIDR block.
    pub const fn prefix_len(&self) -> u8 {
        self.prefix_len
    }
}

impl fmt::Display for Ipv6Cidr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.address, self.prefix_len)
    }
}

impl From<SmoltcpIpv6Cidr> for Ipv6Cidr {
    fn from(cidr: SmoltcpIpv6Cidr) -> Self {
        Self {
            address: cidr.address().into(),
            prefix_len: cidr.prefix_len(),
        }
    }
}

impl From<Ipv6Cidr> for SmoltcpIpv6Cidr {
    fn from(cidr: Ipv6Cidr) -> Self {
        Self::new(cidr.address.into(), cidr.prefix_len)
    }
}

/// Parser and builder for Internet Protocol version 6 packets.
#[derive(Debug)]
pub struct Ipv6Packet<T: AsRef<[u8]>> {
    inner: SmoltcpIpv6Packet<T>,
}

impl<T: AsRef<[u8]>> Ipv6Packet<T> {
    /// Parse an IPv6 packet, verifying the buffer length.
    pub fn new_checked(buffer: T) -> Result<Self, IpError> {
        let packet = SmoltcpIpv6Packet::new_checked(buffer).map_err(|_| IpError::PacketTooShort)?;
        Ok(Self { inner: packet })
    }

    /// Construct an IPv6 packet wrapper without verifying the length.
    pub fn new_unchecked(buffer: T) -> Self {
        Self {
            inner: SmoltcpIpv6Packet::new_unchecked(buffer),
        }
    }

    /// Return the source IPv6 address.
    pub fn src_addr(&self) -> Ipv6Address {
        self.inner.src_addr().into()
    }

    /// Return the destination IPv6 address.
    pub fn dst_addr(&self) -> Ipv6Address {
        self.inner.dst_addr().into()
    }

    /// Return the encapsulated protocol.
    pub fn next_header(&self) -> IpProtocol {
        self.inner.next_header().into()
    }

    /// Return the hop limit field.
    pub fn hop_limit(&self) -> u8 {
        self.inner.hop_limit()
    }

    /// Return the payload length.
    pub fn payload_len(&self) -> u16 {
        self.inner.payload_len()
    }

    /// Return the payload of the packet.
    pub fn payload(&self) -> &[u8] {
        let header_len = 40; // IPv6 header is always 40 bytes
        let total_len = header_len + self.inner.payload_len() as usize;
        &self.inner.as_ref()[header_len..total_len]
    }

    /// Return a reference to the underlying packet buffer.
    pub fn inner(&self) -> &SmoltcpIpv6Packet<T> {
        &self.inner
    }
}

impl<T: AsRef<[u8]> + AsMut<[u8]>> Ipv6Packet<T> {
    /// Return a mutable reference to the payload.
    pub fn payload_mut(&mut self) -> &mut [u8] {
        self.inner.payload_mut()
    }

    /// Set the source IPv6 address.
    pub fn set_src_addr(&mut self, addr: Ipv6Address) {
        self.inner.set_src_addr(addr.into());
    }

    /// Set the destination IPv6 address.
    pub fn set_dst_addr(&mut self, addr: Ipv6Address) {
        self.inner.set_dst_addr(addr.into());
    }

    /// Set the encapsulated protocol.
    pub fn set_next_header(&mut self, proto: IpProtocol) {
        self.inner.set_next_header(proto.into());
    }

    /// Set the hop limit field.
    pub fn set_hop_limit(&mut self, limit: u8) {
        self.inner.set_hop_limit(limit);
    }

    /// Set the payload length.
    pub fn set_payload_len(&mut self, length: u16) {
        self.inner.set_payload_len(length);
    }

    /// Set the version field (always 6 for IPv6).
    pub fn set_version(&mut self) {
        self.inner.set_version(6);
    }
}

#[cfg(ktest)]
mod tests {
    use super::*;
    use alloc::string::ToString;
    use ostd::prelude::ktest;

    #[ktest]
    fn test_ipv6_address() {
        let addr = Ipv6Address::new(0xfe80, 0, 0, 0, 0, 0, 0, 1);
        assert_eq!(addr.to_string(), "fe80::1");
        assert!(addr.is_unicast());
        assert!(!addr.is_unspecified());
        assert!(!addr.is_multicast());

        let loopback = Ipv6Address::LOOPBACK;
        assert!(loopback.is_loopback());
        assert_eq!(loopback.to_string(), "::1");
    }

    #[ktest]
    fn test_ipv6_packet() {
        let mut buffer = [0u8; 40 + 10]; // header (40) + payload (10)
        let mut packet = Ipv6Packet::new_unchecked(&mut buffer[..]);

        let src = Ipv6Address::new(0xfe80, 0, 0, 0, 0, 0, 0, 1);
        let dst = Ipv6Address::new(0xfe80, 0, 0, 0, 0, 0, 0, 2);

        packet.set_version();
        packet.set_payload_len(10);
        packet.set_src_addr(src);
        packet.set_dst_addr(dst);
        packet.set_next_header(IpProtocol::Udp);
        packet.set_hop_limit(64);
        packet.payload_mut().copy_from_slice(&[42u8; 10]);

        // Re-parse and verify
        let reader = Ipv6Packet::new_checked(&buffer[..]).unwrap();
        assert_eq!(reader.src_addr(), src);
        assert_eq!(reader.dst_addr(), dst);
        assert_eq!(reader.next_header(), IpProtocol::Udp);
        assert_eq!(reader.hop_limit(), 64);
        assert_eq!(reader.payload(), &[42u8; 10]);
    }
}
