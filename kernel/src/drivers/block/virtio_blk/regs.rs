/// VirtIO Block Device Register Offsets and Constants
///
/// Defines the VirtIO PCI common configuration structure offsets,
/// device status bits, feature negotiation flags, and block device
/// request/response types per the VirtIO 1.x specification.

// ──────────────────────────────────────────────────────────────
// VirtIO PCI Vendor and Device IDs
// ──────────────────────────────────────────────────────────────

/// Red Hat / VirtIO PCI vendor ID.
pub const VIRTIO_VENDOR_ID: u16 = 0x1AF4;

/// VirtIO transitional block device (PCI device ID range 0x1000–0x103F).
pub const VIRTIO_BLK_DEVICE_ID_TRANSITIONAL: u16 = 0x1001;

/// VirtIO modern block device (VirtIO 1.0+, PCI device ID 0x1042).
pub const VIRTIO_BLK_DEVICE_ID_MODERN: u16 = 0x1042;

// ──────────────────────────────────────────────────────────────
// VirtIO Device Status Bits (§2.1)
// ──────────────────────────────────────────────────────────────

/// Guest OS has noticed the device.
pub const STATUS_ACKNOWLEDGE: u8 = 1;

/// Guest OS knows how to drive the device.
pub const STATUS_DRIVER: u8 = 2;

/// Feature negotiation is complete.
pub const STATUS_FEATURES_OK: u8 = 8;

/// Driver is fully set up and ready.
pub const STATUS_DRIVER_OK: u8 = 4;

/// Device has experienced an error and needs reset.
pub const STATUS_NEEDS_RESET: u8 = 64;

/// Reset value — writing 0 triggers a device reset.
pub const STATUS_RESET: u8 = 0;

// ──────────────────────────────────────────────────────────────
// VirtIO Common Configuration Structure Offsets (§4.1.4.3)
//
// These are offsets within the BAR region pointed to by the
// VIRTIO_PCI_CAP_COMMON_CFG capability structure.
// For legacy/transitional devices, these map to the legacy
// I/O register layout at BAR0.
// ──────────────────────────────────────────────────────────────

/// Device feature bits selector (write). Selects which 32-bit word
/// of the 64-bit feature register to access via `device_feature`.
pub const COMMON_DFSELECT: usize = 0x00;

/// Device feature bits (read-only). Returns the 32-bit word selected
/// by `device_feature_select`.
pub const COMMON_DF: usize = 0x04;

/// Driver feature bits selector (write).
pub const COMMON_GFSELECT: usize = 0x08;

/// Driver feature bits (read/write).
pub const COMMON_GF: usize = 0x0C;

/// MSI-X configuration vector for config changes.
pub const COMMON_MSIX_CONFIG: usize = 0x10;

/// Number of virtqueues supported by this device (read-only).
pub const COMMON_NUM_QUEUES: usize = 0x12;

/// Device status register (read/write).
pub const COMMON_DEVICE_STATUS: usize = 0x14;

/// Configuration generation counter (read-only).
pub const COMMON_CONFIG_GEN: usize = 0x15;

/// Queue selector (write). Selects which virtqueue to configure.
pub const COMMON_QUEUE_SELECT: usize = 0x16;

/// Queue size (read/write). Maximum is read; driver writes actual size.
pub const COMMON_QUEUE_SIZE: usize = 0x18;

/// MSI-X vector for this queue's interrupts.
pub const COMMON_QUEUE_MSIX_VECTOR: usize = 0x1A;

/// Queue enable (read/write). Write 1 to enable the selected queue.
pub const COMMON_QUEUE_ENABLE: usize = 0x1C;

/// Queue notify offset multiplied by notify_off_multiplier gives the
/// notification address offset.
pub const COMMON_QUEUE_NOTIFY_OFF: usize = 0x1E;

/// Queue descriptor table address (64-bit, low 32 bits).
pub const COMMON_QUEUE_DESC_LO: usize = 0x20;

/// Queue descriptor table address (high 32 bits).
pub const COMMON_QUEUE_DESC_HI: usize = 0x24;

/// Queue available ring address (low 32 bits).
pub const COMMON_QUEUE_AVAIL_LO: usize = 0x28;

/// Queue available ring address (high 32 bits).
pub const COMMON_QUEUE_AVAIL_HI: usize = 0x2C;

/// Queue used ring address (low 32 bits).
pub const COMMON_QUEUE_USED_LO: usize = 0x30;

/// Queue used ring address (high 32 bits).
pub const COMMON_QUEUE_USED_HI: usize = 0x34;

// ──────────────────────────────────────────────────────────────
// Legacy I/O Port Register Offsets (§4.1.4.8)
//
// For transitional devices (device_id 0x1001), virtio config lives
// at BAR0 as I/O port registers. These offsets are for the legacy
// interface used by QEMU's default virtio-blk-pci device.
// ──────────────────────────────────────────────────────────────

/// Device features (32-bit, read-only).
pub const LEGACY_DEVICE_FEATURES: usize = 0x00;

