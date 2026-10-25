// SPDX-License-Identifier: GPL-2.0

//! Mode-setting ioctl handlers (`0xA0..0xB8`), the legacy control path.
//!
//! Array-carrying requests follow the two-call protocol Linux documents:
//! counts are always reported, and arrays are only written when the
//! user-provided count already matches the resource count, which lets
//! clients size their buffers safely.

use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;

use super::state::{Framebuffer, KmsState};
use crate::drivers::drm::{DrmFile, GemObject, read_uapi, write_uapi};
use crate::proc::thread::Thread;
use crate::uapi::drm;
use crate::uapi::errno::{Errno, Result};
use crate::utils::userspace::UserPtr;
use zerocopy::IntoBytes;

/// Dispatches a KMS-range ioctl; reached only for devices with a KMS.
pub fn dispatch(kms: &KmsState, file: &mut DrmFile, cmd: u32, arg: usize) -> Result<usize> {
    match cmd {
        drm::DRM_IOCTL_MODE_GETRESOURCES => get_resources(kms, arg),
        drm::DRM_IOCTL_MODE_GETCRTC => get_crtc(kms, arg),
        drm::DRM_IOCTL_MODE_SETCRTC => set_crtc(kms, arg),
        drm::DRM_IOCTL_MODE_CURSOR => mode_cursor(kms, file, arg),
        drm::DRM_IOCTL_MODE_GETENCODER => get_encoder(kms, arg),
        drm::DRM_IOCTL_MODE_GETCONNECTOR => get_connector(kms, arg),
        drm::DRM_IOCTL_MODE_GETFB => get_fb(kms, file, arg),
        drm::DRM_IOCTL_MODE_ADDFB => add_fb(kms, file, arg),
        drm::DRM_IOCTL_MODE_RMFB => rm_fb(kms, arg),
        drm::DRM_IOCTL_MODE_PAGE_FLIP => page_flip(kms, file, arg),
        drm::DRM_IOCTL_MODE_DIRTYFB => dirty_fb(kms, arg),
        drm::DRM_IOCTL_MODE_SETPLANE => set_plane(kms, arg),
        drm::DRM_IOCTL_MODE_ADDFB2 => add_fb2(kms, file, arg),
        drm::DRM_IOCTL_MODE_CURSOR2 => mode_cursor2(kms, file, arg),
        _ => crate::return_errno!(ENOTTY, "ioctl does not exist on this device"),
    }
}

/// Marshals `bytes` into a user buffer at `ptr`.
///
/// A null pointer or an empty payload is the two-call protocol's "no array"
/// reply, not a fault.
fn write_user_bytes(ptr: usize, bytes: &[u8]) -> Result<()> {
    if ptr == 0 || bytes.is_empty() {
        return Ok(());
    }
    let vmar = Thread::proc_or_esrch()?.vmspace();
    UserPtr::new(ptr).write_bytes(&vmar, bytes)
}

/// Marshals `[]` id arrays in the ioctl two-call protocol usage.
fn write_user_ids(ptr: usize, ids: &[u32]) -> Result<()> {
    let mut bytes = Vec::with_capacity(ids.len() * 4);
    for id in ids {
        bytes.extend_from_slice(&id.to_ne_bytes());
    }
    write_user_bytes(ptr, &bytes)
}

fn write_user_modes(ptr: usize, modes: &[drm::DrmModeModeInfo]) -> Result<()> {
    let mut bytes = Vec::with_capacity(modes.len() * core::mem::size_of::<drm::DrmModeModeInfo>());
    for mode in modes {
        bytes.extend_from_slice(mode.as_bytes());
    }
    write_user_bytes(ptr, &bytes)
}

/// Reads `len` `__u32` elements from the user array at `ptr`.
fn read_user_ids(ptr: usize, len: usize) -> Result<Vec<u32>> {
    if len == 0 {
        return Ok(Vec::new());
    }
    if ptr == 0 {
        crate::return_errno!(EFAULT, "null id array");
    }
    let vmar = Thread::proc_or_esrch()?.vmspace();
    let mut bytes = vec![0u8; len * 4];
    UserPtr::new(ptr).read_bytes(&vmar, &mut bytes)?;
    Ok(bytes
        .chunks_exact(4)
        .map(|chunk| u32::from_ne_bytes(chunk.try_into().unwrap()))
        .collect())
}

