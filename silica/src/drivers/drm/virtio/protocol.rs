// SPDX-License-Identifier: GPL-2.0

//! Virtio GPU 2D and cursor wire protocol (`virtio 1.2` §5.7).
//!
//! Defines feature bits, device configuration registers, pixel formats,
//! control and cursor command structures, and response validation for
//! `virtio-gpu`.

use core::mem::size_of;
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

use crate::drivers::bus::virtio::VirtioTransport;
use crate::uapi::drm;
use crate::uapi::errno::Result;

/// `virtio-gpu` modern PCI device ID (`0x1040 + VIRTIO_ID_GPU`, where `VIRTIO_ID_GPU = 16`).
pub const PCI_DEVICE_ID_VIRTIO_GPU: u16 = 0x1050;

/// Red Hat / Virtio PCI vendor ID.
pub const PCI_VENDOR_ID_VIRTIO: u16 = 0x1AF4;

/// Control virtqueue index.
pub const CONTROL_QUEUE: usize = 0;
/// Cursor virtqueue index.
pub const CURSOR_QUEUE: usize = 1;

// ----------------------------------------------------------------------------
// Feature bits (`virtio 1.2` §5.7.3)
// ----------------------------------------------------------------------------

/// `VIRTIO_GPU_F_VIRGL`: virgl 3D mode is supported.
pub const FEATURE_VIRGL: u64 = 1 << 0;
/// `VIRTIO_GPU_F_EDID`: EDID querying (`VIRTIO_GPU_CMD_GET_EDID`) is supported.
pub const FEATURE_EDID: u64 = 1 << 1;
/// `VIRTIO_GPU_F_RESOURCE_UUID`: resource UUID assignment is supported.
pub const FEATURE_RESOURCE_UUID: u64 = 1 << 2;
/// `VIRTIO_GPU_F_RESOURCE_BLOB`: size-based blob resources are supported.
pub const FEATURE_RESOURCE_BLOB: u64 = 1 << 3;
/// `VIRTIO_GPU_F_CONTEXT_INIT`: multiple context types and synchronization timelines.
pub const FEATURE_CONTEXT_INIT: u64 = 1 << 4;

// ----------------------------------------------------------------------------
// Device configuration layout (`struct virtio_gpu_config`, `virtio 1.2` §5.7.4)
// ----------------------------------------------------------------------------

/// Offset of `events_read` in device-specific configuration space.
pub const CONFIG_EVENTS_READ: usize = 0;
/// Offset of `events_clear` in device-specific configuration space.
pub const CONFIG_EVENTS_CLEAR: usize = 4;
/// Offset of `num_scanouts` in device-specific configuration space.
pub const CONFIG_NUM_SCANOUTS: usize = 8;
/// Offset of `num_capsets` in device-specific configuration space.
pub const CONFIG_NUM_CAPSETS: usize = 12;

/// `VIRTIO_GPU_EVENT_DISPLAY`: display configuration changed; re-query `GET_DISPLAY_INFO`.
pub const EVENT_DISPLAY: u32 = 1 << 0;

/// `VIRTIO_GPU_MAX_SCANOUTS`: maximum scanouts a device can expose.
pub const MAX_SCANOUTS: usize = 16;
/// Maximum width or height accepted for a scanout or 2D resource.
pub const MAX_DIMENSION: u32 = 16384;
/// Standard cursor plane dimensions required by QEMU's `virtio-gpu` (`64x64`).
pub const CURSOR_SIZE: u32 = 64;
/// Maximum EDID payload size in `struct virtio_gpu_resp_edid`.
pub const EDID_MAX_BYTES: usize = 1024;

/// Snapshot of `struct virtio_gpu_config`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GpuConfig {
    pub events_read: u32,
    pub num_scanouts: u32,
    pub num_capsets: u32,
}

impl GpuConfig {
    /// Reads the device-specific configuration registers from `transport`.
    pub fn read(transport: &VirtioTransport) -> Result<Self> {
        let events_read = transport.device_read32(CONFIG_EVENTS_READ).unwrap_or(0);
        let num_scanouts = transport
            .device_read32(CONFIG_NUM_SCANOUTS)
            .unwrap_or(MAX_SCANOUTS as u32)
            .clamp(1, MAX_SCANOUTS as u32);
        let num_capsets = transport.device_read32(CONFIG_NUM_CAPSETS).unwrap_or(0);
        Ok(Self {
            events_read,
            num_scanouts,
            num_capsets,
        })
    }
}

// ----------------------------------------------------------------------------
// Command and response codes (`enum virtio_gpu_ctrl_type`, `virtio 1.2` §5.7.6)
// ----------------------------------------------------------------------------

