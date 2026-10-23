// SPDX-License-Identifier: GPL-2.0

//! Direct Rendering Manager (DRM) userspace ABI (`linux/drm.h`, `linux/drm_mode.h`).
//!
//! Every layout is transcribed from the Linux UAPI headers so that stock
//! libdrm-based user space works unmodified against `/dev/dri/cardN`.
//! Request numbers are computed with the generic encoder in
//! [`crate::uapi::ioctl`], reproducing the exact Linux `_IOC_*` bit layout
//! (dir<31:30 | size<29:16 | type<15:8 | nr<7:0).
//!
//! All structures additionally derive the zerocopy traits so that they can be
//! marshalled through [`ostd::mm::VmReader`] / [`ostd::mm::VmWriter`], whose
//! `read_val`/`write_val` operations require plain-old-data. Two deviations
//! from the raw C declarations, both layout-neutral on 64-bit targets:
//! - userspace pointer members are typed as `usize` (zerocopy declines to
//!   implement byte conversion traits for raw pointers), and
//! - compiler-inserted padding holes are spelled out as explicit `pad`
//!   fields so `IntoBytes` is satisfied.
//!
//! The mode-setting (0xA0..) ioctl family and their structures are added
//! alongside the KMS integration.

use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

use crate::uapi::ioctl::{_io, _iow, _iowr};

/// `'d'` — command family shared by all DRM ioctl requests.
pub const DRM_IOCTL_BASE: u32 = 0x64;

/// First command number of the per-driver ioctl range (0x40..0x9F).
pub const DRM_COMMAND_BASE: u32 = 0x40;
/// First command number of the generic KMS range (0xA0..).
pub const DRM_COMMAND_END: u32 = 0xA0;

// ----------------------------------------------------------------------------
// Core ioctl requests (verified against /usr/include/drm/drm.h)
// ----------------------------------------------------------------------------

/// Query driver metadata. In: buffer capacities; out: lengths and buffers.
pub const DRM_IOCTL_VERSION: u32 = _iowr::<DrmVersion>(DRM_IOCTL_BASE, 0x00);
/// Close a GEM handle.
pub const DRM_IOCTL_GEM_CLOSE: u32 = _iow::<DrmGemClose>(DRM_IOCTL_BASE, 0x09);
/// Query a driver capability.
pub const DRM_IOCTL_GET_CAP: u32 = _iowr::<DrmGetCap>(DRM_IOCTL_BASE, 0x0C);
/// Set a client capability (e.g. universal planes, atomic).
pub const DRM_IOCTL_SET_CLIENT_CAP: u32 = _iow::<DrmSetClientCap>(DRM_IOCTL_BASE, 0x0D);

/// Elect a legacy DRM master (accepted as a no-op).
pub const DRM_IOCTL_SET_MASTER: u32 = _io(DRM_IOCTL_BASE, 0x1E);
/// Drop a legacy DRM master (accepted as a no-op).
pub const DRM_IOCTL_DROP_MASTER: u32 = _io(DRM_IOCTL_BASE, 0x1F);

// ----------------------------------------------------------------------------
// Dumb buffers (verified: /usr/include/drm/drm.h 0xB2..0xB4)
// ----------------------------------------------------------------------------

/// Allocate a scanout buffer, returning a GEM handle with driver-decided layout.
pub const DRM_IOCTL_MODE_CREATE_DUMB: u32 = _iowr::<DrmModeCreateDumb>(DRM_IOCTL_BASE, 0xB2);
/// Return the fake offset used to `mmap(2)` a created dumb buffer.
pub const DRM_IOCTL_MODE_MAP_DUMB: u32 = _iowr::<DrmModeMapDumb>(DRM_IOCTL_BASE, 0xB3);
/// Release a dumb buffer allocated with `CREATE_DUMB`.
pub const DRM_IOCTL_MODE_DESTROY_DUMB: u32 = _iowr::<DrmModeDestroyDumb>(DRM_IOCTL_BASE, 0xB4);

// ----------------------------------------------------------------------------
// Core structures (verified against /usr/include/drm/drm.h)
// ----------------------------------------------------------------------------

/// `struct drm_version` — `DRM_IOCTL_VERSION`.
///
/// User space sets the three `*_len` capacities (pointers may be null) to
/// query the lengths, then calls again with allocated buffers. Strings are
/// not `NUL` terminated and truncation is permitted.
#[repr(C)]
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable, Default)]
pub struct DrmVersion {
    pub version_major: i32,
    pub version_minor: i32,
    pub version_patchlevel: i32,
    /// Explicit compiler padding hole into the 8-byte aligned fields.
    pub pad: u32,
    pub name_len: usize,
    pub name: usize,
    pub date_len: usize,
    pub date: usize,
    pub desc_len: usize,
    pub desc: usize,
}

