//! Linux / PetraOS DRM & KMS UAPI definitions (zero external crates).

#![allow(dead_code)]

use std::os::raw::c_ulong;

const fn ioc(dir: u32, ty: u32, nr: u32, size: usize) -> c_ulong {
    (((dir) << 30) | ((size as u32) << 16) | ((ty) << 8) | (nr)) as c_ulong
}

const fn iowr<T>(ty: u32, nr: u32) -> c_ulong {
    ioc(3, ty, nr, std::mem::size_of::<T>())
}

const fn iow<T>(ty: u32, nr: u32) -> c_ulong {
    ioc(1, ty, nr, std::mem::size_of::<T>())
}

const fn io(ty: u32, nr: u32) -> c_ulong {
    ioc(0, ty, nr, 0)
}

const DRM_BASE: u32 = 0x64;

pub const DRM_MODE_TYPE_PREFERRED: u32 = 1 << 3;

pub const DRM_IOCTL_SET_MASTER: c_ulong = io(DRM_BASE, 0x1E);
pub const DRM_IOCTL_DROP_MASTER: c_ulong = io(DRM_BASE, 0x1F);
pub const DRM_IOCTL_MODE_GETRESOURCES: c_ulong = iowr::<DrmModeCardRes>(DRM_BASE, 0xA0);
pub const DRM_IOCTL_MODE_GETCRTC: c_ulong = iowr::<DrmModeCrtc>(DRM_BASE, 0xA1);
pub const DRM_IOCTL_MODE_SETCRTC: c_ulong = iowr::<DrmModeCrtc>(DRM_BASE, 0xA2);
pub const DRM_IOCTL_MODE_GETENCODER: c_ulong = iowr::<DrmModeGetEncoder>(DRM_BASE, 0xA6);
pub const DRM_IOCTL_MODE_GETCONNECTOR: c_ulong = iowr::<DrmModeGetConnector>(DRM_BASE, 0xA7);
pub const DRM_IOCTL_MODE_ADDFB: c_ulong = iowr::<DrmModeFbCmd>(DRM_BASE, 0xAE);
pub const DRM_IOCTL_MODE_RMFB: c_ulong = iowr::<u32>(DRM_BASE, 0xAF);
pub const DRM_IOCTL_MODE_PAGE_FLIP: c_ulong = iowr::<DrmModeCrtcPageFlip>(DRM_BASE, 0xB0);
pub const DRM_IOCTL_MODE_DIRTYFB: c_ulong = iowr::<DrmModeFbDirtyCmd>(DRM_BASE, 0xB1);
pub const DRM_IOCTL_MODE_CREATE_DUMB: c_ulong = iowr::<DrmModeCreateDumb>(DRM_BASE, 0xB2);
pub const DRM_IOCTL_MODE_MAP_DUMB: c_ulong = iowr::<DrmModeMapDumb>(DRM_BASE, 0xB3);
pub const DRM_IOCTL_MODE_DESTROY_DUMB: c_ulong = iowr::<DrmModeDestroyDumb>(DRM_BASE, 0xB4);

pub const DRM_MODE_CONNECTED: u32 = 1;
pub const DRM_MODE_PAGE_FLIP_EVENT: u32 = 0x01;

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
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
    pub ty: u32,
    pub name: [u8; 32],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct DrmModeCardRes {
    pub fb_id_ptr: u64,
    pub crtc_id_ptr: u64,
    pub connector_id_ptr: u64,
    pub encoder_id_ptr: u64,
    pub count_fbs: u32,
    pub count_crtcs: u32,
    pub count_connectors: u32,
    pub count_encoders: u32,
    pub min_width: u32,
    pub max_width: u32,
    pub min_height: u32,
    pub max_height: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct DrmModeGetConnector {
    pub encoders_ptr: u64,
    pub modes_ptr: u64,
    pub props_ptr: u64,
    pub prop_values_ptr: u64,
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

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct DrmModeGetEncoder {
    pub encoder_id: u32,
    pub encoder_type: u32,
    pub crtc_id: u32,
    pub possible_crtcs: u32,
    pub possible_clones: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct DrmModeCrtc {
    pub set_connectors_ptr: u64,
    pub count_connectors: u32,
    pub crtc_id: u32,
    pub fb_id: u32,
    pub x: u32,
    pub y: u32,
    pub gamma_size: u32,
    pub mode_valid: u32,
    pub mode: DrmModeModeInfo,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct DrmModeCreateDumb {
    pub height: u32,
    pub width: u32,
    pub bpp: u32,
    pub flags: u32,
    pub handle: u32,
    pub pitch: u32,
    pub size: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct DrmModeMapDumb {
    pub handle: u32,
    pub pad: u32,
    pub offset: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct DrmModeDestroyDumb {
    pub handle: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct DrmModeFbCmd {
    pub fb_id: u32,
    pub width: u32,
    pub height: u32,
    pub pitch: u32,
    pub bpp: u32,
    pub depth: u32,
    pub handle: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct DrmModeCrtcPageFlip {
    pub crtc_id: u32,
    pub fb_id: u32,
    pub flags: u32,
    pub reserved: u32,
    pub user_data: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct DrmClipRect {
    pub x1: u16,
    pub y1: u16,
    pub x2: u16,
    pub y2: u16,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct DrmModeFbDirtyCmd {
    pub fb_id: u32,
    pub flags: u32,
    pub color: u32,
    pub num_clips: u32,
    pub clips_ptr: u64,
}
