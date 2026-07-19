/// Device abstraction — type tag and base trait.
///
/// Every device in the system implements [`Device`], which allows the
/// device manager to store heterogeneous devices in a single registry.
use crate::fs::vfs::InodeOps;
use alloc::sync::Arc;

// ---------------------------------------------------------------------------
// DeviceType
// ---------------------------------------------------------------------------

/// High-level category of a device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceType {
    /// Character device: byte-oriented (e.g. serial, keyboard).
    Char,
    /// Block device: sector-oriented (e.g. disk, NVME).
    Block,
    /// Network interface.
    Net,
    /// Hardware timer or real-time clock.
    Timer,
    /// Graphics processing unit or framebuffer.
    Gpu,
    /// Input device
    Input,
}

// ---------------------------------------------------------------------------
// Device trait
// ---------------------------------------------------------------------------

/// Trait implemented by every device registered with the kernel.
pub trait Device: Send + Sync {
    /// Human-readable name used as the key in the device registry
    /// and as the node name under `/dev`.
    fn name(&self) -> &str;

    /// Returns the broad category this device belongs to.
    fn device_type(&self) -> DeviceType;

    /// Returns the [`InodeOps`] implementation for this device, if it
    /// should be exposed as a file under `/dev`.
    ///
    /// Returning `None` keeps the device in the registry but invisible
    /// in the virtual filesystem.
    fn inode_ops(&self) -> Option<Arc<dyn InodeOps>> {
        None
    }
}