/// `struct drm_auth` — magic for the legacy master election calls.
#[repr(C)]
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable, Default)]
pub struct DrmAuth {
    pub magic: u32,
}

/// `struct drm_gem_close` — `DRM_IOCTL_GEM_CLOSE` (also `DESTROY_DUMB`).
#[repr(C)]
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable, Default)]
pub struct DrmGemClose {
    pub handle: u32,
    pub pad: u32,
}

/// `struct drm_get_cap` — `DRM_IOCTL_GET_CAP`.
#[repr(C)]
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable, Default)]
pub struct DrmGetCap {
    /// Request.
    pub capability: u64,
    /// Response.
    pub value: u64,
}

/// `struct drm_set_client_cap` — `DRM_IOCTL_SET_CLIENT_CAP`.
#[repr(C)]
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable, Default)]
pub struct DrmSetClientCap {
    pub capability: u64,
    pub value: u64,
}

/// `struct drm_mode_create_dumb` — `DRM_IOCTL_MODE_CREATE_DUMB`.
///
/// User space fills `height`/`width`/`bpp`/`flags` (flags must be 0); the
/// kernel fills `handle`, `pitch` and `size`.
#[repr(C)]
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable, Default)]
pub struct DrmModeCreateDumb {
    pub height: u32,
    pub width: u32,
    pub bpp: u32,
    pub flags: u32,
    pub handle: u32,
    pub pitch: u32,
    pub size: u64,
}

/// `struct drm_mode_map_dumb` — `DRM_IOCTL_MODE_MAP_DUMB`.
///
/// User space sets `handle`; the kernel fills `offset` with the fake offset
/// to use in a subsequent `mmap(2)` of the same DRM fd.
#[repr(C)]
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable, Default)]
pub struct DrmModeMapDumb {
    pub handle: u32,
    pub pad: u32,
    pub offset: u64,
}

/// `struct drm_mode_destroy_dumb` — `DRM_IOCTL_MODE_DESTROY_DUMB`.
#[repr(C)]
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable, Default)]
pub struct DrmModeDestroyDumb {
    pub handle: u32,
}

// ----------------------------------------------------------------------------
// KMS mode-setting ioctls (verified: /usr/include/drm/drm.h 0xA0..0xB8)
// ----------------------------------------------------------------------------

/// Enumerate the display objects and buffer limits of the device.
pub const DRM_IOCTL_MODE_GETRESOURCES: u32 = _iowr::<DrmModeCardRes>(DRM_IOCTL_BASE, 0xA0);
/// Query a CRTC.
pub const DRM_IOCTL_MODE_GETCRTC: u32 = _iowr::<DrmModeCrtc>(DRM_IOCTL_BASE, 0xA1);
/// Enable a CRTC with a framebuffer and connector list.
pub const DRM_IOCTL_MODE_SETCRTC: u32 = _iowr::<DrmModeCrtc>(DRM_IOCTL_BASE, 0xA2);
/// Set or move the hardware cursor on a CRTC.
pub const DRM_IOCTL_MODE_CURSOR: u32 = _iowr::<DrmModeCursor>(DRM_IOCTL_BASE, 0xA3);
/// Query an encoder.
pub const DRM_IOCTL_MODE_GETENCODER: u32 = _iowr::<DrmModeGetEncoder>(DRM_IOCTL_BASE, 0xA6);
/// Query a connector.
pub const DRM_IOCTL_MODE_GETCONNECTOR: u32 = _iowr::<DrmModeGetConnector>(DRM_IOCTL_BASE, 0xA7);
/// Query framebuffer metadata.
pub const DRM_IOCTL_MODE_GETFB: u32 = _iowr::<DrmModeFbCmd>(DRM_IOCTL_BASE, 0xAD);
/// Register a framebuffer from a GEM handle (legacy single-plane form).
pub const DRM_IOCTL_MODE_ADDFB: u32 = _iowr::<DrmModeFbCmd>(DRM_IOCTL_BASE, 0xAE);
/// Remove a framebuffer (the request carries the raw object id).
pub const DRM_IOCTL_MODE_RMFB: u32 = _iowr::<u32>(DRM_IOCTL_BASE, 0xAF);
/// Asynchronously swap the framebuffer displayed by a CRTC.
pub const DRM_IOCTL_MODE_PAGE_FLIP: u32 = _iowr::<DrmModeCrtcPageFlip>(DRM_IOCTL_BASE, 0xB0);
/// Notify the driver of dirty framebuffer regions (flush hook).
pub const DRM_IOCTL_MODE_DIRTYFB: u32 = _iowr::<DrmModeFbDirtyCmd>(DRM_IOCTL_BASE, 0xB1);
/// Change which plane displays a framebuffer.
pub const DRM_IOCTL_MODE_SETPLANE: u32 = _iowr::<DrmModeSetPlane>(DRM_IOCTL_BASE, 0xB7);
/// Register a framebuffer from a GEM handle (multi-plane, fourcc form).
pub const DRM_IOCTL_MODE_ADDFB2: u32 = _iowr::<DrmModeFbCmd2>(DRM_IOCTL_BASE, 0xB8);
/// Set or move the hardware cursor on a CRTC with hotspot coordinates.
pub const DRM_IOCTL_MODE_CURSOR2: u32 = _iowr::<DrmModeCursor2>(DRM_IOCTL_BASE, 0xBB);

