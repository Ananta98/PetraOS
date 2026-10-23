// SPDX-License-Identifier: GPL-2.0

//! Intel `i915` GEM and driver-specific ioctl handlers (`0x40..0x9F`).
//!
//! Implements the `i915` GEM buffer lifecycle (`GEM_CREATE`, `GEM_PREAD`,
//! `GEM_PWRITE`, `GEM_MMAP_GTT`, `GEM_MMAP_OFFSET`, `GEM_SET_DOMAIN`,
//! `GEM_SW_FINISH`, `GEM_SET_TILING`, `GEM_GET_TILING`, `GEM_BUSY`, `GEM_WAIT`)
//! and device parameter queries (`GETPARAM`, `GET_RESET_STATS`) on top of
//! PetraOS's [`GemObject`] and [`DrmFile`] abstractions.

use alloc::vec;

use super::device::IntelGpu;
use crate::drivers::drm::{DrmDevice, DrmFile, read_uapi, write_uapi};
use crate::proc::thread::Thread;
use crate::uapi::drm;
use crate::uapi::errno::{Errno, Result};
use crate::utils::userspace::UserPtr;

/// Maximum single `PREAD` / `PWRITE` transfer size (64 MiB).
const MAX_GEM_IO_BYTES: usize = 64 * 1024 * 1024;

/// Dispatches an `i915` driver-specific ioctl (`0x40..0x9F`).
pub fn dispatch(gpu: &IntelGpu, file: &mut DrmFile, cmd: u32, arg: usize) -> Result<usize> {
    match cmd {
        drm::DRM_IOCTL_I915_GETPARAM => ioctl_getparam(gpu, arg),
        drm::DRM_IOCTL_I915_GEM_CREATE => ioctl_gem_create(gpu, file, arg),
        drm::DRM_IOCTL_I915_GEM_PREAD => ioctl_gem_pread(file, arg),
        drm::DRM_IOCTL_I915_GEM_PWRITE => ioctl_gem_pwrite(gpu, file, arg),
        drm::DRM_IOCTL_I915_GEM_MMAP_GTT => ioctl_gem_mmap_gtt(file, arg),
        drm::DRM_IOCTL_I915_GEM_MMAP_OFFSET => ioctl_gem_mmap_offset(file, arg),
        drm::DRM_IOCTL_I915_GEM_SET_DOMAIN => ioctl_gem_set_domain(gpu, file, arg),
        drm::DRM_IOCTL_I915_GEM_SW_FINISH => ioctl_gem_sw_finish(gpu, file, arg),
        drm::DRM_IOCTL_I915_GEM_SET_TILING => ioctl_gem_set_tiling(gpu, file, arg),
        drm::DRM_IOCTL_I915_GEM_GET_TILING => ioctl_gem_get_tiling(gpu, file, arg),
        drm::DRM_IOCTL_I915_GEM_BUSY => ioctl_gem_busy(file, arg),
        drm::DRM_IOCTL_I915_GEM_WAIT => ioctl_gem_wait(file, arg),
        drm::DRM_IOCTL_I915_GET_RESET_STATS => ioctl_get_reset_stats(arg),
        _ => crate::return_errno!(ENOTTY, "unsupported i915 ioctl"),
    }
}

fn ioctl_getparam(gpu: &IntelGpu, arg: usize) -> Result<usize> {
    let req = read_uapi::<drm::DrmI915GetParam>(arg)?;
    if req.value == 0 {
        crate::return_errno!(EFAULT, "null value pointer in I915_GETPARAM");
    }

    let val: i32 = match req.param {
        drm::I915_PARAM_CHIPSET_ID => gpu.device_id() as i32,
        drm::I915_PARAM_REVISION => gpu.revision() as i32,
        drm::I915_PARAM_IRQ_ACTIVE => 1,
        drm::I915_PARAM_ALLOW_BATCHBUFFER => 0,
        drm::I915_PARAM_HAS_GEM => 1,
        drm::I915_PARAM_HAS_COHERENT_RINGS => 1,
        drm::I915_PARAM_HAS_EXECBUF2 => 1,
        drm::I915_PARAM_HAS_LLC => 1,
        drm::I915_PARAM_HAS_WAIT_TIMEOUT => 1,
        drm::I915_PARAM_MMAP_VERSION => 1,
        drm::I915_PARAM_MMAP_GTT_VERSION => 4,
        drm::I915_PARAM_SUBSLICE_TOTAL => 3,
        drm::I915_PARAM_EU_TOTAL => 24,
        drm::I915_PARAM_MIN_EU_IN_POOL => 8,
        drm::I915_PARAM_HAS_EXEC_SOFTPIN => 1,
        drm::I915_PARAM_HAS_EXEC_NO_RELOC => 1,
        drm::I915_PARAM_HAS_EXEC_BATCH_FIRST => 1,
        drm::I915_PARAM_CS_TIMESTAMP_FREQUENCY => 12_000_000,
        _ => crate::return_errno!(EINVAL, "unknown i915 parameter"),
    };

    let vmar = Thread::proc_or_esrch()?.vmspace();
    UserPtr::new(req.value).write_val(&vmar, val)?;
    Ok(0)
}

