/// VirtIO GPU Register Offsets, Command Headers, and Constants.
///
/// Defines PCI vendor/device IDs, VirtIO legacy registers, control queue
/// command and response types, pixel formats, and command payload structures
/// per the VirtIO GPU Specification.

// ──────────────────────────────────────────────────────────────
// PCI Vendor and Device IDs
// ──────────────────────────────────────────────────────────────

/// Red Hat / VirtIO PCI vendor ID.
pub const VIRTIO_VENDOR_ID: u16 = 0x1AF4;

/// VirtIO transitional GPU device ID.
pub const VIRTIO_GPU_DEVICE_ID_TRANSITIONAL: u16 = 0x1010;

/// VirtIO modern GPU device ID (VirtIO 1.0+).
pub const VIRTIO_GPU_DEVICE_ID_MODERN: u16 = 0x1050;

// ──────────────────────────────────────────────────────────────
// VirtIO Device Status Bits
// ──────────────────────────────────────────────────────────────

pub const STATUS_RESET: u8 = 0;
pub const STATUS_ACKNOWLEDGE: u8 = 1;
pub const STATUS_DRIVER: u8 = 2;
pub const STATUS_DRIVER_OK: u8 = 4;
pub const STATUS_FEATURES_OK: u8 = 8;
pub const STATUS_NEEDS_RESET: u8 = 64;

// ──────────────────────────────────────────────────────────────
// Virtqueue Descriptor Flags
// ──────────────────────────────────────────────────────────────

pub const VIRTQ_DESC_F_NEXT: u16 = 1;
pub const VIRTQ_DESC_F_WRITE: u16 = 2;

// ──────────────────────────────────────────────────────────────
// Legacy I/O Register Offsets
// ──────────────────────────────────────────────────────────────

pub const LEGACY_DEVICE_FEATURES: usize = 0x00;
pub const LEGACY_DRIVER_FEATURES: usize = 0x04;
pub const LEGACY_QUEUE_PFN: usize = 0x08;
pub const LEGACY_QUEUE_SIZE: usize = 0x0C;
pub const LEGACY_QUEUE_SELECT: usize = 0x0E;
pub const LEGACY_QUEUE_NOTIFY: usize = 0x10;
pub const LEGACY_STATUS: usize = 0x12;
pub const LEGACY_ISR_STATUS: usize = 0x13;
pub const LEGACY_DEVICE_CONFIG_OFFSET: usize = 0x14;

// ──────────────────────────────────────────────────────────────
// VirtIO GPU Command Types
// ──────────────────────────────────────────────────────────────

pub const VIRTIO_GPU_CMD_GET_DISPLAY_INFO: u32 = 0x0100;
pub const VIRTIO_GPU_CMD_RESOURCE_CREATE_2D: u32 = 0x0101;
pub const VIRTIO_GPU_CMD_RESOURCE_UNREF: u32 = 0x0102;
pub const VIRTIO_GPU_CMD_SET_SCANOUT: u32 = 0x0103;
pub const VIRTIO_GPU_CMD_RESOURCE_FLUSH: u32 = 0x0104;
pub const VIRTIO_GPU_CMD_TRANSFER_TO_HOST_2D: u32 = 0x0105;
pub const VIRTIO_GPU_CMD_RESOURCE_ATTACH_BACKING: u32 = 0x0106;
pub const VIRTIO_GPU_CMD_RESOURCE_DETACH_BACKING: u32 = 0x0107;

// Responses
pub const VIRTIO_GPU_RESP_OK_NODATA: u32 = 0x1100;
pub const VIRTIO_GPU_RESP_OK_DISPLAY_INFO: u32 = 0x1101;
pub const VIRTIO_GPU_RESP_OK_CAPSET_INFO: u32 = 0x1102;
pub const VIRTIO_GPU_RESP_OK_CAPSET: u32 = 0x1103;

pub const VIRTIO_GPU_RESP_ERR_UNSPEC: u32 = 0x1200;
pub const VIRTIO_GPU_RESP_ERR_OUT_OF_MEMORY: u32 = 0x1201;
pub const VIRTIO_GPU_RESP_ERR_INVALID_SCANOUT: u32 = 0x1202;
pub const VIRTIO_GPU_RESP_ERR_INVALID_RESOURCE_ID: u32 = 0x1203;
pub const VIRTIO_GPU_RESP_ERR_INVALID_PARAMETER: u32 = 0x1204;

// ──────────────────────────────────────────────────────────────
// VirtIO GPU Formats
// ──────────────────────────────────────────────────────────────

pub const VIRTIO_GPU_FORMAT_B8G8R8A8_UNORM: u32 = 1;
pub const VIRTIO_GPU_FORMAT_B8G8R8X8_UNORM: u32 = 2;
pub const VIRTIO_GPU_FORMAT_A8R8G8B8_UNORM: u32 = 3;
pub const VIRTIO_GPU_FORMAT_X8R8G8B8_UNORM: u32 = 4;
pub const VIRTIO_GPU_FORMAT_R8G8B8A8_UNORM: u32 = 67;
pub const VIRTIO_GPU_FORMAT_X8B8G8R8_UNORM: u32 = 68;

pub const VIRTIO_GPU_MAX_SCANOUTS: usize = 16;

// ──────────────────────────────────────────────────────────────
// Struct Layout Definitions
// ──────────────────────────────────────────────────────────────

/// VirtIO GPU Control Command Header.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct VirtioGpuCtrlHdr {
    pub type_: u32,
    pub flags: u32,
    pub fence_id: u64,
    pub ctx_id: u32,
    pub padding: u32,
}

/// 2D Rectangle.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct VirtioGpuRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// Resource 2D Creation Payload.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct VirtioGpuResourceCreate2d {
    pub hdr: VirtioGpuCtrlHdr,
    pub resource_id: u32,
    pub format: u32,
    pub width: u32,
    pub height: u32,
}

/// Set Scanout Payload.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct VirtioGpuSetScanout {
    pub hdr: VirtioGpuCtrlHdr,
    pub r: VirtioGpuRect,
    pub scanout_id: u32,
    pub resource_id: u32,
}

/// Resource Flush Payload.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct VirtioGpuResourceFlush {
    pub hdr: VirtioGpuCtrlHdr,
    pub r: VirtioGpuRect,
    pub resource_id: u32,
    pub padding: u32,
}

/// Transfer to Host 2D Payload.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct VirtioGpuTransferToHost2d {
    pub hdr: VirtioGpuCtrlHdr,
    pub r: VirtioGpuRect,
    pub offset: u64,
    pub resource_id: u32,
    pub padding: u32,
}

/// Memory Entry for Attach Backing.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct VirtioGpuMemEntry {
    pub addr: u64,
    pub length: u32,
    pub padding: u32,
}

/// Resource Attach Backing Payload Header.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct VirtioGpuResourceAttachBacking {
    pub hdr: VirtioGpuCtrlHdr,
    pub resource_id: u32,
    pub nr_entries: u32,
}

/// Single Scanout Display Information.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct VirtioGpuDisplayOne {
    pub r: VirtioGpuRect,
    pub enabled: u32,
    pub flags: u32,
}

/// Response payload for GET_DISPLAY_INFO.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VirtioGpuRespDisplayInfo {
    pub hdr: VirtioGpuCtrlHdr,
    pub pmodes: [VirtioGpuDisplayOne; VIRTIO_GPU_MAX_SCANOUTS],
}
