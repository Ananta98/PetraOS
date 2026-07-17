//! ARP packet parsing and serialization wrapper.
//! Enforces safety guidelines and denies unsafe code.

use core::fmt;
use smoltcp::wire::{ArpPacket as SmoltcpArpPacket, ArpHardware as SmoltcpArpHardware, ArpOperation as SmoltcpArpOperation};
use crate::net::eth::{MacAddress, EtherType};
use crate::net::ipv4::{Ipv4Address, IpError};

/// ARP processing errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArpError {
    /// The buffer was too short to contain a valid ARP packet.
    PacketTooShort,
    /// Invalid hardware or protocol address length.
    InvalidLength,
}

/// ARP hardware type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArpHardware {
    /// Ethernet hardware interface.
    Ethernet,
    /// Unknown hardware type.
    Unknown(u16),
}

impl From<SmoltcpArpHardware> for ArpHardware {
    fn from(hw: SmoltcpArpHardware) -> Self {
        match hw {
            SmoltcpArpHardware::Ethernet => ArpHardware::Ethernet,
            SmoltcpArpHardware::Unknown(val) => ArpHardware::Unknown(val),
        }
    }
}

impl From<ArpHardware> for SmoltcpArpHardware {
    fn from(hw: ArpHardware) -> Self {
        match hw {
            ArpHardware::Ethernet => SmoltcpArpHardware::Ethernet,
            ArpHardware::Unknown(val) => SmoltcpArpHardware::Unknown(val),
        }
    }
}

/// ARP operation type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArpOperation {
    /// Request operation (queries target MAC for a target IP).
    Request,
    /// Reply operation (resolves the requested MAC address).
    Reply,
    /// Unknown operation type.
    Unknown(u16),
}

impl From<SmoltcpArpOperation> for ArpOperation {
    fn from(op: SmoltcpArpOperation) -> Self {
        match op {
            SmoltcpArpOperation::Request => ArpOperation::Request,
            SmoltcpArpOperation::Reply => ArpOperation::Reply,
            SmoltcpArpOperation::Unknown(val) => ArpOperation::Unknown(val),
        }
    }
}

impl From<ArpOperation> for SmoltcpArpOperation {
    fn from(op: ArpOperation) -> Self {
        match op {
            ArpOperation::Request => SmoltcpArpOperation::Request,
            ArpOperation::Reply => SmoltcpArpOperation::Reply,
            ArpOperation::Unknown(val) => SmoltcpArpOperation::Unknown(val),
        }
    }
}

/// A read/write wrapper around an Address Resolution Protocol (ARP) packet buffer.
#[derive(Debug)]
pub struct ArpPacket<T: AsRef<[u8]>> {
    inner: SmoltcpArpPacket<T>,
}

impl<T: AsRef<[u8]>> ArpPacket<T> {
    /// Parse an ARP packet, verifying the buffer length.
    pub fn new_checked(buffer: T) -> Result<Self, ArpError> {
        let packet = SmoltcpArpPacket::new_checked(buffer)
            .map_err(|_| ArpError::PacketTooShort)?;
        Ok(Self { inner: packet })
    }

    /// Construct an ARP packet wrapper without length verification.
    pub fn new_unchecked(buffer: T) -> Self {
        Self {
            inner: SmoltcpArpPacket::new_unchecked(buffer),
        }
    }

    /// Return the hardware type field.
    pub fn hardware_type(&self) -> ArpHardware {
        self.inner.hardware_type().into()
    }

    /// Return the protocol type field.
    pub fn protocol_type(&self) -> EtherType {
        let proto = self.inner.protocol_type();
        EtherType::from(proto)
    }

    /// Return the hardware length field (typically 6 for MAC addresses).
    pub fn hardware_len(&self) -> u8 {
        self.inner.hardware_len()
    }

    /// Return the protocol length field (typically 4 for IPv4).
    pub fn protocol_len(&self) -> u8 {
        self.inner.protocol_len()
    }

    /// Return the operation field.
    pub fn operation(&self) -> ArpOperation {
        self.inner.operation().into()
    }

    /// Return the source hardware address (MAC).
    pub fn source_hardware_addr(&self) -> MacAddress {
        let sha = self.inner.source_hardware_addr();
        if sha.len() == 6 {
            let mut bytes = [0u8; 6];
            bytes.copy_from_slice(sha);
            MacAddress::new(bytes)
        } else {
            MacAddress::ZERO
        }
    }

