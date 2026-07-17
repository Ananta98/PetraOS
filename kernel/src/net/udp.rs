use core::fmt;
use smoltcp::phy::ChecksumCapabilities;
use smoltcp::wire::{IpAddress, UdpPacket as SmoltcpUdpPacket, UdpRepr as SmoltcpUdpRepr, UDP_HEADER_LEN};

/// UDP processing errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UdpError {
    /// The buffer was too short to contain a valid UDP header.
    PacketTooShort,
    /// The checksum is invalid.
    ChecksumInvalid,
}

/// A read/write wrapper around a User Datagram Protocol packet buffer.
#[derive(Debug, Clone)]
pub struct UdpPacket<T: AsRef<[u8]>> {
    inner: SmoltcpUdpPacket<T>,
}

impl<T: AsRef<[u8]>> UdpPacket<T> {
    /// Parse a UDP packet, verifying the buffer length.
    pub fn new_checked(buffer: T) -> Result<Self, UdpError> {
        let packet = SmoltcpUdpPacket::new_checked(buffer).map_err(|_| UdpError::PacketTooShort)?;
        Ok(Self { inner: packet })
    }

    /// Construct a UDP packet wrapper without verifying the length.
    pub fn new_unchecked(buffer: T) -> Self {
        Self {
            inner: SmoltcpUdpPacket::new_unchecked(buffer),
        }
    }

    /// Consume the packet, returning the underlying buffer.
    pub fn into_inner(self) -> T {
        self.inner.into_inner()
    }

    /// Return the source port field.
    pub fn src_port(&self) -> u16 {
        self.inner.src_port()
    }

    /// Return the destination port field.
    pub fn dst_port(&self) -> u16 {
        self.inner.dst_port()
    }

    /// Return the length field.
    pub fn len(&self) -> u16 {
        self.inner.len()
    }

    /// Check if the packet has no payload.
    pub fn is_empty(&self) -> bool {
        self.len() <= UDP_HEADER_LEN as u16
    }

    /// Return the checksum field.
    pub fn checksum(&self) -> u16 {
        self.inner.checksum()
    }

    /// Validate the packet checksum against the given source and destination IP addresses.
    pub fn verify_checksum(&self, src_addr: &IpAddress, dst_addr: &IpAddress) -> bool {
        self.inner.verify_checksum(src_addr, dst_addr)
    }

    /// Return a reference to the underlying packet buffer.
    pub fn inner(&self) -> &SmoltcpUdpPacket<T> {
        &self.inner
    }
}

impl<'a, T: AsRef<[u8]> + ?Sized> UdpPacket<&'a T> {
    /// Return a pointer to the payload.
    pub fn payload(&self) -> &'a [u8] {
        self.inner.payload()
    }
}

impl<T: AsRef<[u8]> + AsMut<[u8]>> UdpPacket<T> {
    /// Set the source port field.
    pub fn set_src_port(&mut self, value: u16) {
        self.inner.set_src_port(value);
    }

    /// Set the destination port field.
    pub fn set_dst_port(&mut self, value: u16) {
        self.inner.set_dst_port(value);
    }

    /// Set the length field.
    pub fn set_len(&mut self, value: u16) {
        self.inner.set_len(value);
    }

    /// Set the checksum field.
    pub fn set_checksum(&mut self, value: u16) {
        self.inner.set_checksum(value);
    }

    /// Compute and fill the header checksum.
    pub fn fill_checksum(&mut self, src_addr: &IpAddress, dst_addr: &IpAddress) {
        self.inner.fill_checksum(src_addr, dst_addr);
    }

    /// Return a mutable pointer to the payload.
    pub fn payload_mut(&mut self) -> &mut [u8] {
        self.inner.payload_mut()
    }
}

impl<T: AsRef<[u8]>> AsRef<[u8]> for UdpPacket<T> {
    fn as_ref(&self) -> &[u8] {
        self.inner.as_ref()
    }
}

/// A high-level representation of a UDP packet header.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UdpRepr {
    pub src_port: u16,
    pub dst_port: u16,
}

