/// VirtIO PCI Transport — Device-Agnostic Register Constants
///
/// Defines the VirtIO vendor ID, device status bits, PCI capability type
/// identifiers, and common configuration register offsets that are shared
/// across all VirtIO device types (block, network, GPU, input, etc.).
///
/// References:
/// - VirtIO 1.2 Specification §4.1 (Virtio Over PCI Bus)
/// - VirtIO 1.2 Specification §2.1  (Device Status Field)

// ──────────────────────────────────────────────────────────────
// VirtIO PCI Vendor ID
// ──────────────────────────────────────────────────────────────

/// PCI vendor ID assigned to Red Hat / QEMU VirtIO devices.
pub const VIRTIO_VENDOR_ID: u16 = 0x1AF4;

// ──────────────────────────────────────────────────────────────
// VirtIO PCI Device ID Ranges (§4.1.2)
//
// Transitional devices use legacy IDs (0x1000–0x103F).
// Modern-only devices use IDs starting from 0x1040.
// ──────────────────────────────────────────────────────────────

/// First device ID in the transitional (legacy-compatible) range.
pub const DEVICE_ID_TRANSITIONAL_MIN: u16 = 0x1000;
/// Last device ID in the transitional range.
pub const DEVICE_ID_TRANSITIONAL_MAX: u16 = 0x103F;

/// Base device ID for modern (VirtIO 1.0+) devices.
pub const DEVICE_ID_MODERN_BASE: u16 = 0x1040;

/// Well-known transitional device IDs.
pub const DEVICE_ID_NET_TRANSITIONAL: u16 = 0x1000;
pub const DEVICE_ID_BLK_TRANSITIONAL: u16 = 0x1001;
pub const DEVICE_ID_CONSOLE_TRANSITIONAL: u16 = 0x1003;
pub const DEVICE_ID_SCSI_TRANSITIONAL: u16 = 0x1004;
pub const DEVICE_ID_INPUT_TRANSITIONAL: u16 = 0x1005;
pub const DEVICE_ID_GPU_TRANSITIONAL: u16 = 0x1007;
pub const DEVICE_ID_CRYPTO_TRANSITIONAL: u16 = 0x103F;

/// Well-known modern (VirtIO 1.0+) device IDs.
pub const DEVICE_ID_NET_MODERN: u16 = 0x1041;
pub const DEVICE_ID_BLK_MODERN: u16 = 0x1042;
pub const DEVICE_ID_CONSOLE_MODERN: u16 = 0x1043;
pub const DEVICE_ID_GPU_MODERN: u16 = 0x1050;
pub const DEVICE_ID_INPUT_MODERN: u16 = 0x1052;

// ──────────────────────────────────────────────────────────────
// VirtIO Device Status Bits (§2.1)
//
// Written to the `device_status` register during the standard
// initialization sequence. Each bit builds on the previous.
// ──────────────────────────────────────────────────────────────

/// Reset: writing this value to the status register triggers a device reset.
pub const STATUS_RESET: u8 = 0;

/// ACKNOWLEDGE: the guest OS has noticed the device.
pub const STATUS_ACKNOWLEDGE: u8 = 1 << 0;

/// DRIVER: the guest OS knows how to drive the device.
pub const STATUS_DRIVER: u8 = 1 << 1;

/// DRIVER_OK: the driver is set up and ready to drive the device.
pub const STATUS_DRIVER_OK: u8 = 1 << 2;

/// FEATURES_OK: the driver has acknowledged all the features it understands,
/// and feature negotiation is complete.
pub const STATUS_FEATURES_OK: u8 = 1 << 3;

/// DEVICE_NEEDS_RESET: the device has experienced an error from which it
/// cannot recover. The driver must reset the device.
pub const STATUS_NEEDS_RESET: u8 = 1 << 6;

/// FAILED: something has gone wrong in the guest, and it has given up on
/// the device. This is a terminal state; a device reset is needed.
pub const STATUS_FAILED: u8 = 1 << 7;