    /// Return the source protocol address (IPv4).
    pub fn source_protocol_addr(&self) -> Ipv4Address {
        let spa = self.inner.source_protocol_addr();
        if spa.len() == 4 {
            Ipv4Address::from_bytes(spa)
        } else {
            Ipv4Address::UNSPECIFIED
        }
    }

    /// Return the target hardware address (MAC).
    pub fn target_hardware_addr(&self) -> MacAddress {
        let tha = self.inner.target_hardware_addr();
        if tha.len() == 6 {
            let mut bytes = [0u8; 6];
            bytes.copy_from_slice(tha);
            MacAddress::new(bytes)
        } else {
            MacAddress::ZERO
        }
    }

    /// Return the target protocol address (IPv4).
    pub fn target_protocol_addr(&self) -> Ipv4Address {
        let tpa = self.inner.target_protocol_addr();
        if tpa.len() == 4 {
            Ipv4Address::from_bytes(tpa)
        } else {
            Ipv4Address::UNSPECIFIED
        }
    }

    /// Return a reference to the underlying packet buffer.
    pub fn inner(&self) -> &SmoltcpArpPacket<T> {
        &self.inner
    }
}

impl<T: AsRef<[u8]> + AsMut<[u8]>> ArpPacket<T> {
    /// Set the hardware type field.
    pub fn set_hardware_type(&mut self, value: ArpHardware) {
        self.inner.set_hardware_type(value.into());
    }

    /// Set the protocol type field.
    pub fn set_protocol_type(&mut self, value: EtherType) {
        self.inner.set_protocol_type(value.into());
    }

    /// Set the hardware length field.
    pub fn set_hardware_len(&mut self, value: u8) {
        self.inner.set_hardware_len(value);
    }

    /// Set the protocol length field.
    pub fn set_protocol_len(&mut self, value: u8) {
        self.inner.set_protocol_len(value);
    }

    /// Set the operation field.
    pub fn set_operation(&mut self, value: ArpOperation) {
        self.inner.set_operation(value.into());
    }

    /// Set the source hardware address field.
    pub fn set_source_hardware_addr(&mut self, value: MacAddress) {
        self.inner.set_source_hardware_addr(value.as_bytes());
    }

    /// Set the source protocol address field.
    pub fn set_source_protocol_addr(&mut self, value: Ipv4Address) {
        self.inner.set_source_protocol_addr(value.as_bytes());
    }

    /// Set the target hardware address field.
    pub fn set_target_hardware_addr(&mut self, value: MacAddress) {
        self.inner.set_target_hardware_addr(value.as_bytes());
    }

    /// Set the target protocol address field.
    pub fn set_target_protocol_addr(&mut self, value: Ipv4Address) {
        self.inner.set_target_protocol_addr(value.as_bytes());
    }
}

#[cfg(ktest)]
mod tests {
    use super::*;
    use ostd::prelude::ktest;

    #[ktest]
    fn test_arp_packet() {
        let mut buffer = [0u8; 28]; // standard Ethernet+IPv4 ARP size is 28 bytes
        let mut packet = ArpPacket::new_unchecked(&mut buffer[..]);

        let sha = MacAddress::new([0x02, 0x02, 0x03, 0x04, 0x05, 0x06]);
        let spa = Ipv4Address::new(192, 168, 1, 1);
        let tha = MacAddress::new([0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f]);
        let tpa = Ipv4Address::new(192, 168, 1, 2);

        packet.set_hardware_type(ArpHardware::Ethernet);
        packet.set_protocol_type(EtherType::Ipv4);
        packet.set_hardware_len(6);
        packet.set_protocol_len(4);
        packet.set_operation(ArpOperation::Request);
        packet.set_source_hardware_addr(sha);
        packet.set_source_protocol_addr(spa);
        packet.set_target_hardware_addr(tha);
        packet.set_target_protocol_addr(tpa);

        // Re-parse and verify
        let reader = ArpPacket::new_checked(&buffer[..]).unwrap();
        assert_eq!(reader.hardware_type(), ArpHardware::Ethernet);
        assert_eq!(reader.protocol_type(), EtherType::Ipv4);
        assert_eq!(reader.hardware_len(), 6);
        assert_eq!(reader.protocol_len(), 4);
        assert_eq!(reader.operation(), ArpOperation::Request);
        assert_eq!(reader.source_hardware_addr(), sha);
        assert_eq!(reader.source_protocol_addr(), spa);
        assert_eq!(reader.target_hardware_addr(), tha);
        assert_eq!(reader.target_protocol_addr(), tpa);
    }
}
