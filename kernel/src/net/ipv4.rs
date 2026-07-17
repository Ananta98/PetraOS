//! IPv4 address, CIDR, and packet parsing/serialization wrapper.
//! Enforces memory safety and denies unsafe code.

use core::fmt;
use smoltcp::wire::{Ipv4Address as SmoltcpIpv4Address, Ipv4Cidr as SmoltcpIpv4Cidr};
use smoltcp::wire::{Ipv4Packet as SmoltcpIpv4Packet, IpProtocol as SmoltcpIpProtocol};

/// IP packet processing errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IpError {
    /// The buffer was too short to contain a valid packet.
    PacketTooShort,
    /// The prefix length was invalid.
    InvalidPrefix,
}

/// IP datagram encapsulated protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IpProtocol {
    /// Hop-by-hop option header (IPv6).
    HopByHop,
    /// Internet Control Message Protocol.
    Icmp,
    /// Internet Group Management Protocol.
    Igmp,
    /// Transmission Control Protocol.
    Tcp,
    /// User Datagram Protocol.
    Udp,
    /// IPv6 Routing header.
    Ipv6Route,
    /// IPv6 Fragment header.
    Ipv6Frag,
    /// Encapsulating Security Payload.
    IpSecEsp,
    /// Authentication Header.
    IpSecAh,
    /// ICMP for IPv6.
    Icmpv6,
    /// No next header (IPv6).
    Ipv6NoNxt,
    /// Destination options header (IPv6).
    Ipv6Opts,
    /// Unknown protocol value.
    Unknown(u8),
}

impl From<SmoltcpIpProtocol> for IpProtocol {
    fn from(proto: SmoltcpIpProtocol) -> Self {
        match proto {
            SmoltcpIpProtocol::HopByHop => IpProtocol::HopByHop,
            SmoltcpIpProtocol::Icmp => IpProtocol::Icmp,
            SmoltcpIpProtocol::Igmp => IpProtocol::Igmp,
            SmoltcpIpProtocol::Tcp => IpProtocol::Tcp,
            SmoltcpIpProtocol::Udp => IpProtocol::Udp,
            SmoltcpIpProtocol::Ipv6Route => IpProtocol::Ipv6Route,
            SmoltcpIpProtocol::Ipv6Frag => IpProtocol::Ipv6Frag,
            SmoltcpIpProtocol::IpSecEsp => IpProtocol::IpSecEsp,
            SmoltcpIpProtocol::IpSecAh => IpProtocol::IpSecAh,
            SmoltcpIpProtocol::Icmpv6 => IpProtocol::Icmpv6,
            SmoltcpIpProtocol::Ipv6NoNxt => IpProtocol::Ipv6NoNxt,
            SmoltcpIpProtocol::Ipv6Opts => IpProtocol::Ipv6Opts,
            SmoltcpIpProtocol::Unknown(val) => IpProtocol::Unknown(val),
        }
    }
}

impl From<IpProtocol> for SmoltcpIpProtocol {
    fn from(proto: IpProtocol) -> Self {
        match proto {
            IpProtocol::HopByHop => SmoltcpIpProtocol::HopByHop,
            IpProtocol::Icmp => SmoltcpIpProtocol::Icmp,
            IpProtocol::Igmp => SmoltcpIpProtocol::Igmp,
            IpProtocol::Tcp => SmoltcpIpProtocol::Tcp,
            IpProtocol::Udp => SmoltcpIpProtocol::Udp,
            IpProtocol::Ipv6Route => SmoltcpIpProtocol::Ipv6Route,
            IpProtocol::Ipv6Frag => SmoltcpIpProtocol::Ipv6Frag,
            IpProtocol::IpSecEsp => SmoltcpIpProtocol::IpSecEsp,
            IpProtocol::IpSecAh => SmoltcpIpProtocol::IpSecAh,
            IpProtocol::Icmpv6 => SmoltcpIpProtocol::Icmpv6,
            IpProtocol::Ipv6NoNxt => SmoltcpIpProtocol::Ipv6NoNxt,
            IpProtocol::Ipv6Opts => SmoltcpIpProtocol::Ipv6Opts,
            IpProtocol::Unknown(val) => SmoltcpIpProtocol::Unknown(val),
        }
    }
}

/// A four-octet IPv4 address.
#[derive(Debug, Hash, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Default)]
pub struct Ipv4Address(pub [u8; 4]);

impl Ipv4Address {
    /// An unspecified address.
    pub const UNSPECIFIED: Self = Self([0x00; 4]);