// ----------------------------------------------------------------------------
// Capabilities (verified against /usr/include/drm/drm.h)
// ----------------------------------------------------------------------------

pub const DRM_CAP_DUMB_BUFFER: u64 = 0x1;
pub const DRM_CAP_VBLANK_HIGH_CRTC: u64 = 0x2;
pub const DRM_CAP_DUMB_PREFERRED_DEPTH: u64 = 0x3;
pub const DRM_CAP_DUMB_PREFER_SHADOW: u64 = 0x4;
pub const DRM_CAP_PRIME: u64 = 0x5;
pub const DRM_CAP_TIMESTAMP_MONOTONIC: u64 = 0x6;
pub const DRM_CAP_ASYNC_PAGE_FLIP: u64 = 0x7;
pub const DRM_CAP_CURSOR_WIDTH: u64 = 0x8;
pub const DRM_CAP_CURSOR_HEIGHT: u64 = 0x9;
pub const DRM_CAP_ADDFB2_MODIFIERS: u64 = 0x10;
pub const DRM_CAP_PAGE_FLIP_TARGET: u64 = 0x11;
pub const DRM_CAP_CRTC_IN_VBLANK_EVENT: u64 = 0x12;
pub const DRM_CAP_SYNCOBJ: u64 = 0x13;
pub const DRM_CAP_SYNCOBJ_TIMELINE: u64 = 0x14;

pub const DRM_PRIME_CAP_IMPORT: u64 = 0x1;
pub const DRM_PRIME_CAP_EXPORT: u64 = 0x2;

/// `DRM_CLIENT_CAP_*` — `DRM_IOCTL_SET_CLIENT_CAP`.
pub const DRM_CLIENT_CAP_STEREO_3D: u64 = 1;
pub const DRM_CLIENT_CAP_UNIVERSAL_PLANES: u64 = 2;
pub const DRM_CLIENT_CAP_ATOMIC: u64 = 3;
pub const DRM_CLIENT_CAP_ASPECT_RATIO: u64 = 4;
pub const DRM_CLIENT_CAP_WRITEBACK_CONNECTORS: u64 = 5;
pub const DRM_CLIENT_CAP_NO_CONNECTOR_MAPPING: u64 = 6;
pub const DRM_CLIENT_CAP_NO_FB_MODIFIERS: u64 = 7;

// ----------------------------------------------------------------------------
// KMS structures (verified against /usr/include/drm/drm_mode.h)
// ----------------------------------------------------------------------------

/// `struct drm_mode_modeinfo` — wire format of a display mode (68 bytes).
#[repr(C)]
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable, Default)]
pub struct DrmModeModeInfo {
    pub clock: u32,
    pub hdisplay: u16,
    pub hsync_start: u16,
    pub hsync_end: u16,
    pub htotal: u16,
    pub hskew: u16,
    pub vdisplay: u16,
    pub vsync_start: u16,
    pub vsync_end: u16,
    pub vtotal: u16,
    pub vscan: u16,
    pub vrefresh: u32,
    pub flags: u32,
    pub type_: u32,
    pub name: [u8; DRM_DISPLAY_MODE_LEN],
}

/// `DRM_DISPLAY_MODE_LEN` from `drm_mode.h`.
pub const DRM_DISPLAY_MODE_LEN: usize = 32;

