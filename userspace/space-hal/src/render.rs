//! RenderBackend abstraction (OCP/DIP) + DRM/KMS double-buffered dumb-buffer backend.

use crate::drm_uapi::*;
use space_core::{simd, DamageRegion, Rect};
use std::io;
use std::os::raw::{c_int, c_void};
use std::ptr;

/// Abstract display/render target backend (Dependency Inversion Principle).
pub trait RenderBackend {
    fn width(&self) -> u32;
    fn height(&self) -> u32;
    fn stride_pixels(&self) -> u32;
    fn back_buffer_mut(&mut self) -> &mut [u32];
    fn present(&mut self, damage: &[Rect]) -> io::Result<()>;
}

struct DumbBuffer {
    handle: u32,
    fb_id: u32,
    pitch: u32,
    size: usize,
    map: *mut u32,
}

/// Direct hardware rendering backend using `/dev/dri/card*`, DRM/KMS,
/// `mmap`'d dumb buffers, dirty-rect `DIRTYFB`, and `PAGE_FLIP`.
pub struct DrmKmsBackend {
    fd: c_int,
    crtc_id: u32,
    connector_id: u32,
    mode: DrmModeModeInfo,
    buffers: [DumbBuffer; 2],
    front_idx: usize,
    shadow: Vec<u32>,
    prev_damage: Vec<Rect>,
    stride_px: u32,
}

