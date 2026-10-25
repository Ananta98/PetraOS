// SPDX-License-Identifier: GPL-2.0

//! DRM ioctl dispatch and userspace struct marshalling.

use alloc::sync::Arc;
use super::file::DrmFile;
use crate::drivers::drm::kms;
use crate::proc::thread::Thread;
use crate::uapi;
use crate::uapi::errno::{Errno, Result};
use crate::utils::userspace::UserPtr;

// ----------------------------------------------------------------------------
// Userspace struct marshalling
// ----------------------------------------------------------------------------

/// Copies a `#[repr(C)]` plain-data struct out of user space at `arg`.
///
/// This is DRM's ioctl convention: every request carries a pointer to a
/// struct in user memory. `UserPtr` faults in `EFAULT` on bad pointers
/// instead of crashing the kernel.
pub(crate) fn read_uapi<T>(arg: usize) -> Result<T>
where
    T: zerocopy::FromBytes
        + zerocopy::IntoBytes
        + zerocopy::KnownLayout
        + zerocopy::Immutable
        + Copy,
{
    let vmar = Thread::proc_or_esrch()?.vmspace();
    UserPtr::new(arg).read_val(&vmar)
}

/// Writes a `#[repr(C)]` plain-data struct into user space at `arg`.
pub(crate) fn write_uapi<T>(arg: usize, val: &T) -> Result<()>
where
    T: zerocopy::FromBytes
        + zerocopy::IntoBytes
        + zerocopy::KnownLayout
        + zerocopy::Immutable
        + Copy,
{
    let vmar = Thread::proc_or_esrch()?.vmspace();
    UserPtr::new(arg).write_val(&vmar, *val)
}

/// Copies a byte string into a user buffer, returning the untruncated length.
///
/// Mirrors the string-returning ioctls like `DRM_IOCTL_VERSION`: at most
/// `len` bytes are copied (`len == 0` or a null pointer means no copy), and
/// the full source length is always reported so user space can size buffers
/// and retry. `NUL`-termination is deliberately not added; Linux does not
/// either.
pub(crate) fn copy_string_to_user(ptr: usize, len: usize, src: &[u8]) -> usize {
    if ptr == 0 || len == 0 {
        return src.len();
    }
    let copy_len = src.len().min(len);
    if let Some(proc) = Thread::current_proc() {
        let _ = UserPtr::new(ptr).write_bytes(&proc.vmspace(), &src[..copy_len]);
    }
    src.len()
}

// ----------------------------------------------------------------------------
// Dispatch
// ----------------------------------------------------------------------------

/// Dispatches DRM ioctl `cmd`/`arg` for `file`.
///
/// Handles everything in the standard core (0x00..0x1F) and dumb-buffer area
/// (0xB2..0xB4); requests in the per-driver range (0x40..0x9F) and the later
/// mode-setting range (0xA0..) delegate to the device's own ioctl handler.
pub fn dispatch(file: &mut DrmFile, cmd: u32, arg: usize) -> Result<usize> {
    match cmd {
        uapi::drm::DRM_IOCTL_VERSION => ioctl_version(file, arg),
        uapi::drm::DRM_IOCTL_GET_CAP => ioctl_get_cap(arg),
        uapi::drm::DRM_IOCTL_SET_CLIENT_CAP => ioctl_set_client_cap(file, arg),
        uapi::drm::DRM_IOCTL_SET_MASTER | uapi::drm::DRM_IOCTL_DROP_MASTER => Ok(0),
        uapi::drm::DRM_IOCTL_PRIME_HANDLE_TO_FD => ioctl_prime_handle_to_fd(file, arg),
        uapi::drm::DRM_IOCTL_PRIME_FD_TO_HANDLE => ioctl_prime_fd_to_handle(file, arg),
        uapi::drm::DRM_IOCTL_GEM_CLOSE | uapi::drm::DRM_IOCTL_MODE_DESTROY_DUMB => {
            ioctl_gem_close(file, arg)
        }
        uapi::drm::DRM_IOCTL_MODE_CREATE_DUMB => ioctl_create_dumb(file, arg),
        uapi::drm::DRM_IOCTL_MODE_MAP_DUMB => ioctl_map_dumb(file, arg),
        _ => {
            let device = file.device.clone();
            // The number byte decides the family: only native KMS request
            // numbers (0xA0..0xB8) go to the mode-setting layer; per-driver
            // ioctls (0x40..0x9F) still reach driver_ioctl despite KMS.
            if kms::KmsState::is_kms_ioctl(cmd)
                && let Some(kms) = device.kms()
            {
                kms::ioctl::dispatch(kms, file, cmd, arg)
            } else {
                device.driver_ioctl(file, cmd, arg)
            }
        }
    }
}