/// Driver (guest) features (32-bit, read/write).
pub const LEGACY_DRIVER_FEATURES: usize = 0x04;

/// Queue address PFN (32-bit, read/write). Physical page frame number
/// of the virtqueue descriptor area.
pub const LEGACY_QUEUE_PFN: usize = 0x08;

/// Queue size (16-bit, read-only). Number of entries in the queue.
pub const LEGACY_QUEUE_SIZE: usize = 0x0C;

/// Queue selector (16-bit, write). Selects which queue to configure.
pub const LEGACY_QUEUE_SELECT: usize = 0x0E;

/// Queue notify (16-bit, write). Notifies the device that new buffers
/// are available in the queue with the written index.
pub const LEGACY_QUEUE_NOTIFY: usize = 0x10;

/// Device status (8-bit, read/write).
pub const LEGACY_STATUS: usize = 0x12;

/// ISR status (8-bit, read). Reading clears the ISR.
pub const LEGACY_ISR_STATUS: usize = 0x13;

/// Start of device-specific configuration space for legacy devices.
/// For virtio-blk, the block device config starts here.
pub const LEGACY_DEVICE_CONFIG_OFFSET: usize = 0x14;

// ──────────────────────────────────────────────────────────────
// VirtIO Block Device Feature Bits (§5.2.3)
// ──────────────────────────────────────────────────────────────

/// Maximum size of any single segment (feature bit 1).
pub const VIRTIO_BLK_F_SIZE_MAX: u32 = 1 << 1;

/// Maximum number of segments in a request (feature bit 2).
pub const VIRTIO_BLK_F_SEG_MAX: u32 = 1 << 2;

/// Disk geometry is available (feature bit 4).
pub const VIRTIO_BLK_F_GEOMETRY: u32 = 1 << 4;

/// Device is read-only (feature bit 5).
pub const VIRTIO_BLK_F_RO: u32 = 1 << 5;

/// Block size is available in config (feature bit 6).
pub const VIRTIO_BLK_F_BLK_SIZE: u32 = 1 << 6;

/// Device supports flush command (feature bit 9).
pub const VIRTIO_BLK_F_FLUSH: u32 = 1 << 9;

// ──────────────────────────────────────────────────────────────
// VirtIO Block Request Types (§5.2.6)
// ──────────────────────────────────────────────────────────────

/// Read request: device reads data from disk and writes to driver buffer.
pub const VIRTIO_BLK_T_IN: u32 = 0;

/// Write request: device reads from driver buffer and writes to disk.
pub const VIRTIO_BLK_T_OUT: u32 = 1;

/// Flush request: device flushes volatile write caches.
pub const VIRTIO_BLK_T_FLUSH: u32 = 4;

// ──────────────────────────────────────────────────────────────
// VirtIO Block Request Status Values (§5.2.6)
// ──────────────────────────────────────────────────────────────

/// Request completed successfully.
pub const VIRTIO_BLK_S_OK: u8 = 0;

/// Request failed with a device or I/O error.
pub const VIRTIO_BLK_S_IOERR: u8 = 1;

/// Request type is unsupported by the device.
pub const VIRTIO_BLK_S_UNSUPP: u8 = 2;

// ──────────────────────────────────────────────────────────────
// VirtIO Block Device Config Offsets (§5.2.4)
//
// Offsets within the device-specific configuration region.
// For legacy devices, add LEGACY_DEVICE_CONFIG_OFFSET.
// ──────────────────────────────────────────────────────────────

/// Total capacity in 512-byte sectors (64-bit, read-only).
pub const BLK_CFG_CAPACITY: usize = 0x00;

/// Maximum segment size in bytes (32-bit, read-only). Only valid if
/// `VIRTIO_BLK_F_SIZE_MAX` is negotiated.
pub const BLK_CFG_SIZE_MAX: usize = 0x08;

/// Maximum number of segments per request (32-bit, read-only). Only
/// valid if `VIRTIO_BLK_F_SEG_MAX` is negotiated.
pub const BLK_CFG_SEG_MAX: usize = 0x0C;

/// Block size in bytes (32-bit, read-only). Only valid if
/// `VIRTIO_BLK_F_BLK_SIZE` is negotiated. Defaults to 512 otherwise.
pub const BLK_CFG_BLK_SIZE: usize = 0x14;

// ──────────────────────────────────────────────────────────────
// Block size constant
// ──────────────────────────────────────────────────────────────

/// Default VirtIO block sector size (512 bytes).
pub const VIRTIO_BLK_SECTOR_SIZE: usize = 512;

// ──────────────────────────────────────────────────────────────
// VirtIO Block Request Header Layout
// ──────────────────────────────────────────────────────────────

/// Size of the `virtio_blk_req` header in bytes.
///
/// Layout:
/// - `type` (u32)  — request type (IN, OUT, FLUSH)
/// - `reserved` (u32) — must be zero
/// - `sector` (u64) — starting sector for the I/O operation
pub const BLK_REQ_HEADER_SIZE: usize = 16;