fn get_resources(kms: &KmsState, arg: usize) -> Result<usize> {
    let mut res = read_uapi::<drm::DrmModeCardRes>(arg)?;

    let crtc_ids = kms.crtc_ids();
    let connector_ids = kms.connector_ids();
    let encoder_ids = kms.encoder_ids();
    let fb_ids = kms.fb_ids();

    let mut limits = (u32::MAX, 0u32, u32::MAX, 0u32);
    for pipeline in 0..crtc_ids.len() {
        let (min_w, max_w, min_h, max_h) = kms.pipeline_geometry(pipeline);
        limits.0 = limits.0.min(min_w);
        limits.1 = limits.1.max(max_w);
        limits.2 = limits.2.min(min_h);
        limits.3 = limits.3.max(max_h);
    }

    if res.count_fbs == fb_ids.len() as u32 {
        write_user_ids(res.fb_id_ptr, &fb_ids)?;
    }
    if res.count_crtcs == crtc_ids.len() as u32 {
        write_user_ids(res.crtc_id_ptr, &crtc_ids)?;
    }
    if res.count_connectors == connector_ids.len() as u32 {
        write_user_ids(res.connector_id_ptr, &connector_ids)?;
    }
    if res.count_encoders == encoder_ids.len() as u32 {
        write_user_ids(res.encoder_id_ptr, &encoder_ids)?;
    }

    res.count_fbs = fb_ids.len() as u32;
    res.count_crtcs = crtc_ids.len() as u32;
    res.count_connectors = connector_ids.len() as u32;
    res.count_encoders = encoder_ids.len() as u32;
    res.min_width = limits.0;
    res.max_width = limits.1;
    res.min_height = limits.2;
    res.max_height = limits.3;
    write_uapi(arg, &res)?;
    Ok(0)
}

fn get_crtc(kms: &KmsState, arg: usize) -> Result<usize> {
    let mut req = read_uapi::<drm::DrmModeCrtc>(arg)?;
    let Some(pipeline) = kms.pipeline_of_crtc(req.crtc_id) else {
        crate::return_errno!(EINVAL, "unknown CRTC id");
    };

    let connector = kms.connector_ids()[pipeline];
    if req.count_connectors == 1 {
        write_user_ids(req.set_connectors_ptr, &[connector])?;
    }
    req.count_connectors = 1;
    req.fb_id = kms.active_fb(pipeline).map(|fb| fb.id).unwrap_or(0);
    req.x = 0;
    req.y = 0;
    req.gamma_size = 0;
    req.mode_valid = if req.fb_id != 0 { 1 } else { 0 };
    req.mode = drm_mode(kms.pipeline_modes(pipeline));
    write_uapi(arg, &req)?;
    Ok(0)
}

/// The first (current) mode of a pipeline, defaulting to an empty one.
fn drm_mode(modes: &[drm::DrmModeModeInfo]) -> drm::DrmModeModeInfo {
    modes.first().copied().unwrap_or_default()
}

fn set_crtc(kms: &KmsState, arg: usize) -> Result<usize> {
    let req = read_uapi::<drm::DrmModeCrtc>(arg)?;
    let Some(pipeline) = kms.pipeline_of_crtc(req.crtc_id) else {
        crate::return_errno!(EINVAL, "unknown CRTC id");
    };

    if req.count_connectors > 0 {
        let connectors = read_user_ids(req.set_connectors_ptr, req.count_connectors as usize)?;
        let own = kms.connector_ids()[pipeline];
        if connectors.iter().any(|id| *id != own) {
            crate::return_errno!(EINVAL, "connector is not driven by this CRTC");
        }
    }

    if req.fb_id == 0 {
        kms.disable(pipeline)?;
        return Ok(0);
    }

    let fb = kms.fb(req.fb_id).ok_or(Errno::EINVAL)?;
    let mode = if req.mode_valid == 1 {
        req.mode
    } else {
        drm_mode(kms.pipeline_modes(pipeline))
    };
    validate_scanout(&fb, &mode)?;
    kms.set_scanout(pipeline, fb, &mode)?;
    Ok(0)
}

/// Checks that a framebuffer can serve `mode` as its scanout source.
fn validate_scanout(fb: &Framebuffer, mode: &drm::DrmModeModeInfo) -> Result<()> {
    if fb.width < mode.hdisplay as u32
        || fb.height < mode.vdisplay as u32
        || fb.pitch as u64 * fb.height as u64 > fb.object.size() as u64
    {
        crate::return_errno!(EINVAL, "framebuffer does not cover the mode");
    }
    Ok(())
}

