// SPDX-License-Identifier: GPL-2.0

//! Virtio network device wire protocol (`virtio 1.2` §5.1).
//!
//! Defines PCI device IDs, virtqueue indices, feature bits, `virtio_net_config`
//! layout, and the `virtio_net_hdr` packet header used on `receiveq` and
//! `transmitq`.

use core::mem::size_of;
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

use crate::drivers::bus::virtio::VirtioTransport;
use crate::uapi::errno::Result;

/// `virtio-net` modern PCI device ID (`0x1040 + VIRTIO_ID_NET`, where `VIRTIO_ID_NET = 1`).
pub const PCI_DEVICE_ID_VIRTIO_NET: u16 = 0x1041;

/// Primary receive virtqueue index (`receiveq1`).
pub const RX_QUEUE: usize = 0;
/// Primary transmit virtqueue index (`transmitq1`).
pub const TX_QUEUE: usize = 1;
/// Number of queues activated by the driver (`receiveq1` + `transmitq1`).
pub const QUEUE_COUNT: usize = 2;

/// Default Ethernet Maximum Transmission Unit (MTU) in bytes.
pub const DEFAULT_ETH_MTU: usize = 1500;
/// Minimum IPv4 Ethernet MTU in bytes.
pub const MIN_ETH_MTU: usize = 68;
/// Fallback locally-administered MAC address when `FEATURE_MAC` is not offered.
pub const DEFAULT_MAC_ADDRESS: [u8; 6] = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56];

// ----------------------------------------------------------------------------
// Feature bits (`virtio 1.2` §5.1.3)
// ----------------------------------------------------------------------------

/// `VIRTIO_NET_F_CSUM`: Device handles packets with partial checksum.
pub const FEATURE_CSUM: u64 = 1 << 0;
/// `VIRTIO_NET_F_GUEST_CSUM`: Driver handles packets with partial checksum.
pub const FEATURE_GUEST_CSUM: u64 = 1 << 1;
/// `VIRTIO_NET_F_MTU`: Initial MTU advice is exported in configuration space.
pub const FEATURE_MTU: u64 = 1 << 3;
/// `VIRTIO_NET_F_MAC`: Device has given MAC address in configuration space.
pub const FEATURE_MAC: u64 = 1 << 5;
/// `VIRTIO_NET_F_MRG_RXBUF`: Driver can merge receive buffers.
pub const FEATURE_MRG_RXBUF: u64 = 1 << 15;
/// `VIRTIO_NET_F_STATUS`: Configuration `status` field is available.
pub const FEATURE_STATUS: u64 = 1 << 16;
/// `VIRTIO_NET_F_CTRL_VQ`: Control channel is available.
pub const FEATURE_CTRL_VQ: u64 = 1 << 17;

// ----------------------------------------------------------------------------
// Device configuration layout (`struct virtio_net_config`, `virtio 1.2` §5.1.4)
// ----------------------------------------------------------------------------

/// Offset of `mac[6]` (`[u8; 6]`).
pub const CONFIG_MAC_OFFSET: usize = 0;
/// Offset of `status` (`u16`).
pub const CONFIG_STATUS_OFFSET: usize = 6;
/// Offset of `max_virtqueue_pairs` (`u16`).
pub const CONFIG_MAX_VQ_PAIRS_OFFSET: usize = 8;
/// Offset of `mtu` (`u16`).
pub const CONFIG_MTU_OFFSET: usize = 10;

/// `VIRTIO_NET_S_LINK_UP`: Link is up.
pub const NET_STATUS_LINK_UP: u16 = 1 << 0;
/// `VIRTIO_NET_S_ANNOUNCE`: Announcement is needed.
pub const NET_STATUS_ANNOUNCE: u16 = 1 << 1;

/// Snapshot of the relevant fields from `struct virtio_net_config`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NetConfig {
    /// Station MAC address.
    pub mac: [u8; 6],
    /// Whether the link is reported up.
    pub link_up: bool,
    /// Maximum transmission unit in bytes.
    pub mtu: usize,
}