// 2D commands
pub const CMD_GET_DISPLAY_INFO: u32 = 0x0100;
pub const CMD_RESOURCE_CREATE_2D: u32 = 0x0101;
pub const CMD_RESOURCE_UNREF: u32 = 0x0102;
pub const CMD_SET_SCANOUT: u32 = 0x0103;
pub const CMD_RESOURCE_FLUSH: u32 = 0x0104;
pub const CMD_TRANSFER_TO_HOST_2D: u32 = 0x0105;
pub const CMD_RESOURCE_ATTACH_BACKING: u32 = 0x0106;
pub const CMD_RESOURCE_DETACH_BACKING: u32 = 0x0107;
pub const CMD_GET_CAPSET_INFO: u32 = 0x0108;
pub const CMD_GET_CAPSET: u32 = 0x0109;
pub const CMD_GET_EDID: u32 = 0x010A;

// Cursor commands
pub const CMD_UPDATE_CURSOR: u32 = 0x0300;
pub const CMD_MOVE_CURSOR: u32 = 0x0301;

// Success responses
pub const RESP_OK_NODATA: u32 = 0x1100;
pub const RESP_OK_DISPLAY_INFO: u32 = 0x1101;
pub const RESP_OK_CAPSET_INFO: u32 = 0x1102;
pub const RESP_OK_CAPSET: u32 = 0x1103;
pub const RESP_OK_EDID: u32 = 0x1104;

// Error responses
pub const RESP_ERR_UNSPEC: u32 = 0x1200;
pub const RESP_ERR_OUT_OF_MEMORY: u32 = 0x1201;
pub const RESP_ERR_INVALID_SCANOUT_ID: u32 = 0x1202;
pub const RESP_ERR_INVALID_RESOURCE_ID: u32 = 0x1203;
pub const RESP_ERR_INVALID_CONTEXT_ID: u32 = 0x1204;
pub const RESP_ERR_INVALID_PARAMETER: u32 = 0x1205;

/// `VIRTIO_GPU_FLAG_FENCE`: fence the request and signal completion only when finished.
pub const FLAG_FENCE: u32 = 1 << 0;

/// Maps a device response code to `Ok(())` when it matches `expected_ok`, or
/// translates device error codes into kernel [`Errno`] values.
pub fn check_response_code(code: u32, expected_ok: u32) -> Result<()> {
    if code == expected_ok {
        return Ok(());
    }
    match code {
        RESP_ERR_OUT_OF_MEMORY => crate::return_errno!(ENOMEM, "virtio-gpu out of memory"),
        RESP_ERR_INVALID_SCANOUT_ID
        | RESP_ERR_INVALID_RESOURCE_ID
        | RESP_ERR_INVALID_CONTEXT_ID
        | RESP_ERR_INVALID_PARAMETER => {
            crate::return_errno!(EINVAL, "virtio-gpu rejected command parameter")
        }
        RESP_ERR_UNSPEC => crate::return_errno!(EIO, "virtio-gpu unspecified device error"),
        _ => crate::return_errno!(EIO, "virtio-gpu unexpected response code"),
    }
}

// ----------------------------------------------------------------------------
// Pixel formats (`enum virtio_gpu_formats`)
// ----------------------------------------------------------------------------

/// 2D resource pixel formats supported by `virtio-gpu`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum GpuFormat {
    B8G8R8A8Unorm = 1,
    B8G8R8X8Unorm = 2,
    A8R8G8B8Unorm = 3,
    X8R8G8B8Unorm = 4,
    R8G8B8A8Unorm = 67,
    X8B8G8R8Unorm = 68,
    A8B8G8R8Unorm = 121,
    R8G8B8X8Unorm = 134,
}

impl GpuFormat {
    /// Translates a DRM FourCC format code into the corresponding `virtio-gpu` format.
    pub fn from_drm_fourcc(fourcc: u32) -> Result<Self> {
        match fourcc {
            drm::DRM_FORMAT_XRGB8888 => Ok(Self::B8G8R8X8Unorm),
            drm::DRM_FORMAT_ARGB8888 => Ok(Self::B8G8R8A8Unorm),
            _ => crate::return_errno!(EINVAL, "pixel format not supported by virtio-gpu"),
        }
    }

    /// Bytes per pixel for this format.
    pub const fn bytes_per_pixel(self) -> u32 {
        4
    }
}

// ----------------------------------------------------------------------------
// Wire structures (`virtio 1.2` §5.7.6)
// ----------------------------------------------------------------------------

/// `struct virtio_gpu_ctrl_hdr` (24 bytes), prefix of every control and cursor message.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, FromBytes, IntoBytes, KnownLayout, Immutable)]
pub struct CtrlHdr {
    pub kind: u32,
    pub flags: u32,
    pub fence_id: u64,
    pub ctx_id: u32,
    pub ring_idx: u8,
    pub pad: [u8; 3],
}

impl CtrlHdr {
    /// Constructs a control header for `kind` with no context or fence.
    pub const fn new(kind: u32) -> Self {
        Self {
            kind,
            flags: 0,
            fence_id: 0,
            ctx_id: 0,
            ring_idx: 0,
            pad: [0; 3],
        }
    }
}