fn ioctl_gem_create(gpu: &IntelGpu, file: &mut DrmFile, arg: usize) -> Result<usize> {
    let mut req = read_uapi::<drm::DrmI915GemCreate>(arg)?;
    let size = usize::try_from(req.size).map_err(|_| Errno::EINVAL)?;
    if size == 0 {
        crate::return_errno!(EINVAL, "i915 GEM object size cannot be zero");
    }

    let object = gpu.create_object(size)?;
    req.size = object.size() as u64;
    req.handle = file.handles.insert(object);
    write_uapi(arg, &req)?;
    Ok(0)
}

fn ioctl_gem_pread(file: &mut DrmFile, arg: usize) -> Result<usize> {
    let req = read_uapi::<drm::DrmI915GemPread>(arg)?;
    let object = file.handles.get(req.handle).ok_or(Errno::ENOENT)?;
    let offset = usize::try_from(req.offset).map_err(|_| Errno::EINVAL)?;
    let size = usize::try_from(req.size).map_err(|_| Errno::EINVAL)?;
    if size == 0 {
        return Ok(0);
    }
    if size > MAX_GEM_IO_BYTES || offset.checked_add(size).ok_or(Errno::EINVAL)? > object.size() {
        crate::return_errno!(EINVAL, "i915 GEM pread out of bounds");
    }
    if req.data_ptr == 0 {
        crate::return_errno!(EFAULT, "null data_ptr in i915 GEM pread");
    }

    object.flush_pages(offset..offset + size);
    let mut kbuf = vec![0u8; size];
    object.read_at(offset, &mut kbuf)?;

    let vmar = Thread::proc_or_esrch()?.vmspace();
    UserPtr::new(req.data_ptr).write_bytes(&vmar, &kbuf)?;
    Ok(0)
}

fn ioctl_gem_pwrite(gpu: &IntelGpu, file: &mut DrmFile, arg: usize) -> Result<usize> {
    let req = read_uapi::<drm::DrmI915GemPwrite>(arg)?;
    let object = file.handles.get(req.handle).ok_or(Errno::ENOENT)?;
    let offset = usize::try_from(req.offset).map_err(|_| Errno::EINVAL)?;
    let size = usize::try_from(req.size).map_err(|_| Errno::EINVAL)?;
    if size == 0 {
        return Ok(0);
    }
    if size > MAX_GEM_IO_BYTES || offset.checked_add(size).ok_or(Errno::EINVAL)? > object.size() {
        crate::return_errno!(EINVAL, "i915 GEM pwrite out of bounds");
    }
    if req.data_ptr == 0 {
        crate::return_errno!(EFAULT, "null data_ptr in i915 GEM pwrite");
    }

    let vmar = Thread::proc_or_esrch()?.vmspace();
    let mut kbuf = vec![0u8; size];
    UserPtr::new(req.data_ptr).read_bytes(&vmar, &mut kbuf)?;

    object.write_at(offset, &kbuf)?;
    object.invalidate_pages();
    gpu.flush_object_if_scanned_out(&object);
    Ok(0)
}

fn ioctl_gem_mmap_gtt(file: &mut DrmFile, arg: usize) -> Result<usize> {
    let mut req = read_uapi::<drm::DrmI915GemMmapGtt>(arg)?;
    let object = file.handles.get(req.handle).ok_or(Errno::ENOENT)?;
    req.offset = file.offsets.grant(object);
    write_uapi(arg, &req)?;
    Ok(0)
}