// ──────────────────────────────────────────────────────────────
// VirtIO PCI Capability Types (§4.1.4)
//
// Each VirtIO capability is a PCI vendor-specific capability (ID 0x09).
// The capability type field (at cap_offset + 3) distinguishes them.
// ──────────────────────────────────────────────────────────────

/// Common configuration structure: device-status, feature bits, queue setup.
pub const CAP_TYPE_COMMON_CFG: u8 = 1;
/// Notification region: the memory range used to notify the device.
pub const CAP_TYPE_NOTIFY_CFG: u8 = 2;
/// ISR status: read to acknowledge interrupts (and clear the ISR flag).
pub const CAP_TYPE_ISR_CFG: u8 = 3;
/// Device-specific configuration: device-type-dependent config fields.
pub const CAP_TYPE_DEVICE_CFG: u8 = 4;
/// PCI Alternative Access (used by some QEMU devices).
pub const CAP_TYPE_PCI_CFG: u8 = 5;

// ──────────────────────────────────────────────────────────────
// VirtIO PCI Capability Structure Layout (§4.1.4.1)
//
// Offsets within a VirtIO vendor-specific PCI capability block.
// Base offset `cap_offset` refers to the standard PCI capability header.
// ──────────────────────────────────────────────────────────────

/// Offset of the VirtIO capability type field relative to the capability header.
pub const CAP_FIELD_CAP_TYPE: u8 = 3;
/// Offset of the BAR index field.
pub const CAP_FIELD_BAR: u8 = 4;
/// Offset of the region offset within the BAR (4-byte field).
pub const CAP_FIELD_OFFSET: u8 = 8;
/// Offset of the region length field (4-byte field).
pub const CAP_FIELD_LENGTH: u8 = 12;
/// Offset of the extra 4-byte field (used by NOTIFY_CFG for the multiplier).
pub const CAP_FIELD_EXTRA: u8 = 16;

// ──────────────────────────────────────────────────────────────
// VirtIO Modern Common Configuration Offsets (§4.1.4.3)
//
// Offsets within the BAR region pointed to by COMMON_CFG capability.
// ──────────────────────────────────────────────────────────────

/// Device feature bits selector (u32, write).
pub const COMMON_DEVICE_FEATURE_SELECT: usize = 0x00;
/// Device feature bits (u32, read-only).
pub const COMMON_DEVICE_FEATURE: usize = 0x04;
/// Driver feature bits selector (u32, write).
pub const COMMON_DRIVER_FEATURE_SELECT: usize = 0x08;
/// Driver feature bits (u32, read/write).
pub const COMMON_DRIVER_FEATURE: usize = 0x0C;
/// MSI-X configuration vector (u16, read/write).
pub const COMMON_MSIX_CONFIG: usize = 0x10;
/// Number of virtqueues supported (u16, read-only).
pub const COMMON_NUM_QUEUES: usize = 0x12;
/// Device status register (u8, read/write).
pub const COMMON_DEVICE_STATUS: usize = 0x14;
/// Configuration generation counter (u8, read-only).
pub const COMMON_CONFIG_GENERATION: usize = 0x15;
/// Queue selector (u16, write). Must be written before accessing queue fields.
pub const COMMON_QUEUE_SELECT: usize = 0x16;
/// Queue size (u16, read/write). Max is read; driver writes the actual size.
pub const COMMON_QUEUE_SIZE: usize = 0x18;
/// Queue MSI-X vector (u16, read/write).
pub const COMMON_QUEUE_MSIX_VECTOR: usize = 0x1A;
/// Queue enable flag (u16, read/write). Write 1 to enable the selected queue.
pub const COMMON_QUEUE_ENABLE: usize = 0x1C;
/// Queue notify offset (u16, read-only). Multiply by notify_off_multiplier for
/// the byte offset within the notification BAR.
pub const COMMON_QUEUE_NOTIFY_OFF: usize = 0x1E;
/// Descriptor table address — low 32 bits (u32, read/write).
pub const COMMON_QUEUE_DESC_LO: usize = 0x20;
/// Descriptor table address — high 32 bits (u32, read/write).
pub const COMMON_QUEUE_DESC_HI: usize = 0x24;
/// Available ring address — low 32 bits (u32, read/write).
pub const COMMON_QUEUE_AVAIL_LO: usize = 0x28;
/// Available ring address — high 32 bits (u32, read/write).
pub const COMMON_QUEUE_AVAIL_HI: usize = 0x2C;
/// Used ring address — low 32 bits (u32, read/write).
pub const COMMON_QUEUE_USED_LO: usize = 0x30;
/// Used ring address — high 32 bits (u32, read/write).
pub const COMMON_QUEUE_USED_HI: usize = 0x34;