/// `struct virtio_gpu_rect` (16 bytes).
#[repr(C)]
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, FromBytes, IntoBytes, KnownLayout, Immutable,
)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    /// Creates a rectangle positioned at the origin `(0, 0)`.
    pub const fn sized(width: u32, height: u32) -> Self {
        Self {
            x: 0,
            y: 0,
            width,
            height,
        }
    }

    /// Checks whether this rectangle fits within `width x height`.
    pub const fn fits(self, width: u32, height: u32) -> bool {
        match (self.x.checked_add(self.width), self.y.checked_add(self.height)) {
            (Some(right), Some(bottom)) => right <= width && bottom <= height,
            _ => false,
        }
    }
}

/// `struct virtio_gpu_resource_create_2d`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, FromBytes, IntoBytes, KnownLayout, Immutable)]
pub struct ResourceCreate2d {
    pub hdr: CtrlHdr,
    pub resource_id: u32,
    pub format: u32,
    pub width: u32,
    pub height: u32,
}

/// `struct virtio_gpu_resource_unref`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, FromBytes, IntoBytes, KnownLayout, Immutable)]
pub struct ResourceUnref {
    pub hdr: CtrlHdr,
    pub resource_id: u32,
    pub pad: u32,
}

/// `struct virtio_gpu_mem_entry` (16 bytes).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, FromBytes, IntoBytes, KnownLayout, Immutable)]
pub struct MemEntry {
    pub addr: u64,
    pub length: u32,
    pub pad: u32,
}

/// `struct virtio_gpu_resource_attach_backing` with a single contiguous DMA entry.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, FromBytes, IntoBytes, KnownLayout, Immutable)]
pub struct AttachBackingSingle {
    pub hdr: CtrlHdr,
    pub resource_id: u32,
    pub nr_entries: u32,
    pub entry: MemEntry,
}

/// `struct virtio_gpu_resource_detach_backing`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, FromBytes, IntoBytes, KnownLayout, Immutable)]
pub struct DetachBacking {
    pub hdr: CtrlHdr,
    pub resource_id: u32,
    pub pad: u32,
}

/// `struct virtio_gpu_set_scanout`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, FromBytes, IntoBytes, KnownLayout, Immutable)]
pub struct SetScanout {
    pub hdr: CtrlHdr,
    pub rect: Rect,
    pub scanout_id: u32,
    pub resource_id: u32,
}

/// `struct virtio_gpu_resource_flush`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, FromBytes, IntoBytes, KnownLayout, Immutable)]
pub struct ResourceFlush {
    pub hdr: CtrlHdr,
    pub rect: Rect,
    pub resource_id: u32,
    pub pad: u32,
}

/// `struct virtio_gpu_transfer_to_host_2d`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, FromBytes, IntoBytes, KnownLayout, Immutable)]
pub struct TransferToHost2d {
    pub hdr: CtrlHdr,
    pub rect: Rect,
    pub offset: u64,
    pub resource_id: u32,
    pub pad: u32,
}

/// `struct virtio_gpu_display_one`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, FromBytes, IntoBytes, KnownLayout, Immutable)]
pub struct DisplayOne {
    pub rect: Rect,
    pub enabled: u32,
    pub flags: u32,
}

/// `struct virtio_gpu_resp_display_info` (408 bytes).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, FromBytes, IntoBytes, KnownLayout, Immutable)]
pub struct RespDisplayInfo {
    pub hdr: CtrlHdr,
    pub pmodes: [DisplayOne; MAX_SCANOUTS],
}

/// `struct virtio_gpu_get_edid`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, FromBytes, IntoBytes, KnownLayout, Immutable)]
pub struct GetEdid {
    pub hdr: CtrlHdr,
    pub scanout: u32,
    pub pad: u32,
}

/// `struct virtio_gpu_resp_edid` (1056 bytes).
#[repr(C)]
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable)]
pub struct RespEdid {
    pub hdr: CtrlHdr,
    pub size: u32,
    pub pad: u32,
    pub edid: [u8; EDID_MAX_BYTES],
}

impl Default for RespEdid {
    fn default() -> Self {
        Self {
            hdr: CtrlHdr::default(),
            size: 0,
            pad: 0,
            edid: [0u8; EDID_MAX_BYTES],
        }
    }
}

/// `struct virtio_gpu_cursor_pos` (16 bytes).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, FromBytes, IntoBytes, KnownLayout, Immutable)]
pub struct CursorPos {
    pub scanout_id: u32,
    pub x: u32,
    pub y: u32,
    pub pad: u32,
}

/// `struct virtio_gpu_update_cursor` (56 bytes), shared by `UPDATE_CURSOR` and `MOVE_CURSOR`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, FromBytes, IntoBytes, KnownLayout, Immutable)]
pub struct UpdateCursor {
    pub hdr: CtrlHdr,
    pub pos: CursorPos,
    pub resource_id: u32,
    pub hot_x: u32,
    pub hot_y: u32,
    pub pad: u32,
}

const _: () = {
    assert!(size_of::<CtrlHdr>() == 24);
    assert!(size_of::<Rect>() == 16);
    assert!(size_of::<DisplayOne>() == 24);
    assert!(size_of::<RespDisplayInfo>() == 408);
    assert!(size_of::<RespEdid>() == 1056);
    assert!(size_of::<UpdateCursor>() == 56);
};
