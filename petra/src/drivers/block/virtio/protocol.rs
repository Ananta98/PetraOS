// SPDX-License-Identifier: GPL-2.0

//! Virtio block device wire protocol (`virtio 1.2` §5.2).
//!
//! Defines PCI device IDs, feature bits, device-specific configuration space
//! layout, request headers, command opcodes, and status byte values for
//! `virtio-blk`.

use core::mem::size_of;
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

use crate::drivers::bus::virtio::VirtioTransport;
use crate::uapi::errno::{Errno, Result};

/// `virtio-blk` modern PCI device ID (`0x1040 + VIRTIO_ID_BLOCK`, where `VIRTIO_ID_BLOCK = 2`).
pub const PCI_DEVICE_ID_VIRTIO_BLK: u16 = 0x1042;

/// Primary request virtqueue index (`requestq1`).
pub const REQUEST_QUEUE: usize = 0;

/// Standard 512-byte sector unit used by the virtio block protocol.
pub const VIRTIO_BLK_SECTOR_SIZE: usize = 512;

// ----------------------------------------------------------------------------
// Feature bits (`virtio 1.2` §5.2.3)
// ----------------------------------------------------------------------------

/// `VIRTIO_BLK_F_SIZE_MAX`: maximum size of any single segment is in `size_max`.
pub const FEATURE_SIZE_MAX: u64 = 1 << 1;
/// `VIRTIO_BLK_F_SEG_MAX`: maximum number of segments in a request is in `seg_max`.
pub const FEATURE_SEG_MAX: u64 = 1 << 2;
/// `VIRTIO_BLK_F_GEOMETRY`: disk-style geometry specified in `geometry`.
pub const FEATURE_GEOMETRY: u64 = 1 << 4;
/// `VIRTIO_BLK_F_RO`: device is read-only.
pub const FEATURE_RO: u64 = 1 << 5;
/// `VIRTIO_BLK_F_BLK_SIZE`: block size of disk is in `blk_size`.
pub const FEATURE_BLK_SIZE: u64 = 1 << 6;
/// `VIRTIO_BLK_F_FLUSH`: cache flush command support.
pub const FEATURE_FLUSH: u64 = 1 << 9;
/// `VIRTIO_BLK_F_TOPOLOGY`: device exports information on optimal I/O alignment.
pub const FEATURE_TOPOLOGY: u64 = 1 << 10;
/// `VIRTIO_BLK_F_CONFIG_WCE`: device can toggle its cache between writeback and writethrough modes.
pub const FEATURE_CONFIG_WCE: u64 = 1 << 11;
/// `VIRTIO_BLK_F_MQ`: device supports multiqueue (`num_queues`).
pub const FEATURE_MQ: u64 = 1 << 12;
/// `VIRTIO_BLK_F_DISCARD`: device can support discard command.
pub const FEATURE_DISCARD: u64 = 1 << 13;
/// `VIRTIO_BLK_F_WRITE_ZEROES`: device can support write zeroes command.
pub const FEATURE_WRITE_ZEROES: u64 = 1 << 14;

// ----------------------------------------------------------------------------
// Device configuration layout (`struct virtio_blk_config`, `virtio 1.2` §5.2.4)
// ----------------------------------------------------------------------------

/// Offset of `capacity` (low 32 bits, in 512-byte sectors).
pub const CONFIG_CAPACITY_LO: usize = 0;
/// Offset of `capacity` (high 32 bits, in 512-byte sectors).
pub const CONFIG_CAPACITY_HI: usize = 4;
/// Offset of `size_max` (`u32`).
pub const CONFIG_SIZE_MAX: usize = 8;
/// Offset of `seg_max` (`u32`).
pub const CONFIG_SEG_MAX: usize = 12;
/// Offset of `blk_size` (`u32`).
pub const CONFIG_BLK_SIZE: usize = 20;

/// Snapshot of the relevant fields from `struct virtio_blk_config`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlkConfig {
    /// Total device capacity in 512-byte sectors.
    pub capacity_sectors: u64,
    /// Logical block size in bytes (defaults to 512 if `FEATURE_BLK_SIZE` is not set).
    pub blk_size: usize,
    /// Maximum size of any single segment, if `FEATURE_SIZE_MAX` was negotiated.
    pub size_max: Option<u32>,
    /// Maximum number of segments in a request, if `FEATURE_SEG_MAX` was negotiated.
    pub seg_max: Option<u32>,
    /// Whether the device was negotiated as read-only (`FEATURE_RO`).
    pub read_only: bool,
    /// Whether `VIRTIO_BLK_T_FLUSH` is supported (`FEATURE_FLUSH`).
    pub flush_supported: bool,
}