    /// The broadcast address.
    pub const BROADCAST: Self = Self([0xff; 4]);

    /// Construct an IPv4 address from parts.
    pub const fn new(a0: u8, a1: u8, a2: u8, a3: u8) -> Self {
        Self([a0, a1, a2, a3])
    }

    /// Construct an IPv4 address from a sequence of octets, in big-endian.
    pub fn from_bytes(data: &[u8]) -> Self {
        let mut bytes = [0; 4];
        bytes.copy_from_slice(data);
        Self(bytes)
    }

    /// Return an IPv4 address as a sequence of octets, in big-endian.
    pub const fn as_bytes(&self) -> &[u8; 4] {
        &self.0
    }

    /// Query whether the address is an unicast address.
    pub fn is_unicast(&self) -> bool {
        !(self.is_broadcast() || self.is_multicast() || self.is_unspecified())
    }

    /// Query whether the address is the broadcast address.
    pub fn is_broadcast(&self) -> bool {
        self.0 == [255; 4]
    }

    /// Query whether the address is a multicast address.
    pub const fn is_multicast(&self) -> bool {
        self.0[0] & 0xf0 == 224
    }

    /// Query whether the address falls into the "unspecified" range.
    pub const fn is_unspecified(&self) -> bool {
        self.0[0] == 0
    }

    /// Query whether the address falls into the "loopback" range.
    pub const fn is_loopback(&self) -> bool {
        self.0[0] == 127
    }
}

impl fmt::Display for Ipv4Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", SmoltcpIpv4Address::from(*self))
    }
}

impl From<[u8; 4]> for Ipv4Address {
    fn from(bytes: [u8; 4]) -> Self {
        Self(bytes)
    }
}

impl From<Ipv4Address> for [u8; 4] {
    fn from(addr: Ipv4Address) -> Self {
        addr.0
    }
}

impl From<SmoltcpIpv4Address> for Ipv4Address {
    fn from(addr: SmoltcpIpv4Address) -> Self {
        Self(addr.0)
    }
}

impl From<Ipv4Address> for SmoltcpIpv4Address {
    fn from(addr: Ipv4Address) -> Self {
        SmoltcpIpv4Address(addr.0)
    }
}

/// A specification of an IPv4 CIDR block.
#[derive(Debug, Hash, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Default)]
pub struct Ipv4Cidr {
    address: Ipv4Address,
    prefix_len: u8,
}

impl Ipv4Cidr {
    /// Create a new IPv4 CIDR block.
    pub const fn new(address: Ipv4Address, prefix_len: u8) -> Self {
        Self { address, prefix_len }
    }

    /// Return the address of this CIDR block.
    pub const fn address(&self) -> Ipv4Address {
        self.address
    }

    /// Return the prefix length of this CIDR block.
    pub const fn prefix_len(&self) -> u8 {
        self.prefix_len
    }
}

impl fmt::Display for Ipv4Cidr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.address, self.prefix_len)
    }
}

impl From<SmoltcpIpv4Cidr> for Ipv4Cidr {
    fn from(cidr: SmoltcpIpv4Cidr) -> Self {
        Self {
            address: cidr.address().into(),
            prefix_len: cidr.prefix_len(),
        }
    }
}

impl From<Ipv4Cidr> for SmoltcpIpv4Cidr {
    fn from(cidr: Ipv4Cidr) -> Self {
        Self::new(cidr.address.into(), cidr.prefix_len)
    }
}

/// Parser and builder for Internet Protocol version 4 packets.
#[derive(Debug)]
pub struct Ipv4Packet<T: AsRef<[u8]>> {
    inner: SmoltcpIpv4Packet<T>,
}

impl<T: AsRef<[u8]>> Ipv4Packet<T> {
    /// Parse an IPv4 packet, verifying the buffer length and checksum.
    pub fn new_checked(buffer: T) -> Result<Self, IpError> {
        let packet = SmoltcpIpv4Packet::new_checked(buffer)
            .map_err(|_| IpError::PacketTooShort)?;
        Ok(Self { inner: packet })
    }

    /// Construct an IPv4 packet wrapper without verifying the length or checksum.
    pub fn new_unchecked(buffer: T) -> Self {
        Self {
            inner: SmoltcpIpv4Packet::new_unchecked(buffer),
        }
    }

    /// Return the source IPv4 address.
    pub fn src_addr(&self) -> Ipv4Address {
        self.inner.src_addr().into()
    }

