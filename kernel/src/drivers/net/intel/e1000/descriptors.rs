//! Intel 8254x (e1000) Hardware Descriptor Structures
//!
//! Defines 16-byte Receive and Transmit legacy descriptors used for DMA operations.

// ===== Receive Descriptor Status & Error Bits =====

pub const RXD_STAT_DD: u8 = 1 << 0; // Descriptor Done
pub const RXD_STAT_EOP: u8 = 1 << 1; // End of Packet
pub const RXD_STAT_IXSM: u8 = 1 << 2; // Ignore Checksum Indication
pub const RXD_STAT_VP: u8 = 1 << 3; // Packet is 802.1Q (VLAN)
pub const RXD_STAT_TCPCS: u8 = 1 << 5; // TCP Checksum Calculated on Packet
pub const RXD_STAT_IPCS: u8 = 1 << 6; // IP Checksum Calculated on Packet
pub const RXD_STAT_PIF: u8 = 1 << 7; // Passed in-exact filter

pub const RXD_ERR_CE: u8 = 1 << 0; // CRC Error
pub const RXD_ERR_SE: u8 = 1 << 1; // Symbol Error
pub const RXD_ERR_SEQ: u8 = 1 << 2; // Sequence Error
pub const RXD_ERR_CXE: u8 = 1 << 4; // Carrier Extension Error
pub const RXD_ERR_TCPE: u8 = 1 << 5; // TCP/UDP Checksum Error
pub const RXD_ERR_IPE: u8 = 1 << 6; // IP Checksum Error
pub const RXD_ERR_RXE: u8 = 1 << 7; // RX Data Error

// ===== Transmit Descriptor Command & Status Bits =====

pub const TXD_CMD_EOP: u8 = 1 << 0; // End of Packet
pub const TXD_CMD_IFCS: u8 = 1 << 1; // Insert FCS / CRC
pub const TXD_CMD_IC: u8 = 1 << 2; // Insert Checksum
pub const TXD_CMD_RS: u8 = 1 << 3; // Report Status
pub const TXD_CMD_RPS: u8 = 1 << 4; // Report Packet Sent
pub const TXD_CMD_DEXT: u8 = 1 << 5; // Descriptor Extension (0 = legacy)
pub const TXD_CMD_VLE: u8 = 1 << 6; // VLAN Packet Enable
pub const TXD_CMD_IDE: u8 = 1 << 7; // Interrupt Delay Enable

pub const TXD_STAT_DD: u8 = 1 << 0; // Descriptor Done
pub const TXD_STAT_EC: u8 = 1 << 1; // Excess Collisions
pub const TXD_STAT_LC: u8 = 1 << 2; // Late Collision
pub const TXD_STAT_TU: u8 = 1 << 3; // Transmit Underrun

/// Legacy 16-byte Receive Descriptor layout.
#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default)]
pub struct RxDesc {
    /// Physical address of the receive data buffer.
    pub address: u64,
    /// Length of received packet data in bytes (valid when DD is set).
    pub length: u16,
    /// Hardware packet checksum.
    pub checksum: u16,
    /// Descriptor status flags.
    pub status: u8,
    /// Descriptor error flags.
    pub errors: u8,
    /// Special field (VLAN tag info).
    pub special: u16,
}

/// Legacy 16-byte Transmit Descriptor layout.
#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default)]
pub struct TxDesc {
    /// Physical address of the transmit data buffer.
    pub address: u64,
    /// Length of packet buffer to transmit in bytes.
    pub length: u16,
    /// Checksum Offset.
    pub cso: u8,
    /// Command flags (EOP, IFCS, RS, etc.).
    pub cmd: u8,
    /// Status flags (DD, etc.).
    pub status: u8,
    /// Checksum Start Field.
    pub css: u8,
    /// Special field (VLAN info).
    pub special: u16,
}
