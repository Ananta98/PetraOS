use core::fmt;
use smoltcp::phy::ChecksumCapabilities;
use smoltcp::wire::{
    Icmpv4DstUnreachable as SmoltcpDstUnreachable, Icmpv4Message as SmoltcpMessage,
    Icmpv4Packet as SmoltcpIcmpv4Packet, Icmpv4Repr as SmoltcpIcmpv4Repr,
    Icmpv4TimeExceeded as SmoltcpTimeExceeded, Ipv4Repr,
};

/// ICMP processing errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IcmpError {
    /// The buffer was too short to contain a valid ICMP header.
    PacketTooShort,
    /// The checksum is invalid.
    ChecksumInvalid,
    /// Unsupported ICMP message type or code.
    Unsupported,
}

/// ICMPv4 message types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IcmpMessage {
    /// Echo reply (ping response).
    EchoReply,
    /// Destination unreachable.
    DstUnreachable,
    /// Message redirect.
    Redirect,
    /// Echo request (ping).
    EchoRequest,
    /// Router advertisement.
    RouterAdvert,
    /// Router solicitation.
    RouterSolicit,
    /// Time exceeded.
    TimeExceeded,
    /// Parameter problem.
    ParamProblem,
    /// Timestamp.
    Timestamp,
    /// Timestamp reply.
    TimestampReply,
    /// Unknown message type.
    Unknown(u8),
}

impl From<SmoltcpMessage> for IcmpMessage {
    fn from(msg: SmoltcpMessage) -> Self {
        match msg {
            SmoltcpMessage::EchoReply => IcmpMessage::EchoReply,
            SmoltcpMessage::DstUnreachable => IcmpMessage::DstUnreachable,
            SmoltcpMessage::Redirect => IcmpMessage::Redirect,
            SmoltcpMessage::EchoRequest => IcmpMessage::EchoRequest,
            SmoltcpMessage::RouterAdvert => IcmpMessage::RouterAdvert,
            SmoltcpMessage::RouterSolicit => IcmpMessage::RouterSolicit,
            SmoltcpMessage::TimeExceeded => IcmpMessage::TimeExceeded,
            SmoltcpMessage::ParamProblem => IcmpMessage::ParamProblem,
            SmoltcpMessage::Timestamp => IcmpMessage::Timestamp,
            SmoltcpMessage::TimestampReply => IcmpMessage::TimestampReply,
            SmoltcpMessage::Unknown(val) => IcmpMessage::Unknown(val),
        }
    }
}

impl From<IcmpMessage> for SmoltcpMessage {
    fn from(msg: IcmpMessage) -> Self {
        match msg {
            IcmpMessage::EchoReply => SmoltcpMessage::EchoReply,
            IcmpMessage::DstUnreachable => SmoltcpMessage::DstUnreachable,
            IcmpMessage::Redirect => SmoltcpMessage::Redirect,
            IcmpMessage::EchoRequest => SmoltcpMessage::EchoRequest,
            IcmpMessage::RouterAdvert => SmoltcpMessage::RouterAdvert,
            IcmpMessage::RouterSolicit => SmoltcpMessage::RouterSolicit,
            IcmpMessage::TimeExceeded => SmoltcpMessage::TimeExceeded,
            IcmpMessage::ParamProblem => SmoltcpMessage::ParamProblem,
            IcmpMessage::Timestamp => SmoltcpMessage::Timestamp,
            IcmpMessage::TimestampReply => SmoltcpMessage::TimestampReply,
            IcmpMessage::Unknown(val) => SmoltcpMessage::Unknown(val),
        }
    }
}

/// Destination unreachable reasons for ICMPv4.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IcmpDstUnreachable {
    NetUnreachable,
    HostUnreachable,
    ProtoUnreachable,
    PortUnreachable,
    FragRequired,
    SrcRouteFailed,
    DstNetUnknown,
    DstHostUnknown,
    SrcHostIsolated,
    NetProhibited,
    HostProhibited,
    NetUnreachToS,
    HostUnreachToS,
    CommProhibited,
    HostPrecedViol,
    PrecedCutoff,
    Unknown(u8),
}