    /// Return the destination IPv4 address.
    pub fn dst_addr(&self) -> Ipv4Address {
        self.inner.dst_addr().into()
    }

    /// Return the encapsulated protocol.
    pub fn next_header(&self) -> IpProtocol {
        self.inner.next_header().into()
    }

    /// Return the Time-to-Live (hop limit) field.
    pub fn hop_limit(&self) -> u8 {
        self.inner.hop_limit()
    }

    /// Return the header length, in octets.
    pub fn header_len(&self) -> u8 {
        self.inner.header_len()
    }

    /// Return the total length of the packet.
    pub fn total_len(&self) -> u16 {
        self.inner.total_len()
    }

    /// Return the payload of the packet.
    pub fn payload(&self) -> &[u8] {
        let header_len = self.inner.header_len() as usize;
        let total_len = self.inner.total_len() as usize;
        &self.inner.as_ref()[header_len..total_len]
    }

    /// Verify the header checksum.
    pub fn verify_checksum(&self) -> bool {
        self.inner.verify_checksum()
    }

    /// Return a reference to the underlying packet buffer.
    pub fn inner(&self) -> &SmoltcpIpv4Packet<T> {
        &self.inner
    }
}

impl<T: AsRef<[u8]> + AsMut<[u8]>> Ipv4Packet<T> {
    /// Return a mutable reference to the payload.
    pub fn payload_mut(&mut self) -> &mut [u8] {
        self.inner.payload_mut()
    }

    /// Set the source IPv4 address.
    pub fn set_src_addr(&mut self, addr: Ipv4Address) {
        self.inner.set_src_addr(addr.into());
    }

    /// Set the destination IPv4 address.
    pub fn set_dst_addr(&mut self, addr: Ipv4Address) {
        self.inner.set_dst_addr(addr.into());
    }

    /// Set the encapsulated protocol.
    pub fn set_next_header(&mut self, proto: IpProtocol) {
        self.inner.set_next_header(proto.into());
    }

    /// Set the Time-to-Live (hop limit) field.
    pub fn set_hop_limit(&mut self, limit: u8) {
        self.inner.set_hop_limit(limit);
    }

    /// Set the total length of the packet.
    pub fn set_total_len(&mut self, length: u16) {
        self.inner.set_total_len(length);
    }

    /// Set the version field (always 4 for IPv4).
    pub fn set_version(&mut self) {
        self.inner.set_version(4);
    }

    /// Set the header length field (in units of 32-bit words, standard is 5).
    pub fn set_header_len(&mut self, len: u8) {
        self.inner.set_header_len(len);
    }

    /// Compute and fill the header checksum.
    pub fn fill_checksum(&mut self) {
        self.inner.fill_checksum();
    }
}

#[cfg(ktest)]
mod tests {
    use super::*;
    use alloc::string::ToString;
    use ostd::prelude::ktest;

    #[ktest]
    fn test_ipv4_address() {
        let addr = Ipv4Address::new(192, 168, 1, 1);
        assert_eq!(addr.to_string(), "192.168.1.1");
        assert!(addr.is_unicast());
        assert!(!addr.is_broadcast());
        assert!(!addr.is_multicast());
        assert!(!addr.is_loopback());

        let loopback = Ipv4Address::new(127, 0, 0, 1);
        assert!(loopback.is_loopback());
    }

    #[ktest]
    fn test_ipv4_packet() {
        let mut buffer = [0u8; 20 + 10]; // header (20) + payload (10)
        let mut packet = Ipv4Packet::new_unchecked(&mut buffer[..]);

        let src = Ipv4Address::new(10, 0, 0, 1);
        let dst = Ipv4Address::new(10, 0, 0, 2);

        packet.set_version();
        packet.set_header_len(20);
        packet.set_total_len(30);
        packet.set_src_addr(src);
        packet.set_dst_addr(dst);
        packet.set_next_header(IpProtocol::Udp);
        packet.set_hop_limit(64);
        packet.payload_mut().copy_from_slice(&[42u8; 10]);
        packet.fill_checksum();

        // Re-parse and verify
        let reader = Ipv4Packet::new_checked(&buffer[..]).unwrap();
        assert_eq!(reader.src_addr(), src);
        assert_eq!(reader.dst_addr(), dst);
        assert_eq!(reader.next_header(), IpProtocol::Udp);
        assert_eq!(reader.hop_limit(), 64);
        assert_eq!(reader.payload(), &[42u8; 10]);
        assert!(reader.verify_checksum());
    }
}