impl UdpRepr {
    /// Parse a UDP packet and return a high-level representation.
    pub fn parse<T>(
        packet: &UdpPacket<&T>,
        src_addr: &IpAddress,
        dst_addr: &IpAddress,
        checksum_caps: &ChecksumCapabilities,
    ) -> Result<Self, UdpError>
    where
        T: AsRef<[u8]> + ?Sized,
    {
        let repr =
            SmoltcpUdpRepr::parse(&packet.inner, src_addr, dst_addr, checksum_caps)
                .map_err(|_| UdpError::ChecksumInvalid)?;
        Ok(Self {
            src_port: repr.src_port,
            dst_port: repr.dst_port,
        })
    }

    /// Return the length of the packet header.
    pub const fn header_len(&self) -> usize {
        UDP_HEADER_LEN
    }

    /// Emit this high-level representation into a UDP packet.
    pub fn emit<T: ?Sized>(
        &self,
        packet: &mut UdpPacket<&mut T>,
        src_addr: &IpAddress,
        dst_addr: &IpAddress,
        payload_len: usize,
        emit_payload: impl FnOnce(&mut [u8]),
        checksum_caps: &ChecksumCapabilities,
    ) where
        T: AsRef<[u8]> + AsMut<[u8]>,
    {
        let repr = SmoltcpUdpRepr {
            src_port: self.src_port,
            dst_port: self.dst_port,
        };
        repr.emit(
            &mut packet.inner,
            src_addr,
            dst_addr,
            payload_len,
            emit_payload,
            checksum_caps,
        );
    }
}

impl fmt::Display for UdpRepr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "UDP src={} dst={}",
            self.src_port, self.dst_port
        )
    }
}

#[cfg(ktest)]
mod tests {
    use super::*;
    use ostd::prelude::ktest;
    use smoltcp::wire::Ipv4Address;

    const SRC_IP: Ipv4Address = Ipv4Address::new(192, 168, 1, 1);
    const DST_IP: Ipv4Address = Ipv4Address::new(192, 168, 1, 2);

    static PACKET_BYTES: [u8; 12] = [
        0xbf, 0x00, 0x00, 0x35, 0x00, 0x0c, 0x12, 0x4d, 0xaa, 0x00, 0x00, 0xff,
    ];

    static PAYLOAD_BYTES: [u8; 4] = [0xaa, 0x00, 0x00, 0xff];

    #[ktest]
    fn test_udp_deconstruct() {
        let packet = UdpPacket::new_unchecked(&PACKET_BYTES[..]);
        assert_eq!(packet.src_port(), 48896);
        assert_eq!(packet.dst_port(), 53);
        assert_eq!(packet.len(), 12);
        assert_eq!(packet.checksum(), 0x124d);
        assert_eq!(packet.payload(), &PAYLOAD_BYTES[..]);
        assert!(packet.verify_checksum(&SRC_IP.into(), &DST_IP.into()));
    }

    #[ktest]
    fn test_udp_construct() {
        let mut bytes = [0xa5u8; 12];
        let mut packet = UdpPacket::new_unchecked(&mut bytes[..]);
        packet.set_src_port(48896);
        packet.set_dst_port(53);
        packet.set_len(12);
        packet.set_checksum(0xffff);
        packet.payload_mut().copy_from_slice(&PAYLOAD_BYTES[..]);
        packet.fill_checksum(&SRC_IP.into(), &DST_IP.into());
        assert_eq!(*packet.into_inner(), PACKET_BYTES);
    }

    #[ktest]
    fn test_udp_repr_parse() {
        let packet = UdpPacket::new_unchecked(&PACKET_BYTES[..]);
        let repr = UdpRepr::parse(
            &packet,
            &SRC_IP.into(),
            &DST_IP.into(),
            &ChecksumCapabilities::default(),
        )
        .unwrap();
        assert_eq!(
            repr,
            UdpRepr {
                src_port: 48896,
                dst_port: 53,
            }
        );
    }

    #[ktest]
    fn test_udp_repr_emit() {
        let repr = UdpRepr {
            src_port: 48896,
            dst_port: 53,
        };
        let mut bytes = [0xa5u8; 12];
        let mut packet = UdpPacket::new_unchecked(&mut bytes[..]);
        repr.emit(
            &mut packet,
            &SRC_IP.into(),
            &DST_IP.into(),
            PAYLOAD_BYTES.len(),
            |payload| payload.copy_from_slice(&PAYLOAD_BYTES),
            &ChecksumCapabilities::default(),
        );
        assert_eq!(*packet.into_inner(), PACKET_BYTES);
    }
}