impl DrmKmsBackend {
    pub fn open_default() -> io::Result<Self> {
        for idx in 0..4 {
            let path = format!("/dev/dri/card{}", idx);
            if let Ok(backend) = Self::open_path(&path) {
                return Ok(backend);
            }
        }
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            "No usable /dev/dri/card* KMS device found",
        ))
    }

    pub fn open_path(path: &str) -> io::Result<Self> {
        let fd = space_core::sys::open_path(path, space_core::sys::O_RDWR | space_core::sys::O_CLOEXEC, 0)?;
        let _ = unsafe { space_core::sys::ioctl(fd, DRM_IOCTL_SET_MASTER) };

        let mut res = DrmModeCardRes::default();
        if unsafe { space_core::sys::ioctl(fd, DRM_IOCTL_MODE_GETRESOURCES, &mut res) } < 0 {
            let err = io::Error::last_os_error();
            unsafe { space_core::sys::close(fd) };
            return Err(err);
        }

        let mut connectors = vec![0u32; res.count_connectors.max(1) as usize];
        let mut crtcs = vec![0u32; res.count_crtcs.max(1) as usize];
        let mut encoders = vec![0u32; res.count_encoders.max(1) as usize];
        res.connector_id_ptr = connectors.as_mut_ptr() as u64;
        res.crtc_id_ptr = crtcs.as_mut_ptr() as u64;
        res.encoder_id_ptr = encoders.as_mut_ptr() as u64;

        if unsafe { space_core::sys::ioctl(fd, DRM_IOCTL_MODE_GETRESOURCES, &mut res) } < 0 {
            let err = io::Error::last_os_error();
            unsafe { space_core::sys::close(fd) };
            return Err(err);
        }

        let mut chosen_conn = 0u32;
        let mut chosen_enc = 0u32;
        let mut chosen_mode = DrmModeModeInfo::default();

        for &conn_id in connectors.iter().take(res.count_connectors as usize) {
            let mut conn = DrmModeGetConnector {
                connector_id: conn_id,
                ..Default::default()
            };
            if unsafe { space_core::sys::ioctl(fd, DRM_IOCTL_MODE_GETCONNECTOR, &mut conn) } < 0 {
                continue;
            }
            if conn.connection != DRM_MODE_CONNECTED || conn.count_modes == 0 {
                continue;
            }
            let mut modes = vec![DrmModeModeInfo::default(); conn.count_modes as usize];
            let mut conn_encs = vec![0u32; conn.count_encoders.max(1) as usize];
            conn.modes_ptr = modes.as_mut_ptr() as u64;
            conn.encoders_ptr = conn_encs.as_mut_ptr() as u64;
            if unsafe { space_core::sys::ioctl(fd, DRM_IOCTL_MODE_GETCONNECTOR, &mut conn) } < 0 {
                continue;
            }
            chosen_conn = conn_id;
            chosen_enc = conn.encoder_id;
            if chosen_enc == 0 && !conn_encs.is_empty() {
                chosen_enc = conn_encs[0];
            }
            chosen_mode = modes[0];
            break;
        }

        if chosen_conn == 0 {
            unsafe { space_core::sys::close(fd) };
            return Err(io::Error::new(io::ErrorKind::NotFound, "No connected DRM display"));
        }

        let mut chosen_crtc = crtcs.first().copied().unwrap_or(0);
        if chosen_enc != 0 {
            let mut enc = DrmModeGetEncoder {
                encoder_id: chosen_enc,
                ..Default::default()
            };
            if unsafe { space_core::sys::ioctl(fd, DRM_IOCTL_MODE_GETENCODER, &mut enc) } == 0
                && enc.crtc_id != 0
            {
                chosen_crtc = enc.crtc_id;
            }
        }

        let w = chosen_mode.hdisplay as u32;
        let h = chosen_mode.vdisplay as u32;
        let buf0 = Self::create_dumb(fd, w, h)?;
        let buf1 = Self::create_dumb(fd, w, h)?;
        let stride_px = buf0.pitch / 4;

        let mut crtc = DrmModeCrtc {
            set_connectors_ptr: &chosen_conn as *const u32 as u64,
            count_connectors: 1,
            crtc_id: chosen_crtc,
            fb_id: buf0.fb_id,
            x: 0,
            y: 0,
            gamma_size: 0,
            mode_valid: 1,
            mode: chosen_mode,
        };
        if unsafe { space_core::sys::ioctl(fd, DRM_IOCTL_MODE_SETCRTC, &mut crtc) } < 0 {
            let err = io::Error::last_os_error();
            Self::destroy_dumb(fd, &buf0);
            Self::destroy_dumb(fd, &buf1);
            unsafe { space_core::sys::close(fd) };
            return Err(err);
        }

        Ok(Self {
            fd,
            crtc_id: chosen_crtc,
            connector_id: chosen_conn,
            mode: chosen_mode,
            buffers: [buf0, buf1],
            front_idx: 0,
            shadow: vec![0xFF14_1820; stride_px as usize * h as usize],
            prev_damage: Vec::new(),
            stride_px,
        })
    }

    fn create_dumb(fd: c_int, width: u32, height: u32) -> io::Result<DumbBuffer> {
        let mut create = DrmModeCreateDumb {
            height,
            width,
            bpp: 32,
            ..Default::default()
        };
        if unsafe { space_core::sys::ioctl(fd, DRM_IOCTL_MODE_CREATE_DUMB, &mut create) } < 0 {
            return Err(io::Error::last_os_error());
        }

        let mut fb = DrmModeFbCmd {
            width,
            height,
            pitch: create.pitch,
            bpp: 32,
            depth: 24,
            handle: create.handle,
            ..Default::default()
        };
        if unsafe { space_core::sys::ioctl(fd, DRM_IOCTL_MODE_ADDFB, &mut fb) } < 0 {
            let err = io::Error::last_os_error();
            let mut destroy = DrmModeDestroyDumb { handle: create.handle };
            unsafe { space_core::sys::ioctl(fd, DRM_IOCTL_MODE_DESTROY_DUMB, &mut destroy) };
            return Err(err);
        }

        let mut map_req = DrmModeMapDumb {
            handle: create.handle,
            ..Default::default()
        };
        if unsafe { space_core::sys::ioctl(fd, DRM_IOCTL_MODE_MAP_DUMB, &mut map_req) } < 0 {
            let err = io::Error::last_os_error();
            unsafe { space_core::sys::ioctl(fd, DRM_IOCTL_MODE_RMFB, &fb.fb_id) };
            let mut destroy = DrmModeDestroyDumb { handle: create.handle };
            unsafe { space_core::sys::ioctl(fd, DRM_IOCTL_MODE_DESTROY_DUMB, &mut destroy) };
            return Err(err);
        }

        let map = unsafe {
            space_core::sys::mmap(
                ptr::null_mut(),
                create.size as usize,
                space_core::sys::PROT_READ | space_core::sys::PROT_WRITE,
                space_core::sys::MAP_SHARED,
                fd,
                map_req.offset as i64,
            )
        };
        if map == space_core::sys::MAP_FAILED {
            let err = io::Error::last_os_error();
            unsafe { space_core::sys::ioctl(fd, DRM_IOCTL_MODE_RMFB, &fb.fb_id) };
            let mut destroy = DrmModeDestroyDumb { handle: create.handle };
            unsafe { space_core::sys::ioctl(fd, DRM_IOCTL_MODE_DESTROY_DUMB, &mut destroy) };
            return Err(err);
        }

        Ok(DumbBuffer {
            handle: create.handle,
            fb_id: fb.fb_id,
            pitch: create.pitch,
            size: create.size as usize,
            map: map as *mut u32,
        })
    }

    fn destroy_dumb(fd: c_int, buf: &DumbBuffer) {
        unsafe {
            space_core::sys::munmap(buf.map as *mut c_void, buf.size);
            space_core::sys::ioctl(fd, DRM_IOCTL_MODE_RMFB, &buf.fb_id);
            let mut destroy = DrmModeDestroyDumb { handle: buf.handle };
            space_core::sys::ioctl(fd, DRM_IOCTL_MODE_DESTROY_DUMB, &mut destroy);
        }
    }
}