fn ioctl_version(file: &mut DrmFile, arg: usize) -> Result<usize> {
    let ver = read_uapi::<uapi::drm::DrmVersion>(arg)?;
    let (major, minor, patch) = file.device.driver_version();
    let driver_name = file.device.driver_name().as_bytes();
    let (date, desc) = file.device.driver_info();

    let out = uapi::drm::DrmVersion {
        version_major: major as i32,
        version_minor: minor as i32,
        version_patchlevel: patch as i32,
        pad: ver.pad,
        name_len: copy_string_to_user(ver.name, ver.name_len, driver_name),
        name: ver.name,
        date_len: copy_string_to_user(ver.date, ver.date_len, date.as_bytes()),
        date: ver.date,
        desc_len: copy_string_to_user(ver.desc, ver.desc_len, desc.as_bytes()),
        desc: ver.desc,
    };
    write_uapi(arg, &out)?;
    Ok(0)
}

fn ioctl_get_cap(arg: usize) -> Result<usize> {
    let mut cap = read_uapi::<uapi::drm::DrmGetCap>(arg)?;
    cap.value = match cap.capability {
        uapi::drm::DRM_CAP_DUMB_BUFFER => 1,
        uapi::drm::DRM_CAP_DUMB_PREFERRED_DEPTH => 24, // 24-bit weight preferred
        uapi::drm::DRM_CAP_CURSOR_WIDTH => 64,
        uapi::drm::DRM_CAP_CURSOR_HEIGHT => 64,
        uapi::drm::DRM_CAP_PRIME => {
            uapi::drm::DRM_PRIME_CAP_IMPORT | uapi::drm::DRM_PRIME_CAP_EXPORT
        }
        uapi::drm::DRM_CAP_TIMESTAMP_MONOTONIC => 1,
        uapi::drm::DRM_CAP_CRTC_IN_VBLANK_EVENT => 1,
        _ => 0,
    };
    write_uapi(arg, &cap)?;
    Ok(0)
}

fn ioctl_set_client_cap(file: &mut DrmFile, arg: usize) -> Result<usize> {
    let req = read_uapi::<uapi::drm::DrmSetClientCap>(arg)?;
    if req.value > 1 {
        // Linux rejects non-boolean values with EINVAL.
        crate::return_errno!(EINVAL, "client capability value must be 0 or 1");
    }
    let enabled = req.value == 1;
    match req.capability {
        uapi::drm::DRM_CLIENT_CAP_STEREO_3D => file.caps.stereo_3d = enabled,
        uapi::drm::DRM_CLIENT_CAP_UNIVERSAL_PLANES => file.caps.universal_planes = enabled,
        uapi::drm::DRM_CLIENT_CAP_ATOMIC => file.caps.atomic = enabled,
        uapi::drm::DRM_CLIENT_CAP_ASPECT_RATIO => file.caps.aspect_ratio = enabled,
        _ => {
            // Unknown capabilities are rejected, matching Linux.
            crate::return_errno!(EINVAL, "unknown client capability");
        }
    }
    Ok(0)
}

fn ioctl_gem_close(file: &mut DrmFile, arg: usize) -> Result<usize> {
    let req = read_uapi::<uapi::drm::DrmGemClose>(arg)?;
    if !file.handles.remove(req.handle) {
        crate::return_errno!(EINVAL, "handle to close does not exist");
    }
    Ok(0)
}

fn ioctl_create_dumb(file: &mut DrmFile, arg: usize) -> Result<usize> {
    let mut req = read_uapi::<uapi::drm::DrmModeCreateDumb>(arg)?;
    if req.flags != 0 || req.bpp == 0 || req.height == 0 || req.width == 0 {
        crate::return_errno!(EINVAL, "invalid dumb buffer parameters");
    }

    // The dumb-buffer contract: driver-aligned pitch, kernel-chosen layout.
    let pitch = file.device.dumb_pitch(req.width, req.bpp);
    let size = (req.height as usize).saturating_mul(pitch as usize);

    let object = file.device.create_object(size)?;
    req.size = object.size() as u64;
    req.handle = file.handles.insert(object);
    req.pitch = pitch;
    write_uapi(arg, &req)?;
    Ok(0)
}