/// `DRM_MODE_TYPE_PREFERRED` — mode user space should pick by default.
pub const DRM_MODE_TYPE_PREFERRED: u32 = 1 << 3;
/// `DRM_MODE_TYPE_DRIVER` — mode reported by the display driver.
pub const DRM_MODE_TYPE_DRIVER: u32 = 1 << 6;

/// `DRM_MODE_FLAG_*` — sync polarity bits in `DrmModeModeInfo::flags`.
pub const DRM_MODE_FLAG_PHSYNC: u32 = 1 << 0;
pub const DRM_MODE_FLAG_NHSYNC: u32 = 1 << 1;
pub const DRM_MODE_FLAG_PVSYNC: u32 = 1 << 2;
pub const DRM_MODE_FLAG_NVSYNC: u32 = 1 << 3;

/// `enum drm_connector_status` — `drm_mode_get_connector::connection`.
pub const DRM_MODE_CONNECTED: u32 = 1;
pub const DRM_MODE_DISCONNECTED: u32 = 2;
pub const DRM_MODE_UNKNOWN_CONNECTION: u32 = 3;

/// `DRM_MODE_ENCODER_VIRTUAL` — encoder class for virtual GPUs.
pub const DRM_MODE_ENCODER_VIRTUAL: u32 = 5;
/// `DRM_MODE_CONNECTOR_VIRTUAL` — connector class for virtual GPUs.
pub const DRM_MODE_CONNECTOR_VIRTUAL: u32 = 15;

/// `struct drm_mode_card_res` — `DRM_IOCTL_MODE_GETRESOURCES`.
///
/// Two-call protocol: user space sets each pointer/count to a buffer it is
/// willing to fill; the kernel only writes arrays whose count already
/// matches and otherwise just reports the count — plus the range of supported
/// framebuffer dimensions.
#[repr(C)]
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable, Default)]
pub struct DrmModeCardRes {
    pub fb_id_ptr: usize,
    pub crtc_id_ptr: usize,
    pub connector_id_ptr: usize,
    pub encoder_id_ptr: usize,
    pub count_fbs: u32,
    pub count_crtcs: u32,
    pub count_connectors: u32,
    pub count_encoders: u32,
    pub min_width: u32,
    pub max_width: u32,
    pub min_height: u32,
    pub max_height: u32,
}

/// `struct drm_mode_crtc` — `GETCRTC`/`SETCRTC`.
#[repr(C)]
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable, Default)]
pub struct DrmModeCrtc {
    /// In: pointer to a `__u32` connector-id array for `SETCRTC`.
    pub set_connectors_ptr: usize,
    pub count_connectors: u32,
    pub crtc_id: u32,
    /// Set: current/user-requested framebuffer (0 disables the CRTC).
    pub fb_id: u32,
    pub x: u32,
    pub y: u32,
    pub gamma_size: u32,
    /// In for `SETCRTC`: 1 when `mode` is meaningful.
    pub mode_valid: u32,
    pub mode: DrmModeModeInfo,
}

/// `struct drm_mode_get_encoder` — `DRM_IOCTL_MODE_GETENCODER`.
#[repr(C)]
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable, Default)]
pub struct DrmModeGetEncoder {
    pub encoder_id: u32,
    pub encoder_type: u32,
    pub crtc_id: u32,
    pub possible_crtcs: u32,
    pub possible_clones: u32,
}

/// `struct drm_mode_get_connector` — `DRM_IOCTL_MODE_GETCONNECTOR`.
#[repr(C)]
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable, Default)]
pub struct DrmModeGetConnector {
    pub encoders_ptr: usize,
    pub modes_ptr: usize,
    pub props_ptr: usize,
    pub prop_values_ptr: usize,
    pub count_modes: u32,
    pub count_props: u32,
    pub count_encoders: u32,
    pub encoder_id: u32,
    pub connector_id: u32,
    pub connector_type: u32,
    pub connector_type_id: u32,
    pub connection: u32,
    pub mm_width: u32,
    pub mm_height: u32,
    pub subpixel: u32,
    pub pad: u32,
}

/// `struct drm_mode_fb_cmd` — `GETFB`/`ADDFB` (legacy form).
#[repr(C)]
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable, Default)]
pub struct DrmModeFbCmd {
    pub fb_id: u32,
    pub width: u32,
    pub height: u32,
    pub pitch: u32,
    pub bpp: u32,
    pub depth: u32,
    pub handle: u32,
}