impl From<SmoltcpDstUnreachable> for IcmpDstUnreachable {
    fn from(r: SmoltcpDstUnreachable) -> Self {
        match r {
            SmoltcpDstUnreachable::NetUnreachable => IcmpDstUnreachable::NetUnreachable,
            SmoltcpDstUnreachable::HostUnreachable => IcmpDstUnreachable::HostUnreachable,
            SmoltcpDstUnreachable::ProtoUnreachable => IcmpDstUnreachable::ProtoUnreachable,
            SmoltcpDstUnreachable::PortUnreachable => IcmpDstUnreachable::PortUnreachable,
            SmoltcpDstUnreachable::FragRequired => IcmpDstUnreachable::FragRequired,
            SmoltcpDstUnreachable::SrcRouteFailed => IcmpDstUnreachable::SrcRouteFailed,
            SmoltcpDstUnreachable::DstNetUnknown => IcmpDstUnreachable::DstNetUnknown,
            SmoltcpDstUnreachable::DstHostUnknown => IcmpDstUnreachable::DstHostUnknown,
            SmoltcpDstUnreachable::SrcHostIsolated => IcmpDstUnreachable::SrcHostIsolated,
            SmoltcpDstUnreachable::NetProhibited => IcmpDstUnreachable::NetProhibited,
            SmoltcpDstUnreachable::HostProhibited => IcmpDstUnreachable::HostProhibited,
            SmoltcpDstUnreachable::NetUnreachToS => IcmpDstUnreachable::NetUnreachToS,
            SmoltcpDstUnreachable::HostUnreachToS => IcmpDstUnreachable::HostUnreachToS,
            SmoltcpDstUnreachable::CommProhibited => IcmpDstUnreachable::CommProhibited,
            SmoltcpDstUnreachable::HostPrecedViol => IcmpDstUnreachable::HostPrecedViol,
            SmoltcpDstUnreachable::PrecedCutoff => IcmpDstUnreachable::PrecedCutoff,
            SmoltcpDstUnreachable::Unknown(val) => IcmpDstUnreachable::Unknown(val),
        }
    }
}

impl From<IcmpDstUnreachable> for SmoltcpDstUnreachable {
    fn from(r: IcmpDstUnreachable) -> Self {
        match r {
            IcmpDstUnreachable::NetUnreachable => SmoltcpDstUnreachable::NetUnreachable,
            IcmpDstUnreachable::HostUnreachable => SmoltcpDstUnreachable::HostUnreachable,
            IcmpDstUnreachable::ProtoUnreachable => SmoltcpDstUnreachable::ProtoUnreachable,
            IcmpDstUnreachable::PortUnreachable => SmoltcpDstUnreachable::PortUnreachable,
            IcmpDstUnreachable::FragRequired => SmoltcpDstUnreachable::FragRequired,
            IcmpDstUnreachable::SrcRouteFailed => SmoltcpDstUnreachable::SrcRouteFailed,
            IcmpDstUnreachable::DstNetUnknown => SmoltcpDstUnreachable::DstNetUnknown,
            IcmpDstUnreachable::DstHostUnknown => SmoltcpDstUnreachable::DstHostUnknown,
            IcmpDstUnreachable::SrcHostIsolated => SmoltcpDstUnreachable::SrcHostIsolated,
            IcmpDstUnreachable::NetProhibited => SmoltcpDstUnreachable::NetProhibited,
            IcmpDstUnreachable::HostProhibited => SmoltcpDstUnreachable::HostProhibited,
            IcmpDstUnreachable::NetUnreachToS => SmoltcpDstUnreachable::NetUnreachToS,
            IcmpDstUnreachable::HostUnreachToS => SmoltcpDstUnreachable::HostUnreachToS,
            IcmpDstUnreachable::CommProhibited => SmoltcpDstUnreachable::CommProhibited,
            IcmpDstUnreachable::HostPrecedViol => SmoltcpDstUnreachable::HostPrecedViol,
            IcmpDstUnreachable::PrecedCutoff => SmoltcpDstUnreachable::PrecedCutoff,
            IcmpDstUnreachable::Unknown(val) => SmoltcpDstUnreachable::Unknown(val),
        }
    }
}

/// Time exceeded reasons for ICMPv4.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IcmpTimeExceeded {
    /// TTL expired in transit.
    TtlExpired,
    /// Fragment reassembly time exceeded.
    FragExpired,
    /// Unknown reason.
    Unknown(u8),
}