fn ioctl_gem_mmap_offset(file: &mut DrmFile, arg: usize) -> Result<usize> {
    let mut req = read_uapi::<drm::DrmI915GemMmapOffset>(arg)?;
    if req.extensions != 0 {
        crate::return_errno!(EINVAL, "i915 mmap_offset extensions are not supported");
    }
    let object = file.handles.get(req.handle).ok_or(Errno::ENOENT)?;
    req.offset = file.offsets.grant(object);
    write_uapi(arg, &req)?;
    Ok(0)
}

fn ioctl_gem_set_domain(gpu: &IntelGpu, file: &mut DrmFile, arg: usize) -> Result<usize> {
    let req = read_uapi::<drm::DrmI915GemSetDomain>(arg)?;
    let object = file.handles.get(req.handle).ok_or(Errno::ENOENT)?;
    object.flush_pages(0..object.size());
    gpu.flush_object_if_scanned_out(&object);
    Ok(0)
}

fn ioctl_gem_sw_finish(gpu: &IntelGpu, file: &mut DrmFile, arg: usize) -> Result<usize> {
    let req = read_uapi::<drm::DrmI915GemSwFinish>(arg)?;
    let object = file.handles.get(req.handle).ok_or(Errno::ENOENT)?;
    object.flush_pages(0..object.size());
    gpu.flush_object_if_scanned_out(&object);
    Ok(0)
}

fn ioctl_gem_set_tiling(gpu: &IntelGpu, file: &mut DrmFile, arg: usize) -> Result<usize> {
    let mut req = read_uapi::<drm::DrmI915GemTiling>(arg)?;
    let object = file.handles.get(req.handle).ok_or(Errno::ENOENT)?;
    match req.tiling_mode {
        drm::I915_TILING_NONE => {
            gpu.set_object_tiling(object.id, drm::I915_TILING_NONE, 0);
        }
        drm::I915_TILING_X | drm::I915_TILING_Y => {
            if req.stride == 0 || (req.stride & (super::regs::SCANOUT_STRIDE_ALIGN - 1)) != 0 {
                crate::return_errno!(EINVAL, "invalid tiling stride alignment");
            }
            gpu.set_object_tiling(object.id, req.tiling_mode, req.stride);
        }
        _ => crate::return_errno!(EINVAL, "invalid i915 tiling mode"),
    }
    req.swizzle_mode = drm::I915_BIT_6_SWIZZLE_NONE;
    write_uapi(arg, &req)?;
    Ok(0)
}

fn ioctl_gem_get_tiling(gpu: &IntelGpu, file: &mut DrmFile, arg: usize) -> Result<usize> {
    let mut req = read_uapi::<drm::DrmI915GemTiling>(arg)?;
    let object = file.handles.get(req.handle).ok_or(Errno::ENOENT)?;
    let (tiling_mode, stride) = gpu.object_tiling(object.id);
    req.tiling_mode = tiling_mode;
    req.stride = stride;
    req.swizzle_mode = drm::I915_BIT_6_SWIZZLE_NONE;
    write_uapi(arg, &req)?;
    Ok(0)
}

fn ioctl_gem_busy(file: &mut DrmFile, arg: usize) -> Result<usize> {
    let mut req = read_uapi::<drm::DrmI915GemBusy>(arg)?;
    let _object = file.handles.get(req.handle).ok_or(Errno::ENOENT)?;
    req.busy = 0;
    write_uapi(arg, &req)?;
    Ok(0)
}

fn ioctl_gem_wait(file: &mut DrmFile, arg: usize) -> Result<usize> {
    let req = read_uapi::<drm::DrmI915GemWait>(arg)?;
    let _object = file.handles.get(req.bo_handle).ok_or(Errno::ENOENT)?;
    Ok(0)
}

fn ioctl_get_reset_stats(arg: usize) -> Result<usize> {
    let mut req = read_uapi::<drm::DrmI915ResetStats>(arg)?;
    if req.flags != 0 {
        crate::return_errno!(EINVAL, "invalid reset_stats flags");
    }
    req.reset_count = 0;
    req.batch_active = 0;
    req.batch_pending = 0;
    write_uapi(arg, &req)?;
    Ok(0)
}