fn ioctl_map_dumb(file: &mut DrmFile, arg: usize) -> Result<usize> {
    let mut req = read_uapi::<uapi::drm::DrmModeMapDumb>(arg)?;
    let Some(object) = file.handles.get(req.handle) else {
        crate::return_errno!(EINVAL, "handle to map does not exist");
    };
    req.offset = file.offsets.grant(object);
    write_uapi(arg, &req)?;
    Ok(0)
}

/// VFS `FileOps` for exported PRIME DMA-BUF file descriptors.
pub struct DmaBufFileOps {
    pub object: Arc<crate::drivers::drm::GemObject>,
}

impl crate::fs::vfs::FileOps for DmaBufFileOps {
    fn read(&self, _offset: u64, _buf: &mut [u8]) -> Result<usize> {
        crate::return_errno!(EINVAL, "DMA-BUF does not support read");
    }

    fn write(&self, _offset: u64, _buf: &[u8]) -> Result<usize> {
        crate::return_errno!(EINVAL, "DMA-BUF does not support write");
    }

    fn mmap(&self, _offset: u64, len: usize) -> Result<()> {
        if len > self.object.size() {
            crate::return_errno!(EINVAL, "mapping extends past buffer object");
        }
        Ok(())
    }

    fn mmap_backing(
        &self,
        offset: u64,
        len: usize,
    ) -> Result<Option<Arc<dyn crate::vm::backing::Backing>>> {
        if len > self.object.size() {
            crate::return_errno!(EINVAL, "mapping extends past buffer object");
        }
        Ok(Some(crate::drivers::drm::gem::GemBacking::granted(
            Arc::clone(&self.object),
            offset,
        )))
    }

    fn as_dmabuf(&self) -> Option<Arc<crate::drivers::drm::GemObject>> {
        Some(Arc::clone(&self.object))
    }
}

struct DmaBufNodeOps;

impl crate::fs::vfs::NodeOps for DmaBufNodeOps {
    fn getattr(&self) -> Result<crate::fs::vfs::INodeAttr> {
        Ok(crate::fs::vfs::INodeAttr {
            mode: crate::fs::vfs::Mode::FILE
                | crate::fs::vfs::Mode::RUSR
                | crate::fs::vfs::Mode::WUSR,
            ..Default::default()
        })
    }
}

fn ioctl_prime_handle_to_fd(file: &mut DrmFile, arg: usize) -> Result<usize> {
    let mut req = read_uapi::<uapi::drm::DrmPrimeHandle>(arg)?;
    let object = file.handles.get(req.handle).ok_or(Errno::ENOENT)?;
    let proc = Thread::proc_or_esrch()?;
    let inode = Arc::new(crate::fs::vfs::INode::new(
        alloc::boxed::Box::new(DmaBufNodeOps),
        crate::fs::vfs::Mode::FILE | crate::fs::vfs::Mode::RUSR | crate::fs::vfs::Mode::WUSR,
        0xd1a0,
    ));
    let dmabuf_file = Arc::new(crate::fs::vfs::File::new(
        alloc::boxed::Box::new(DmaBufFileOps { object }),
        inode,
        crate::fs::vfs::OpenFlags::READ | crate::fs::vfs::OpenFlags::WRITE,
        false,
    ));
    let cloexec = (req.flags & 0x80000) != 0; // O_CLOEXEC
    let fd_flags = if cloexec {
        crate::fs::fd::FdFlags::FD_CLOEXEC
    } else {
        crate::fs::fd::FdFlags::empty()
    };
    let fd = proc.fd_table.lock().alloc_fd(dmabuf_file, fd_flags)?;
    req.fd = fd;
    write_uapi(arg, &req)?;
    Ok(0)
}

fn ioctl_prime_fd_to_handle(file: &mut DrmFile, arg: usize) -> Result<usize> {
    let mut req = read_uapi::<uapi::drm::DrmPrimeHandle>(arg)?;
    let proc = Thread::proc_or_esrch()?;
    let f = proc.fd_table.lock().get(req.fd)?;
    let object = f.as_dmabuf().ok_or(Errno::EINVAL)?;
    let handle = file.handles.insert(object);
    req.handle = handle;
    write_uapi(arg, &req)?;
    Ok(0)
}