impl From<SmoltcpTimeExceeded> for IcmpTimeExceeded {
    fn from(r: SmoltcpTimeExceeded) -> Self {
        match r {
            SmoltcpTimeExceeded::TtlExpired => IcmpTimeExceeded::TtlExpired,
            SmoltcpTimeExceeded::FragExpired => IcmpTimeExceeded::FragExpired,
            SmoltcpTimeExceeded::Unknown(val) => IcmpTimeExceeded::Unknown(val),
        }
    }
}

impl From<IcmpTimeExceeded> for SmoltcpTimeExceeded {
    fn from(r: IcmpTimeExceeded) -> Self {
        match r {
            IcmpTimeExceeded::TtlExpired => SmoltcpTimeExceeded::TtlExpired,
            IcmpTimeExceeded::FragExpired => SmoltcpTimeExceeded::FragExpired,
            IcmpTimeExceeded::Unknown(val) => SmoltcpTimeExceeded::Unknown(val),
        }
    }
}

/// A read/write wrapper around an ICMPv4 packet buffer.
#[derive(Debug, Clone)]
pub struct IcmpPacket<T: AsRef<[u8]>> {
    inner: SmoltcpIcmpv4Packet<T>,
}

impl<T: AsRef<[u8]>> IcmpPacket<T> {
    /// Parse an ICMPv4 packet, verifying the buffer length.
    pub fn new_checked(buffer: T) -> Result<Self, IcmpError> {
        let packet =
            SmoltcpIcmpv4Packet::new_checked(buffer).map_err(|_| IcmpError::PacketTooShort)?;
        Ok(Self { inner: packet })
    }

    /// Construct an ICMPv4 packet wrapper without verifying the length.
    pub fn new_unchecked(buffer: T) -> Self {
        Self {
            inner: SmoltcpIcmpv4Packet::new_unchecked(buffer),
        }
    }

    /// Consume the packet, returning the underlying buffer.
    pub fn into_inner(self) -> T {
        self.inner.into_inner()
    }

    /// Return the message type field.
    pub fn msg_type(&self) -> IcmpMessage {
        self.inner.msg_type().into()
    }

    /// Return the message code field.
    pub fn msg_code(&self) -> u8 {
        self.inner.msg_code()
    }

    /// Return the checksum field.
    pub fn checksum(&self) -> u16 {
        self.inner.checksum()
    }

    /// Return the identifier field (for echo request and reply packets).
    pub fn echo_ident(&self) -> u16 {
        self.inner.echo_ident()
    }

    /// Return the sequence number field (for echo request and reply packets).
    pub fn echo_seq_no(&self) -> u16 {
        self.inner.echo_seq_no()
    }

    /// Return the header length (depends on message type).
    pub fn header_len(&self) -> usize {
        self.inner.header_len()
    }

    /// Validate the header checksum.
    pub fn verify_checksum(&self) -> bool {
        self.inner.verify_checksum()
    }

    /// Return a reference to the underlying packet buffer.
    pub fn inner(&self) -> &SmoltcpIcmpv4Packet<T> {
        &self.inner
    }
}

impl<'a, T: AsRef<[u8]> + ?Sized> IcmpPacket<&'a T> {
    /// Return a pointer to the type-specific data (payload after the ICMP header).
    pub fn data(&self) -> &'a [u8] {
        self.inner.data()
    }
}

impl<T: AsRef<[u8]> + AsMut<[u8]>> IcmpPacket<T> {
    /// Set the message type field.
    pub fn set_msg_type(&mut self, value: IcmpMessage) {
        self.inner.set_msg_type(value.into());
    }

    /// Set the message code field.
    pub fn set_msg_code(&mut self, value: u8) {
        self.inner.set_msg_code(value);
    }

    /// Set the checksum field.
    pub fn set_checksum(&mut self, value: u16) {
        self.inner.set_checksum(value);
    }

    /// Set the identifier field (for echo request and reply packets).
    pub fn set_echo_ident(&mut self, value: u16) {
        self.inner.set_echo_ident(value);
    }

    /// Set the sequence number field (for echo request and reply packets).
    pub fn set_echo_seq_no(&mut self, value: u16) {
        self.inner.set_echo_seq_no(value);
    }

    /// Compute and fill the header checksum.
    pub fn fill_checksum(&mut self) {
        self.inner.fill_checksum();
    }
}

impl<'a, T: AsRef<[u8]> + AsMut<[u8]> + ?Sized> IcmpPacket<&'a mut T> {
    /// Return a mutable pointer to the type-specific data.
    pub fn data_mut(&mut self) -> &mut [u8] {
        self.inner.data_mut()
    }
}