fn get_encoder(kms: &KmsState, arg: usize) -> Result<usize> {
    let mut req = read_uapi::<drm::DrmModeGetEncoder>(arg)?;
    let Some(pipeline) = kms.pipeline_of_encoder(req.encoder_id) else {
        crate::return_errno!(EINVAL, "unknown encoder id");
    };
    let (kind, crtc_id, possible) = kms.pipeline_encoder(pipeline);
    req.encoder_type = kind;
    req.crtc_id = crtc_id;
    req.possible_crtcs = possible;
    req.possible_clones = 0;
    write_uapi(arg, &req)?;
    Ok(0)
}

fn get_connector(kms: &KmsState, arg: usize) -> Result<usize> {
    let mut req = read_uapi::<drm::DrmModeGetConnector>(arg)?;
    let Some(pipeline) = kms.pipeline_of_connector(req.connector_id) else {
        crate::return_errno!(EINVAL, "unknown connector id");
    };
    let (kind, type_id, mm_width, mm_height) = kms.pipeline_connector(pipeline);

    let modes = kms.pipeline_modes(pipeline).to_vec();
    if req.count_modes == modes.len() as u32 {
        write_user_modes(req.modes_ptr, &modes)?;
    }
    req.count_modes = modes.len() as u32;

    if req.count_encoders == 1 {
        let encoder_id = kms.encoder_ids()[pipeline];
        write_user_ids(req.encoders_ptr, &[encoder_id])?;
        req.encoder_id = encoder_id;
    }
    req.count_encoders = 1;
    req.connector_type = kind;
    req.connector_type_id = type_id;
    req.connection = drm::DRM_MODE_CONNECTED;
    req.mm_width = mm_width;
    req.mm_height = mm_height;
    req.subpixel = 0;
    req.pad = 0;
    // Property lists are not exposed yet.
    req.count_props = 0;
    req.props_ptr = 0;
    req.prop_values_ptr = 0;
    write_uapi(arg, &req)?;
    Ok(0)
}

fn get_fb(kms: &KmsState, file: &mut DrmFile, arg: usize) -> Result<usize> {
    let mut req = read_uapi::<drm::DrmModeFbCmd>(arg)?;
    let fb = kms.fb(req.fb_id).ok_or(Errno::EINVAL)?;
    let (bpp, depth) = format_bpp_depth(fb.format);
    req.width = fb.width;
    req.height = fb.height;
    req.pitch = fb.pitch;
    req.bpp = bpp;
    req.depth = depth;
    req.handle = file.handles.handle_of(&fb.object).unwrap_or(0);
    write_uapi(arg, &req)?;
    Ok(0)
}

fn format_bpp_depth(format: u32) -> (u32, u32) {
    match format {
        drm::DRM_FORMAT_XRGB8888 => (32, 24),
        drm::DRM_FORMAT_ARGB8888 => (32, 32),
        drm::DRM_FORMAT_RGB565 => (16, 16),
        drm::DRM_FORMAT_XRGB1555 => (16, 15),
        _ => (0, 0),
    }
}

/// Registers a validated framebuffer geometry and buffer combination.
fn fb_from_object(
    kms: &KmsState,
    object: Arc<GemObject>,
    width: u32,
    height: u32,
    pitch: u32,
    format: u32,
) -> Result<u32> {
    if width == 0 || height == 0 || pitch == 0 {
        crate::return_errno!(EINVAL, "invalid framebuffer geometry");
    }
    let fb = Framebuffer {
        id: 0,
        object,
        width,
        height,
        pitch,
        format,
    };
    let bytes_pp = (fb.bpp() / 8) as u64;
    if (pitch as u64) < width as u64 * bytes_pp
        || (pitch as u64 * height as u64) > fb.object.size() as u64
    {
        crate::return_errno!(EINVAL, "buffer smaller than the framebuffer");
    }
    Ok(kms.add_fb(fb))
}

/// Legacy `depth`/`bpp` pairs, matching Linux's table.
fn format_of_legacy(bpp: u32, depth: u32) -> Option<u32> {
    match (bpp, depth) {
        (32, 24) => Some(drm::DRM_FORMAT_XRGB8888),
        (32, 32) => Some(drm::DRM_FORMAT_ARGB8888),
        (16, 15) => Some(drm::DRM_FORMAT_XRGB1555),
        (16, 16) => Some(drm::DRM_FORMAT_RGB565),
        _ => None,
    }
}

