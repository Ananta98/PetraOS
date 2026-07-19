//! Ethernet parsing and serialization library for PetraOS.
//! Enforces memory safety and denies unsafe code.

use core::fmt;
use smoltcp::wire::{EthernetAddress, EthernetFrame as SmoltcpEthernetFrame, EthernetProtocol};

/// The Ethernet header length.
pub const ETHERNET_HEADER_LEN: usize = 14;

/// Representation of a 48-bit MAC (Ethernet) address.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MacAddress(pub [u8; 6]);

impl MacAddress {
    /// Broadcast MAC address: FF:FF:FF:FF:FF:FF
    pub const BROADCAST: Self = Self([0xff; 6]);

    /// Zero MAC address: 00:00:00:00:00:00
    pub const ZERO: Self = Self([0x00; 6]);

    /// Create a new MAC address from 6 bytes.
    pub const fn new(addr: [u8; 6]) -> Self {
        Self(addr)
    }

    /// Check if the address is broadcast.
    pub fn is_broadcast(&self) -> bool {
        self.0 == [0xff; 6]
    }

    /// Check if the address is multicast.
    pub fn is_multicast(&self) -> bool {
        (self.0[0] & 0x01) != 0
    }

    /// Check if the address is unicast (neither broadcast nor multicast).
    pub fn is_unicast(&self) -> bool {
        !self.is_multicast() && !self.is_broadcast()
    }

    /// Get a reference to the raw bytes of the MAC address.
    pub const fn as_bytes(&self) -> &[u8; 6] {
        &self.0
    }
}

impl fmt::Display for MacAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            self.0[0], self.0[1], self.0[2], self.0[3], self.0[4], self.0[5]
        )
    }
}

impl From<[u8; 6]> for MacAddress {
    fn from(bytes: [u8; 6]) -> Self {
        Self(bytes)
    }
}

impl From<MacAddress> for [u8; 6] {
    fn from(mac: MacAddress) -> Self {
        mac.0
    }
}

impl From<EthernetAddress> for MacAddress {
    fn from(addr: EthernetAddress) -> Self {
        Self(addr.0)
    }
}

impl From<MacAddress> for EthernetAddress {
    fn from(mac: MacAddress) -> Self {
        Self(mac.0)
    }
}

/// Supported Ethernet protocol types (EtherType).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EtherType {
    /// Internet Protocol version 4.
    Ipv4,
    /// Address Resolution Protocol.
    Arp,
    /// Internet Protocol version 6.
    Ipv6,
    /// Unknown / unsupported protocol type.
    Unknown(u16),
}

impl From<EthernetProtocol> for EtherType {
    fn from(protocol: EthernetProtocol) -> Self {
        match protocol {
            EthernetProtocol::Ipv4 => EtherType::Ipv4,
            EthernetProtocol::Arp => EtherType::Arp,
            EthernetProtocol::Ipv6 => EtherType::Ipv6,
            EthernetProtocol::Unknown(val) => EtherType::Unknown(val),
        }
    }
}

impl From<EtherType> for EthernetProtocol {
    fn from(ether_type: EtherType) -> Self {
        match ether_type {
            EtherType::Ipv4 => EthernetProtocol::Ipv4,
            EtherType::Arp => EthernetProtocol::Arp,
            EtherType::Ipv6 => EthernetProtocol::Ipv6,
            EtherType::Unknown(val) => EthernetProtocol::Unknown(val),
        }
    }
}

impl From<u16> for EtherType {
    fn from(val: u16) -> Self {
        Self::from(EthernetProtocol::from(val))
    }
}

impl From<EtherType> for u16 {
    fn from(ether_type: EtherType) -> Self {
        u16::from(EthernetProtocol::from(ether_type))
    }
}

/// Ethernet processing errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EtherError {
    /// The buffer is too small to fit the Ethernet frame header.
    PacketTooShort,
    /// The address format is invalid.
    InvalidAddress,
}

/// Parser and builder for Ethernet frames.
#[derive(Debug)]
pub struct EthernetFrame<T: AsRef<[u8]>> {
    inner: SmoltcpEthernetFrame<T>,
}

