// SPDX-License-Identifier: GPL-2.0

//! Intel e1000 Hardware Descriptors and Memory Structures.

use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

pub const NUM_RX_DESCRIPTORS: usize = 128;
pub const NUM_TX_DESCRIPTORS: usize = 128;
pub const RX_BUFFER_SIZE: usize = 2048;
pub const TX_BUFFER_SIZE: usize = 2048;

/// 16-byte Receive Descriptor (Legacy format).
#[repr(C)]
#[derive(Clone, Copy, Default, Debug, FromBytes, IntoBytes, KnownLayout, Immutable)]
pub struct RxDesc {
    pub addr: u64,
    pub length: u16,
    pub checksum: u16,
    pub status: u8,
    pub errors: u8,
    pub special: u16,
}

impl RxDesc {
    pub fn to_bytes(self) -> [u8; 16] {
        zerocopy::transmute!(self)
    }

    pub fn from_bytes(b: [u8; 16]) -> Self {
        zerocopy::transmute!(b)
    }
}

/// 16-byte Transmit Descriptor (Legacy format).
#[repr(C)]
#[derive(Clone, Copy, Default, Debug, FromBytes, IntoBytes, KnownLayout, Immutable)]
pub struct TxDesc {
    pub addr: u64,
    pub length: u16,
    pub cso: u8,
    pub cmd: u8,
    pub status: u8,
    pub css: u8,
    pub special: u16,
}

impl TxDesc {
    pub fn to_bytes(self) -> [u8; 16] {
        zerocopy::transmute!(self)
    }

    pub fn from_bytes(b: [u8; 16]) -> Self {
        zerocopy::transmute!(b)
    }
}
