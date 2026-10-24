// SPDX-License-Identifier: GPL-2.0

//! VirtIO Input Protocol Definitions (`virtio 1.1` §5.8).

/// PCI Device ID for modern VirtIO Input devices (`0x1040 + 18`).
pub const PCI_DEVICE_ID_VIRTIO_INPUT: u16 = 0x1052;

/// VirtQueue indices for VirtIO Input.
pub const EVENT_QUEUE: usize = 0;
pub const STATUS_QUEUE: usize = 1;
pub const QUEUE_COUNT: usize = 2;

/// VirtIO Input configuration selection registers (`virtio 1.1` §5.8.4).
pub const VIRTIO_INPUT_CFG_UNSET: u8 = 0x00;
pub const VIRTIO_INPUT_CFG_ID_NAME: u8 = 0x01;
pub const VIRTIO_INPUT_CFG_ID_SERIAL: u8 = 0x02;
pub const VIRTIO_INPUT_CFG_ID_DEVIDS: u8 = 0x03;
pub const VIRTIO_INPUT_CFG_PROP_BITS: u8 = 0x10;
pub const VIRTIO_INPUT_CFG_EV_BITS: u8 = 0x11;
pub const VIRTIO_INPUT_CFG_ABS_INFO: u8 = 0x12;

/// Maximum number of event descriptors pre-populated in `eventq`.
pub const MAX_EVENT_BUFFERS: usize = 64;

/// A single event transmitted across `eventq` or `statusq` (`virtio 1.1` §5.8.6.1).
#[repr(C)]
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    zerocopy::FromBytes,
    zerocopy::IntoBytes,
    zerocopy::KnownLayout,
    zerocopy::Immutable,
)]
pub struct VirtioInputEvent {
    pub type_: u16,
    pub code: u16,
    pub value: u32,
}