impl<T: AsRef<[u8]>> AsRef<[u8]> for IcmpPacket<T> {
    fn as_ref(&self) -> &[u8] {
        self.inner.as_ref()
    }
}

/// A high-level representation of an ICMPv4 packet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IcmpRepr<'a> {
    /// Echo request (ping).
    EchoRequest {
        ident: u16,
        seq_no: u16,
        data: &'a [u8],
    },
    /// Echo reply (ping response).
    EchoReply {
        ident: u16,
        seq_no: u16,
        data: &'a [u8],
    },
    /// Destination unreachable.
    DstUnreachable {
        reason: IcmpDstUnreachable,
        header: Ipv4Repr,
        data: &'a [u8],
    },
    /// Time exceeded.
    TimeExceeded {
        reason: IcmpTimeExceeded,
        header: Ipv4Repr,
        data: &'a [u8],
    },
}

impl<'a> IcmpRepr<'a> {
    /// Parse an ICMPv4 packet and return a high-level representation.
    pub fn parse<T>(
        packet: &IcmpPacket<&'a T>,
        checksum_caps: &ChecksumCapabilities,
    ) -> Result<Self, IcmpError>
    where
        T: AsRef<[u8]> + ?Sized,
    {
        let repr = SmoltcpIcmpv4Repr::parse(&packet.inner, checksum_caps)
            .map_err(|_| IcmpError::Unsupported)?;
        Ok(match repr {
            SmoltcpIcmpv4Repr::EchoRequest {
                ident,
                seq_no,
                data,
            } => IcmpRepr::EchoRequest {
                ident,
                seq_no,
                data,
            },
            SmoltcpIcmpv4Repr::EchoReply {
                ident,
                seq_no,
                data,
            } => IcmpRepr::EchoReply {
                ident,
                seq_no,
                data,
            },
            SmoltcpIcmpv4Repr::DstUnreachable {
                reason,
                header,
                data,
            } => IcmpRepr::DstUnreachable {
                reason: reason.into(),
                header,
                data,
            },
            SmoltcpIcmpv4Repr::TimeExceeded {
                reason,
                header,
                data,
            } => IcmpRepr::TimeExceeded {
                reason: reason.into(),
                header,
                data,
            },
            _ => return Err(IcmpError::Unsupported),
        })
    }

    /// Return the length of the buffer needed to emit this representation.
    pub fn buffer_len(&self) -> usize {
        match self {
            IcmpRepr::EchoRequest { data, .. } | IcmpRepr::EchoReply { data, .. } => 8 + data.len(),
            IcmpRepr::DstUnreachable { header, data, .. }
            | IcmpRepr::TimeExceeded { header, data, .. } => 8 + header.buffer_len() + data.len(),
        }
    }

    /// Emit this high-level representation into an ICMPv4 packet.
    pub fn emit<T>(&self, packet: &mut IcmpPacket<&mut T>, checksum_caps: &ChecksumCapabilities)
    where
        T: AsRef<[u8]> + AsMut<[u8]> + ?Sized,
    {
        match self {
            IcmpRepr::EchoRequest {
                ident,
                seq_no,
                data,
            } => {
                let repr = SmoltcpIcmpv4Repr::EchoRequest {
                    ident: *ident,
                    seq_no: *seq_no,
                    data,
                };
                repr.emit(&mut packet.inner, checksum_caps);
            }
            IcmpRepr::EchoReply {
                ident,
                seq_no,
                data,
            } => {
                let repr = SmoltcpIcmpv4Repr::EchoReply {
                    ident: *ident,
                    seq_no: *seq_no,
                    data,
                };
                repr.emit(&mut packet.inner, checksum_caps);
            }
            IcmpRepr::DstUnreachable {
                reason,
                header,
                data,
                ..
            } => {
                let reason_smoltcp: SmoltcpDstUnreachable = (*reason).into();
                let repr = SmoltcpIcmpv4Repr::DstUnreachable {
                    reason: reason_smoltcp,
                    header: *header,
                    data,
                };
                repr.emit(&mut packet.inner, checksum_caps);
            }
            IcmpRepr::TimeExceeded {
                reason,
                header,
                data,
                ..
            } => {
                let reason_smoltcp: SmoltcpTimeExceeded = (*reason).into();
                let repr = SmoltcpIcmpv4Repr::TimeExceeded {
                    reason: reason_smoltcp,
                    header: *header,
                    data,
                };
                repr.emit(&mut packet.inner, checksum_caps);
            }
        }
    }
}