fn add_fb(kms: &KmsState, file: &mut DrmFile, arg: usize) -> Result<usize> {
    let mut req = read_uapi::<drm::DrmModeFbCmd>(arg)?;
    let object = file.handles.get(req.handle).ok_or(Errno::EINVAL)?;
    let Some(format) = format_of_legacy(req.bpp, req.depth) else {
        crate::return_errno!(EINVAL, "unsupported depth/bpp pair");
    };
    req.fb_id = fb_from_object(kms, object, req.width, req.height, req.pitch, format)?;
    write_uapi(arg, &req)?;
    Ok(0)
}

fn add_fb2(kms: &KmsState, file: &mut DrmFile, arg: usize) -> Result<usize> {
    let mut req = read_uapi::<drm::DrmModeFbCmd2>(arg)?;
    // Single-plane, linear formats only: plane 1..3 must be unused and
    // format modifiers unsupported.
    if req.handles[1] != 0 || req.pitches[1] != 0 || req.offsets[1] != 0 {
        crate::return_errno!(EINVAL, "multi-plane formats are not supported");
    }
    if req.flags & drm::DRM_MODE_FB_MODIFIERS != 0 && req.modifier[0] != 0 {
        crate::return_errno!(EINVAL, "format modifiers are not supported");
    }
    let format = req.pixel_format;
    if format_bpp_depth(format) == (0, 0) {
        crate::return_errno!(EINVAL, "unsupported pixel format");
    }

    let object = file.handles.get(req.handles[0]).ok_or(Errno::EINVAL)?;
    req.fb_id = fb_from_object(kms, object, req.width, req.height, req.pitches[0], format)?;
    write_uapi(arg, &req)?;
    Ok(0)
}

fn rm_fb(kms: &KmsState, arg: usize) -> Result<usize> {
    let id = read_uapi::<u32>(arg)?;
    // Linux drops a framebuffer even when a CRTC shows it: it takes the
    // CRTC down along with the buffer, mirrored here.
    for pipeline in 0..kms.crtc_ids().len() {
        if kms.active_fb(pipeline).is_some_and(|fb| fb.id == id) {
            kms.disable(pipeline)?;
        }
    }
    let fb = kms.remove_fb(id)?;
    drop(fb); // releasing the Arc lets the object die once handles are closed
    write_uapi(arg, &id)?;
    Ok(0)
}

fn page_flip(kms: &KmsState, file: &mut DrmFile, arg: usize) -> Result<usize> {
    let req = read_uapi::<drm::DrmModeCrtcPageFlip>(arg)?;
    let Some(pipeline) = kms.pipeline_of_crtc(req.crtc_id) else {
        crate::return_errno!(EINVAL, "unknown CRTC id");
    };

    let mode = drm_mode(kms.pipeline_modes(pipeline));
    if req.fb_id == 0 {
        kms.disable(pipeline)?;
    } else {
        let fb = kms.fb(req.fb_id).ok_or(Errno::EINVAL)?;
        validate_scanout(&fb, &mode)?;
        kms.set_scanout(pipeline, fb, &mode)?;
    }

    if req.flags & 0x01 != 0 {
        let tv = crate::time::get_timeval();
        static SEQ: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(1);
        let seq = SEQ.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        let event = drm::DrmEventVblank {
            base: drm::DrmEvent {
                type_: drm::DRM_EVENT_FLIP_COMPLETE,
                length: core::mem::size_of::<drm::DrmEventVblank>() as u32,
            },
            user_data: req.user_data,
            tv_sec: tv.tv_sec as u32,
            tv_usec: tv.tv_usec as u32,
            sequence: seq,
            crtc_id: req.crtc_id,
        };
        file.queue_vblank_event(event);
    }

    Ok(0)
}

fn set_plane(kms: &KmsState, arg: usize) -> Result<usize> {
    let req = read_uapi::<drm::DrmModeSetPlane>(arg)?;
    let Some(pipeline) = kms.pipeline_of_plane(req.plane_id) else {
        crate::return_errno!(EINVAL, "unknown plane id");
    };
    let mode = drm_mode(kms.pipeline_modes(pipeline));

    if req.fb_id == 0 {
        kms.disable(pipeline)?;
    } else {
        let fb = kms.fb(req.fb_id).ok_or(Errno::EINVAL)?;
        validate_scanout(&fb, &mode)?;
        kms.set_scanout(pipeline, fb, &mode)?;
    }
    Ok(0)
}

