// SPDX-License-Identifier: GPL-2.0

//! DRM device registry and driver interface.

use alloc::{string::String, sync::Arc, vec::Vec};
use spin::Mutex;

use super::file::DrmFile;
use crate::drivers::drm::gem;
use crate::drivers::drm::kms::KmsState;
use crate::uapi::errno::{Errno, Result};

/// The programming interface exposed by each DRM-capable GPU driver.
///
/// The DRM core (ioctls, GEM tables, devfs nodes) is implemented once for all
/// devices; hardware specifics enter here. Implementations exist for the
/// builtin virtio GPU and the Intel i915-lite driver.
pub trait DrmDevice: Send + Sync {
    /// Driver name reported by `DRM_IOCTL_VERSION`.
    fn driver_name(&self) -> &'static str;

    /// Driver identifier strings (like `DRM_IOCTL_VERSION`'s date and
    /// descriptor fields).
    fn driver_info(&self) -> (String, String) {
        (
            String::from("20250101"),
            String::from("PetraOS built-in DRM driver"),
        )
    }

    /// Major/minor/patch version triple for `DRM_IOCTL_VERSION`.
    fn driver_version(&self) -> (u32, u32, u32) {
        (1, 0, 0)
    }

    /// Whether the device has KMS, deciding which ioctls are advertised.
    fn has_kms(&self) -> bool {
        self.kms().is_some()
    }

    /// The mode-setting layer of the device, if any. When present, all
    /// requests in the native KMS number range (0xA0..0xB8) are driven
    /// through it; per-device ioctls (0x40..0x9F) still reach
    /// [`DrmDevice::driver_ioctl`].
    fn kms(&self) -> Option<&'static KmsState> {
        None
    }

    /// Allocates a buffer object of `size` bytes.
    ///
    /// The default is a plain zeroed shmem-style object; drivers may override
    /// it to allocate device-special memory (e.g. VRAM-backed).
    fn create_object(&self, size: usize) -> Result<Arc<gem::GemObject>> {
        gem::GemObject::alloc(size)
    }

    /// Custom ioctls that fall outside the standard DRM numbering.
    fn driver_ioctl(&self, _file: &mut DrmFile, _cmd: u32, _arg: usize) -> Result<usize> {
        crate::return_errno!(ENOTTY, "driver does not support this ioctl");
    }

    /// Cleans per-file resources at file close.
    fn close_file(&self, _file: &DrmFile) {}
}

/// Registered DRM card devices, in registration (minor) order.
pub static DRM_DEVICES: Mutex<Vec<Arc<dyn DrmDevice>>> = Mutex::new(Vec::new());

/// Registers a DRM device, assigning it the next `/dev/dri/cardN` minor.
pub fn register_drm_device(device: Arc<dyn DrmDevice>) -> u32 {
    let mut devices = DRM_DEVICES.lock();
    let minor = devices.len() as u32;
    ostd::info!(
        "DRM: Registered card device {} (driver: {})",
        minor,
        device.driver_name()
    );
    devices.push(device);
    minor
}

/// Returns the DRM device with the given minor, if present.
pub fn get_device(minor: u32) -> Result<Arc<dyn DrmDevice>> {
    DRM_DEVICES
        .lock()
        .get(minor as usize)
        .cloned()
        .ok_or(Errno::ENODEV)
}