// ──────────────────────────────────────────────────────────────
// VirtIO Legacy I/O Register Offsets (§4.1.4.8)
//
// For transitional devices (device_id in 0x1000–0x103F), these are the
// I/O port register offsets within BAR0 when no modern capabilities are found.
// ──────────────────────────────────────────────────────────────

/// Device features (u32, read-only).
pub const LEGACY_DEVICE_FEATURES: usize = 0x00;
/// Driver (guest) features (u32, read/write).
pub const LEGACY_DRIVER_FEATURES: usize = 0x04;
/// Queue address as a page frame number (u32, read/write).
pub const LEGACY_QUEUE_PFN: usize = 0x08;
/// Queue size (u16, read-only).
pub const LEGACY_QUEUE_SIZE: usize = 0x0C;
/// Queue selector (u16, write).
pub const LEGACY_QUEUE_SELECT: usize = 0x0E;
/// Queue notify (u16, write). Writing the queue index triggers notification.
pub const LEGACY_QUEUE_NOTIFY: usize = 0x10;
/// Device status (u8, read/write).
pub const LEGACY_DEVICE_STATUS: usize = 0x12;
/// ISR status (u8, read). Reading this register clears the interrupt.
pub const LEGACY_ISR_STATUS: usize = 0x13;
/// Start offset of device-specific configuration space within the BAR.
pub const LEGACY_DEVICE_CONFIG_START: usize = 0x14;

// ──────────────────────────────────────────────────────────────
// ISR Status Bits (§4.1.4.5)
// ──────────────────────────────────────────────────────────────

/// Virtqueue interrupt: set when the device has consumed a buffer from a queue.
pub const ISR_VIRTQUEUE: u8 = 1 << 0;
/// Device configuration change interrupt.
pub const ISR_CONFIG_CHANGE: u8 = 1 << 1;

// ──────────────────────────────────────────────────────────────
// VirtIO Feature Bits (§6)
// ──────────────────────────────────────────────────────────────

/// RING_INDIRECT_DESC: the driver can use indirect descriptor tables.
pub const VIRTIO_F_RING_INDIRECT_DESC: u64 = 1 << 28;
/// RING_EVENT_IDX: the driver and device can suppress notifications.
pub const VIRTIO_F_RING_EVENT_IDX: u64 = 1 << 29;
/// VERSION_1: this is a VirtIO 1.0 compliant device.
pub const VIRTIO_F_VERSION_1: u64 = 1 << 32;
/// ACCESS_PLATFORM: the device needs the driver to provide IOMMU translation.
pub const VIRTIO_F_ACCESS_PLATFORM: u64 = 1 << 33;
/// RING_PACKED: the device supports the packed virtqueue layout.
pub const VIRTIO_F_RING_PACKED: u64 = 1 << 34;
/// IN_ORDER: the device processes requests in submission order.
pub const VIRTIO_F_IN_ORDER: u64 = 1 << 35;
/// ORDER_PLATFORM: memory ordering is guaranteed by the platform.
pub const VIRTIO_F_ORDER_PLATFORM: u64 = 1 << 36;
/// SR_IOV: the device supports Single Root I/O Virtualization.
pub const VIRTIO_F_SR_IOV: u64 = 1 << 37;
/// NOTIFICATION_DATA: the driver can supply extra data in device notifications.
pub const VIRTIO_F_NOTIFICATION_DATA: u64 = 1 << 38;
