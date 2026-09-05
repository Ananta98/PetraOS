//! Direct Rendering Manager (DRM) Subsystem
//!
//! Provides the kernel DRM driver infrastructure: core types for display modes,
//! dumb buffer allocation, and the card abstraction. Concrete hardware drivers
//! (e.g. `framebuffer`, future display controllers and accelerators) live in submodules.

pub mod framebuffer;

pub use framebuffer::{
    FramebufferDevice, FramebufferDriver, FramebufferInfo,
    fb_read, fb_write, get_framebuffer_info, init,
};

// ===== DRM Core Types =====

/// Display mode descriptor (resolution + timing metadata).
#[derive(Debug, Clone, Copy, Default)]
pub struct DrmModeInfo {
    /// Horizontal active pixels.
    pub hdisplay: u32,
    /// Vertical active lines.
    pub vdisplay: u32,
    /// Refresh rate in Hz.
    pub vrefresh: u32,
    /// Pixel clock in kHz.
    pub clock: u32,
}

/// Dumb buffer descriptor (kernel-managed, CPU-accessible pixel buffer).
#[derive(Debug, Clone, Copy, Default)]
pub struct DrmDumbBuffer {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Bits per pixel.
    pub bpp: u32,
    /// Row stride in bytes (set by the driver on creation).
    pub pitch: u32,
    /// Total size in bytes.
    pub size: u64,
    /// Opaque handle for this buffer (driver-assigned).
    pub handle: u32,
}

/// Capabilities a DRM card may report to userspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u64)]
pub enum DrmCap {
    /// DRM_CAP_DUMB_BUFFER — driver supports dumb buffer creation.
    DumbBuffer = 0x1,
    /// DRM_CAP_VBLANK_HIGH_CRTC — vblank events use high CRTC index bits.
    VblankHighCrtc = 0x2,
    /// DRM_CAP_DUMB_PREFERRED_DEPTH — preferred colour depth for dumb buffers.
    DumbPreferredDepth = 0x3,
    /// DRM_CAP_DUMB_PREFER_SHADOW — prefer shadow buffer for dumb buffers.
    DumbPreferShadow = 0x4,
    /// DRM_CAP_PRIME — driver supports PRIME buffer sharing.
    Prime = 0x5,
    /// DRM_CAP_TIMESTAMP_MONOTONIC — timestamps use CLOCK_MONOTONIC.
    TimestampMonotonic = 0x6,
}

/// Top-level DRM card representation.
///
/// `DrmCard` binds a logical card index to its underlying device and
/// exposes mode enumeration and capability queries.
pub struct DrmCard {
    /// Card index (card0 = 0, card1 = 1, …).
    pub index: u32,
}

impl DrmCard {
    /// Create a new card descriptor for the given index.
    pub const fn new(index: u32) -> Self {
        Self { index }
    }

    /// Query a capability value.
    pub fn get_cap(&self, cap: DrmCap) -> u64 {
        match cap {
            DrmCap::DumbBuffer => 1,
            DrmCap::DumbPreferredDepth => 32,
            DrmCap::DumbPreferShadow => 0,
            DrmCap::TimestampMonotonic => 1,
            _ => 0,
        }
    }
}