impl fmt::Display for IcmpRepr<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IcmpRepr::EchoRequest {
                ident,
                seq_no,
                data,
                ..
            } => {
                write!(
                    f,
                    "ICMPv4 echo request id={} seq={} len={}",
                    ident,
                    seq_no,
                    data.len()
                )
            }
            IcmpRepr::EchoReply {
                ident,
                seq_no,
                data,
                ..
            } => {
                write!(
                    f,
                    "ICMPv4 echo reply id={} seq={} len={}",
                    ident,
                    seq_no,
                    data.len()
                )
            }
            IcmpRepr::DstUnreachable { reason, .. } => {
                write!(f, "ICMPv4 destination unreachable ({:?})", reason)
            }
            IcmpRepr::TimeExceeded { reason, .. } => {
                write!(f, "ICMPv4 time exceeded ({:?})", reason)
            }
        }
    }
}

#[cfg(ktest)]
mod tests {
    use super::*;
    use ostd::prelude::ktest;

    static ECHO_PACKET_BYTES: [u8; 12] = [
        0x08, 0x00, 0x8e, 0xfe, 0x12, 0x34, 0xab, 0xcd, 0xaa, 0x00, 0x00, 0xff,
    ];

    static ECHO_DATA_BYTES: [u8; 4] = [0xaa, 0x00, 0x00, 0xff];

    #[ktest]
    fn test_icmp_echo_deconstruct() {
        let packet = IcmpPacket::new_unchecked(&ECHO_PACKET_BYTES[..]);
        assert_eq!(packet.msg_type(), IcmpMessage::EchoRequest);
        assert_eq!(packet.msg_code(), 0);
        assert_eq!(packet.checksum(), 0x8efe);
        assert_eq!(packet.echo_ident(), 0x1234);
        assert_eq!(packet.echo_seq_no(), 0xabcd);
        assert_eq!(packet.data(), &ECHO_DATA_BYTES[..]);
        assert!(packet.verify_checksum());
    }

    #[ktest]
    fn test_icmp_echo_construct() {
        let mut bytes = [0xa5u8; 12];
        let mut packet = IcmpPacket::new_unchecked(&mut bytes[..]);
        packet.set_msg_type(IcmpMessage::EchoRequest);
        packet.set_msg_code(0);
        packet.set_echo_ident(0x1234);
        packet.set_echo_seq_no(0xabcd);
        packet.data_mut().copy_from_slice(&ECHO_DATA_BYTES[..]);
        packet.fill_checksum();
        assert_eq!(*packet.into_inner(), ECHO_PACKET_BYTES);
    }

    #[ktest]
    fn test_icmp_echo_repr_parse() {
        let packet = IcmpPacket::new_unchecked(&ECHO_PACKET_BYTES[..]);
        let repr = IcmpRepr::parse(&packet, &ChecksumCapabilities::default()).unwrap();
        assert_eq!(
            repr,
            IcmpRepr::EchoRequest {
                ident: 0x1234,
                seq_no: 0xabcd,
                data: &ECHO_DATA_BYTES,
            }
        );
    }

    #[ktest]
    fn test_icmp_echo_repr_emit() {
        let repr = IcmpRepr::EchoRequest {
            ident: 0x1234,
            seq_no: 0xabcd,
            data: &ECHO_DATA_BYTES,
        };
        let mut bytes = [0xa5u8; 12];
        let mut packet = IcmpPacket::new_unchecked(&mut bytes[..]);
        repr.emit(&mut packet, &ChecksumCapabilities::default());
        assert_eq!(*packet.into_inner(), ECHO_PACKET_BYTES);
    }

    #[ktest]
    fn test_icmp_message_conversion() {
        assert_eq!(
            IcmpMessage::from(SmoltcpMessage::EchoRequest),
            IcmpMessage::EchoRequest
        );
        assert_eq!(
            IcmpMessage::from(SmoltcpMessage::EchoReply),
            IcmpMessage::EchoReply
        );
        assert_eq!(
            IcmpMessage::from(SmoltcpMessage::DstUnreachable),
            IcmpMessage::DstUnreachable
        );
        assert_eq!(
            IcmpMessage::from(SmoltcpMessage::TimeExceeded),
            IcmpMessage::TimeExceeded
        );
    }
}