impl NetConfig {
    /// Reads the device-specific configuration space from `transport` according
    /// to the negotiated feature bits.
    pub fn read(transport: &VirtioTransport) -> Result<Self> {
        let features = transport.negotiated_features();

        let mut mac = DEFAULT_MAC_ADDRESS;
        if features & FEATURE_MAC != 0 {
            let mut valid = true;
            let mut hw_mac = [0u8; 6];
            for (i, byte) in hw_mac.iter_mut().enumerate() {
                match transport.device_read8(CONFIG_MAC_OFFSET + i) {
                    Ok(val) => *byte = val,
                    Err(_) => {
                        valid = false;
                        break;
                    }
                }
            }
            if valid && hw_mac != [0u8; 6] {
                mac = hw_mac;
            }
        }

        let link_up = if features & FEATURE_STATUS != 0 {
            transport
                .device_read16(CONFIG_STATUS_OFFSET)
                .map(|s| (s & NET_STATUS_LINK_UP) != 0)
                .unwrap_or(true)
        } else {
            true
        };

        let mtu = if features & FEATURE_MTU != 0 {
            let raw = transport
                .device_read16(CONFIG_MTU_OFFSET)
                .unwrap_or(DEFAULT_ETH_MTU as u16) as usize;
            if (MIN_ETH_MTU..=DEFAULT_ETH_MTU).contains(&raw) {
                raw
            } else {
                DEFAULT_ETH_MTU
            }
        } else {
            DEFAULT_ETH_MTU
        };

        Ok(Self { mac, link_up, mtu })
    }
}

// ----------------------------------------------------------------------------
// Packet header (`struct virtio_net_hdr`, `virtio 1.2` §5.1.6.1)
// ----------------------------------------------------------------------------

/// Packet needs checksum (`VIRTIO_NET_HDR_F_NEEDS_CSUM`).
pub const HDR_F_NEEDS_CSUM: u8 = 1;
/// Checksum is valid (`VIRTIO_NET_HDR_F_DATA_VALID`).
pub const HDR_F_DATA_VALID: u8 = 2;
/// Reports number of coalesced TCP segments (`VIRTIO_NET_HDR_F_RSC_INFO`).
pub const HDR_F_RSC_INFO: u8 = 4;

/// Not a GSO packet (`VIRTIO_NET_HDR_GSO_NONE`).
pub const GSO_NONE: u8 = 0;
/// TCPv4 GSO (`VIRTIO_NET_HDR_GSO_TCPV4`).
pub const GSO_TCPV4: u8 = 1;
/// UDP GSO (`VIRTIO_NET_HDR_GSO_UDP`).
pub const GSO_UDP: u8 = 3;
/// TCPv6 GSO (`VIRTIO_NET_HDR_GSO_TCPV6`).
pub const GSO_TCPV6: u8 = 4;
/// TCP has ECN set (`VIRTIO_NET_HDR_GSO_ECN`).
pub const GSO_ECN: u8 = 0x80;

/// Modern 12-byte `struct virtio_net_hdr` prepended to every RX and TX packet
/// when `VIRTIO_F_VERSION_1` is negotiated (`virtio 1.2` §5.1.6.1).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, FromBytes, IntoBytes, KnownLayout, Immutable)]
pub struct VirtioNetHdr {
    /// Header flags (`HDR_F_*`).
    pub flags: u8,
    /// Generic segmentation offload type (`GSO_*`).
    pub gso_type: u8,
    /// Ethernet + IP + tcp/udp header length.
    pub hdr_len: u16,
    /// Bytes to append to `hdr_len` per segment.
    pub gso_size: u16,
    /// Position where checksum starts.
    pub csum_start: u16,
    /// Offset after `csum_start` to store checksum.
    pub csum_offset: u16,
    /// Number of merged RX buffers (0 on TX; set by device on RX).
    pub num_buffers: u16,
}

impl VirtioNetHdr {
    /// Constructs a plain (no checksum offload, no GSO) packet header for TX.
    pub const fn new_tx() -> Self {
        Self {
            flags: 0,
            gso_type: GSO_NONE,
            hdr_len: 0,
            gso_size: 0,
            csum_start: 0,
            csum_offset: 0,
            num_buffers: 0,
        }
    }
}

const _: () = {
    assert!(size_of::<VirtioNetHdr>() == 12);
};