fn dirty_fb(kms: &KmsState, arg: usize) -> Result<usize> {
    let req = read_uapi::<drm::DrmModeFbDirtyCmd>(arg)?;
    let fb = kms.fb(req.fb_id).ok_or(Errno::EINVAL)?;
    if req.num_clips > drm::DRM_MODE_FB_DIRTY_MAX_CLIPS {
        crate::return_errno!(EINVAL, "too many dirty clip rectangles");
    }

    if req.num_clips > 0 && req.clips_ptr != 0 {
        if let Some((x, y, width, height)) =
            read_clip_bounding_box(req.clips_ptr, req.num_clips as usize, fb.width, fb.height)?
        {
            let row_start = (y as usize)
                .saturating_mul(fb.pitch as usize)
                .min(fb.object.size());
            let row_end = (y.saturating_add(height) as usize)
                .saturating_mul(fb.pitch as usize)
                .min(fb.object.size());
            if row_start < row_end {
                fb.object.flush_pages(row_start..row_end);
            }
            kms.refresh_rect(&fb, x, y, width, height)?;
        }
        return Ok(0);
    }

    fb.object.flush_pages(0..fb.object.size());
    kms.refresh(&fb)?;
    Ok(0)
}

/// Reads `count` clip rectangles from `ptr` and computes their bounding box
/// clamped to `fb_width x fb_height`. Returns `Ok(None)` if all clips are empty.
fn read_clip_bounding_box(
    ptr: usize,
    count: usize,
    fb_width: u32,
    fb_height: u32,
) -> Result<Option<(u32, u32, u32, u32)>> {
    let clip_size = core::mem::size_of::<drm::DrmClipRect>();
    let vmar = Thread::proc_or_esrch()?.vmspace();
    let mut raw = vec![0u8; count * clip_size];
    UserPtr::new(ptr).read_bytes(&vmar, &mut raw)?;

    let mut min_x = fb_width;
    let mut min_y = fb_height;
    let mut max_x = 0u32;
    let mut max_y = 0u32;

    for chunk in raw.chunks_exact(clip_size) {
        let x1 = (u16::from_ne_bytes([chunk[0], chunk[1]]) as u32).min(fb_width);
        let y1 = (u16::from_ne_bytes([chunk[2], chunk[3]]) as u32).min(fb_height);
        let x2 = (u16::from_ne_bytes([chunk[4], chunk[5]]) as u32).min(fb_width);
        let y2 = (u16::from_ne_bytes([chunk[6], chunk[7]]) as u32).min(fb_height);
        if x1 < x2 && y1 < y2 {
            min_x = min_x.min(x1);
            min_y = min_y.min(y1);
            max_x = max_x.max(x2);
            max_y = max_y.max(y2);
        }
    }

    if min_x < max_x && min_y < max_y {
        Ok(Some((min_x, min_y, max_x - min_x, max_y - min_y)))
    } else {
        Ok(None)
    }
}

fn mode_cursor(kms: &KmsState, file: &mut DrmFile, arg: usize) -> Result<usize> {
    let req = read_uapi::<drm::DrmModeCursor>(arg)?;
    apply_cursor(
        kms,
        file,
        req.flags,
        req.crtc_id,
        req.x,
        req.y,
        req.width,
        req.height,
        req.handle,
        0,
        0,
    )
}

fn mode_cursor2(kms: &KmsState, file: &mut DrmFile, arg: usize) -> Result<usize> {
    let req = read_uapi::<drm::DrmModeCursor2>(arg)?;
    apply_cursor(
        kms,
        file,
        req.flags,
        req.crtc_id,
        req.x,
        req.y,
        req.width,
        req.height,
        req.handle,
        req.hot_x,
        req.hot_y,
    )
}

#[expect(clippy::too_many_arguments)]
fn apply_cursor(
    kms: &KmsState,
    file: &mut DrmFile,
    flags: u32,
    crtc_id: u32,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    handle: u32,
    hot_x: i32,
    hot_y: i32,
) -> Result<usize> {
    if flags & !drm::DRM_MODE_CURSOR_FLAGS != 0 || flags == 0 {
        crate::return_errno!(EINVAL, "invalid cursor flags");
    }
    let Some(pipeline) = kms.pipeline_of_crtc(crtc_id) else {
        crate::return_errno!(EINVAL, "unknown CRTC id");
    };

    if flags & drm::DRM_MODE_CURSOR_BO != 0 {
        let bo = if handle == 0 {
            None
        } else {
            let obj = file.handles.get(handle).ok_or(Errno::ENOENT)?;
            obj.flush_pages(0..obj.size());
            Some(obj)
        };
        kms.set_cursor(pipeline, bo.as_ref(), width, height, x, y, hot_x, hot_y)?;
    } else if flags & drm::DRM_MODE_CURSOR_MOVE != 0 {
        kms.move_cursor(pipeline, x, y)?;
    }

    Ok(0)
}