impl<T: AsRef<[u8]>> EthernetFrame<T> {
    /// Parse an Ethernet frame from a raw byte slice, verifying buffer length.
    pub fn new_checked(buffer: T) -> Result<Self, EtherError> {
        let frame =
            SmoltcpEthernetFrame::new_checked(buffer).map_err(|_| EtherError::PacketTooShort)?;
        Ok(Self { inner: frame })
    }

    /// Construct an Ethernet frame wrapper without verifying buffer length.
    pub fn new_unchecked(buffer: T) -> Self {
        Self {
            inner: SmoltcpEthernetFrame::new_unchecked(buffer),
        }
    }

    /// Return the destination MAC address of the frame.
    pub fn dst_addr(&self) -> MacAddress {
        self.inner.dst_addr().into()
    }

    /// Return the source MAC address of the frame.
    pub fn src_addr(&self) -> MacAddress {
        self.inner.src_addr().into()
    }

    /// Return the EtherType of the frame.
    pub fn ethertype(&self) -> EtherType {
        self.inner.ethertype().into()
    }

    /// Return a reference to the payload of the frame.
    pub fn payload(&self) -> &[u8] {
        &self.inner.as_ref()[ETHERNET_HEADER_LEN..]
    }

    /// Return a reference to the underlying frame buffer.
    pub fn inner(&self) -> &SmoltcpEthernetFrame<T> {
        &self.inner
    }
}

impl<T: AsRef<[u8]> + AsMut<[u8]>> EthernetFrame<T> {
    /// Return a mutable reference to the payload of the frame.
    pub fn payload_mut(&mut self) -> &mut [u8] {
        self.inner.payload_mut()
    }

    /// Set the destination MAC address of the frame.
    pub fn set_dst_addr(&mut self, addr: MacAddress) {
        self.inner.set_dst_addr(addr.into());
    }

    /// Set the source MAC address of the frame.
    pub fn set_src_addr(&mut self, addr: MacAddress) {
        self.inner.set_src_addr(addr.into());
    }

    /// Set the EtherType of the frame.
    pub fn set_ethertype(&mut self, ethertype: EtherType) {
        self.inner.set_ethertype(ethertype.into());
    }
}

#[cfg(ktest)]
mod tests {
    use super::*;
    use alloc::string::ToString;
    use ostd::prelude::ktest;

    #[ktest]
    fn test_mac_address() {
        let mac = MacAddress::new([0x02, 0x02, 0x03, 0x04, 0x05, 0x06]);
        assert_eq!(mac.to_string(), "02:02:03:04:05:06");
        assert!(mac.is_unicast());
        assert!(!mac.is_broadcast());
        assert!(!mac.is_multicast());

        let broadcast = MacAddress::BROADCAST;
        assert!(broadcast.is_broadcast());
        assert!(broadcast.is_multicast());
    }

    #[ktest]
    fn test_ethertype() {
        assert_eq!(EtherType::from(0x0800), EtherType::Ipv4);
        assert_eq!(EtherType::from(0x0806), EtherType::Arp);
        assert_eq!(EtherType::from(0x86dd), EtherType::Ipv6);
        assert_eq!(EtherType::from(0x1234), EtherType::Unknown(0x1234));
    }

    #[ktest]
    fn test_ethernet_frame() {
        let mut buffer = [0u8; 14 + 10]; // header (14) + payload (10)
        let mut frame = EthernetFrame::new_unchecked(&mut buffer[..]);

        let dst = MacAddress::new([0x01, 0x02, 0x03, 0x04, 0x05, 0x06]);
        let src = MacAddress::new([0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f]);

        frame.set_dst_addr(dst);
        frame.set_src_addr(src);
        frame.set_ethertype(EtherType::Ipv4);
        frame.payload_mut().copy_from_slice(&[42u8; 10]);

        // Re-parse and verify
        let reader = EthernetFrame::new_checked(&buffer[..]).unwrap();
        assert_eq!(reader.dst_addr(), dst);
        assert_eq!(reader.src_addr(), src);
        assert_eq!(reader.ethertype(), EtherType::Ipv4);
        assert_eq!(reader.payload(), &[42u8; 10]);
    }
}