impl BlkConfig {
    /// Reads the device-specific configuration space from `transport` according
    /// to the negotiated feature bits.
    pub fn read(transport: &VirtioTransport) -> Result<Self> {
        let features = transport.negotiated_features();

        let cap_lo = transport.device_read32(CONFIG_CAPACITY_LO)? as u64;
        let cap_hi = transport.device_read32(CONFIG_CAPACITY_HI)? as u64;
        let capacity_sectors = cap_lo | (cap_hi << 32);

        let blk_size = if features & FEATURE_BLK_SIZE != 0 {
            let raw = transport
                .device_read32(CONFIG_BLK_SIZE)
                .unwrap_or(VIRTIO_BLK_SECTOR_SIZE as u32) as usize;
            if raw >= VIRTIO_BLK_SECTOR_SIZE && raw.is_power_of_two() {
                raw
            } else {
                VIRTIO_BLK_SECTOR_SIZE
            }
        } else {
            VIRTIO_BLK_SECTOR_SIZE
        };

        let size_max = if features & FEATURE_SIZE_MAX != 0 {
            transport.device_read32(CONFIG_SIZE_MAX).ok()
        } else {
            None
        };

        let seg_max = if features & FEATURE_SEG_MAX != 0 {
            transport.device_read32(CONFIG_SEG_MAX).ok()
        } else {
            None
        };

        Ok(Self {
            capacity_sectors,
            blk_size,
            size_max,
            seg_max,
            read_only: features & FEATURE_RO != 0,
            flush_supported: features & FEATURE_FLUSH != 0,
        })
    }
}

// ----------------------------------------------------------------------------
// Request types and status codes (`virtio 1.2` §5.2.6)
// ----------------------------------------------------------------------------

/// Read request (`VIRTIO_BLK_T_IN`).
pub const REQ_TYPE_IN: u32 = 0;
/// Write request (`VIRTIO_BLK_T_OUT`).
pub const REQ_TYPE_OUT: u32 = 1;
/// Cache flush request (`VIRTIO_BLK_T_FLUSH`).
pub const REQ_TYPE_FLUSH: u32 = 4;
/// Get device ID string (`VIRTIO_BLK_T_GET_ID`).
pub const REQ_TYPE_GET_ID: u32 = 8;
/// Discard command (`VIRTIO_BLK_T_DISCARD`).
pub const REQ_TYPE_DISCARD: u32 = 11;
/// Write zeroes command (`VIRTIO_BLK_T_WRITE_ZEROES`).
pub const REQ_TYPE_WRITE_ZEROES: u32 = 13;

/// Request succeeded (`VIRTIO_BLK_S_OK`).
pub const STATUS_OK: u8 = 0;
/// Device or driver error (`VIRTIO_BLK_S_IOERR`).
pub const STATUS_IOERR: u8 = 1;
/// Request unsupported by device (`VIRTIO_BLK_S_UNSUPP`).
pub const STATUS_UNSUPP: u8 = 2;
/// Sentinel written by the driver before submission to detect incomplete DMA writes.
pub const STATUS_PENDING: u8 = 0xFF;

/// Maps a `virtio-blk` completion status byte to `Ok(())` or a kernel [`Errno`].
pub fn check_status(status: u8) -> Result<()> {
    match status {
        STATUS_OK => Ok(()),
        STATUS_UNSUPP => Err(Errno::ENOTSUP),
        STATUS_IOERR => Err(Errno::EIO),
        _ => Err(Errno::EIO),
    }
}

// ----------------------------------------------------------------------------
// Wire structures (`virtio 1.2` §5.2.6)
// ----------------------------------------------------------------------------

/// `struct virtio_blk_req_hdr` (16 bytes), prefix of every block request descriptor chain.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, FromBytes, IntoBytes, KnownLayout, Immutable)]
pub struct BlkReqHdr {
    /// Request type (`REQ_TYPE_IN`, `REQ_TYPE_OUT`, `REQ_TYPE_FLUSH`, ...).
    pub req_type: u32,
    /// Reserved (previously `ioprio`), must be 0.
    pub reserved: u32,
    /// Starting offset in 512-byte sectors (must be 0 for `REQ_TYPE_FLUSH`).
    pub sector: u64,
}

impl BlkReqHdr {
    /// Constructs a new request header for `req_type` starting at `sector`.
    pub const fn new(req_type: u32, sector: u64) -> Self {
        Self {
            req_type,
            reserved: 0,
            sector,
        }
    }
}

const _: () = {
    assert!(size_of::<BlkReqHdr>() == 16);
};