/// `struct drm_mode_fb_cmd2` — `ADDFB2` (fourcc multi-plane form).
#[repr(C)]
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable, Default)]
pub struct DrmModeFbCmd2 {
    pub fb_id: u32,
    pub width: u32,
    pub height: u32,
    /// FourCC format code (see `DRM_FORMAT_*`).
    pub pixel_format: u32,
    pub flags: u32,
    pub handles: [u32; 4],
    pub pitches: [u32; 4],
    pub offsets: [u32; 4],
    /// Explicit padding before the 8-byte aligned `modifier` array.
    pub pad: u32,
    pub modifier: [u64; 4],
}

/// `DRM_FORMAT_XRGB8888` (`'XR24'`): 32-bit linear RGB, X ignored.
pub const DRM_FORMAT_XRGB8888: u32 = 0x3432_5258;
/// `DRM_FORMAT_ARGB8888` (`'AR24'`): 32-bit linear RGB with alpha.
pub const DRM_FORMAT_ARGB8888: u32 = 0x3432_5241;
/// `DRM_FORMAT_RGB565` (`'RG16'`): 16-bit RGB 5:6:5.
pub const DRM_FORMAT_RGB565: u32 = 0x3631_4752;
/// `DRM_FORMAT_XRGB1555` (`'XR15'`): 16-bit RGB 5:5:5, X ignored.
pub const DRM_FORMAT_XRGB1555: u32 = 0x3531_5258;

/// `DRM_MODE_FB_MODIFIERS` — `drm_mode_fb_cmd2::flags`.
pub const DRM_MODE_FB_MODIFIERS: u32 = 1 << 1;

/// `struct drm_mode_crtc_page_flip` — `DRM_IOCTL_MODE_PAGE_FLIP`.
#[repr(C)]
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable, Default)]
pub struct DrmModeCrtcPageFlip {
    pub crtc_id: u32,
    pub fb_id: u32,
    pub flags: u32,
    pub reserved: u32,
    pub user_data: u64,
}

/// `struct drm_mode_fb_dirty_cmd` — `DRM_IOCTL_MODE_DIRTYFB`.
#[repr(C)]
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable, Default)]
pub struct DrmModeFbDirtyCmd {
    pub fb_id: u32,
    pub flags: u32,
    pub color: u32,
    pub num_clips: u32,
    pub clips_ptr: usize,
}

/// `struct drm_clip_rect` — clip rectangle passed in `DrmModeFbDirtyCmd::clips_ptr`.
#[repr(C)]
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable, Default)]
pub struct DrmClipRect {
    pub x1: u16,
    pub y1: u16,
    pub x2: u16,
    pub y2: u16,
}

/// Maximum number of dirty clip rectangles accepted in a single `DIRTYFB` call.
pub const DRM_MODE_FB_DIRTY_MAX_CLIPS: u32 = 256;

/// Cursor operation flags for `DRM_IOCTL_MODE_CURSOR` and `DRM_IOCTL_MODE_CURSOR2`.
pub const DRM_MODE_CURSOR_BO: u32 = 0x01;
pub const DRM_MODE_CURSOR_MOVE: u32 = 0x02;
pub const DRM_MODE_CURSOR_FLAGS: u32 = 0x03;

/// `struct drm_mode_cursor` — `DRM_IOCTL_MODE_CURSOR`.
#[repr(C)]
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable, Default)]
pub struct DrmModeCursor {
    pub flags: u32,
    pub crtc_id: u32,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub handle: u32,
}

/// `struct drm_mode_cursor2` — `DRM_IOCTL_MODE_CURSOR2`.
#[repr(C)]
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable, Default)]
pub struct DrmModeCursor2 {
    pub flags: u32,
    pub crtc_id: u32,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub handle: u32,
    pub hot_x: i32,
    pub hot_y: i32,
}

/// `struct drm_mode_set_plane` — `DRM_IOCTL_MODE_SETPLANE`.
#[repr(C)]
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable, Default)]
pub struct DrmModeSetPlane {
    pub plane_id: u32,
    pub crtc_id: u32,
    pub fb_id: u32,
    pub flags: u32,
    pub crtc_x: i32,
    pub crtc_y: i32,
    pub crtc_w: u32,
    pub crtc_h: u32,
    /// Source rectangle in 16.16 fixed point.
    pub src_x: u32,
    pub src_y: u32,
    pub src_h: u32,
    pub src_w: u32,
}