impl RenderBackend for DrmKmsBackend {
    #[inline]
    fn width(&self) -> u32 {
        self.mode.hdisplay as u32
    }

    #[inline]
    fn height(&self) -> u32 {
        self.mode.vdisplay as u32
    }

    #[inline]
    fn stride_pixels(&self) -> u32 {
        self.stride_px
    }

    #[inline]
    fn back_buffer_mut(&mut self) -> &mut [u32] {
        &mut self.shadow
    }

    fn present(&mut self, damage: &[Rect]) -> io::Result<()> {
        if damage.is_empty() && self.prev_damage.is_empty() {
            return Ok(());
        }
        let back_idx = 1 - self.front_idx;
        let back_buf = &self.buffers[back_idx];
        let stride = self.stride_px as usize;
        let h = self.height() as i32;
        let w = self.width() as i32;
        let dst_slice = unsafe { std::slice::from_raw_parts_mut(back_buf.map, back_buf.size / 4) };

        // Sync both the previous frame's damage (which only went to the current front buffer)
        // and this frame's damage from `shadow` into `back_buf`, preventing stale 2-frame-old
        // pixels from flickering or leaving cursor/drag trails on page flip.
        let bounds = Rect::new(0, 0, w, h);
        let mut sync_region = DamageRegion::new();
        for &r in &self.prev_damage {
            sync_region.add_clipped(r, bounds);
        }
        for &r in damage {
            sync_region.add_clipped(r, bounds);
        }

        let mut clips = Vec::with_capacity(sync_region.rects().len());
        for &c in sync_region.rects() {
            for row in c.y..c.bottom() {
                let off = row as usize * stride + c.x as usize;
                let len = c.w as usize;
                simd::blit_opaque_row(&mut dst_slice[off..off + len], &self.shadow[off..off + len]);
            }
            clips.push(DrmClipRect {
                x1: c.x as u16,
                y1: c.y as u16,
                x2: c.right() as u16,
                y2: c.bottom() as u16,
            });
        }

        let mut flip = DrmModeCrtcPageFlip {
            crtc_id: self.crtc_id,
            fb_id: back_buf.fb_id,
            flags: 0,
            reserved: 0,
            user_data: 0,
        };
        if unsafe { space_core::sys::ioctl(self.fd, DRM_IOCTL_MODE_PAGE_FLIP, &mut flip) } < 0 {
            // Fallback to SETCRTC if driver does not support async page flip
            let mut crtc = DrmModeCrtc {
                set_connectors_ptr: &self.connector_id as *const u32 as u64,
                count_connectors: 1,
                crtc_id: self.crtc_id,
                fb_id: back_buf.fb_id,
                x: 0,
                y: 0,
                gamma_size: 0,
                mode_valid: 1,
                mode: self.mode,
            };
            let _ = unsafe { space_core::sys::ioctl(self.fd, DRM_IOCTL_MODE_SETCRTC, &mut crtc) };
        }

        if !clips.is_empty() {
            let mut dirty = DrmModeFbDirtyCmd {
                fb_id: back_buf.fb_id,
                flags: 0,
                color: 0,
                num_clips: clips.len() as u32,
                clips_ptr: clips.as_ptr() as u64,
            };
            let _ = unsafe { space_core::sys::ioctl(self.fd, DRM_IOCTL_MODE_DIRTYFB, &mut dirty) };
        }

        self.front_idx = back_idx;
        self.prev_damage.clear();
        self.prev_damage.extend_from_slice(damage);
        Ok(())
    }
}

impl Drop for DrmKmsBackend {
    fn drop(&mut self) {
        Self::destroy_dumb(self.fd, &self.buffers[0]);
        Self::destroy_dumb(self.fd, &self.buffers[1]);
        unsafe {
            space_core::sys::ioctl(self.fd, DRM_IOCTL_DROP_MASTER);
            space_core::sys::close(self.fd);
        }
    }
}

/// In-memory headless backend for automated testing without DRM hardware.
pub struct HeadlessBackend {
    width: u32,
    height: u32,
    buffer: Vec<u32>,
    pub presents: usize,
}

impl HeadlessBackend {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            buffer: vec![0xFF00_0000; width as usize * height as usize],
            presents: 0,
        }
    }
}

impl RenderBackend for HeadlessBackend {
    fn width(&self) -> u32 {
        self.width
    }
    fn height(&self) -> u32 {
        self.height
    }
    fn stride_pixels(&self) -> u32 {
        self.width
    }
    fn back_buffer_mut(&mut self) -> &mut [u32] {
        &mut self.buffer
    }
    fn present(&mut self, damage: &[Rect]) -> io::Result<()> {
        if !damage.is_empty() {
            self.presents += 1;
        }
        Ok(())
    }
}
