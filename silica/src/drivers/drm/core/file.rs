// SPDX-License-Identifier: GPL-2.0

//! Per-open DRM file state ([`DrmFile`]).

use alloc::sync::Arc;

use super::device::DrmDevice;
use alloc::collections::VecDeque;
use spin::Mutex;
use crate::drivers::drm::gem;
use crate::uapi::drm::DrmEventVblank;

/// Client capabilities set via `DRM_IOCTL_SET_CLIENT_CAP`.
///
/// Accepted eagerly, since they only widen what mode-setting ioctls may
/// return; the DRM core re-checks them when it builds responses.
#[derive(Debug, Clone, Copy, Default)]
pub struct ClientCaps {
    /// `DRM_CLIENT_CAP_STEREO_3D`: deliver stereo mode flags.
    pub stereo_3d: bool,
    /// `DRM_CLIENT_CAP_UNIVERSAL_PLANES`: expose all plane types.
    pub universal_planes: bool,
    /// `DRM_CLIENT_CAP_ATOMIC`: allow the atomic world.
    pub atomic: bool,
    /// `DRM_CLIENT_CAP_ASPECT_RATIO`: deliver aspect-ratio mode flags.
    pub aspect_ratio: bool,
}

/// The state of a single open `/dev/dri/cardN` description.
///
/// GEM handles and mapping grants are namespaced per file, exactly as in
/// Linux: one process cannot reference another's objects by handle. All
/// tables are guarded by the lock of the `DrmCardFileOps` wrapper that holds
/// the state (see `/dev/dri` glue in `fs/devfs/drm.rs`).
pub struct DrmFile {
    /// The device this file was opened on.
    pub device: Arc<dyn DrmDevice>,
    /// GEM handles keyed by handle number.
    pub handles: gem::GemHandleTable,
    /// Mapping grants keyed by fake mmap offset.
    pub offsets: gem::GemOffsetTable,
    /// Capabilities set through `DRM_IOCTL_SET_CLIENT_CAP`.
    pub caps: ClientCaps,
    /// Pending vblank / page-flip events queued for user read().
    pub events: Mutex<VecDeque<DrmEventVblank>>,
}

impl DrmFile {
    /// Creates file state for `device`.
    pub fn new(device: Arc<dyn DrmDevice>) -> Self {
        Self {
            device,
            handles: gem::GemHandleTable::new(),
            offsets: gem::GemOffsetTable::new(),
            caps: ClientCaps::default(),
            events: Mutex::new(VecDeque::new()),
        }
    }

    /// Queues a vblank or page-flip completion event and wakes pollers.
    pub fn queue_vblank_event(&self, event: DrmEventVblank) {
        self.events.lock().push_back(event);
        crate::fs::poll::notify();
    }
}

impl Drop for DrmFile {
    fn drop(&mut self) {
        self.device.close_file(self);
    }
}

#[cfg(ktest)]
mod tests {
    use super::*;
    use crate::uapi::drm::{DRM_EVENT_FLIP_COMPLETE, DrmEvent, DrmEventVblank};

    #[ktest]
    fn drm_file_vblank_event_queueing() {
        let events = Mutex::new(VecDeque::new());
        let event = DrmEventVblank {
            base: DrmEvent {
                type_: DRM_EVENT_FLIP_COMPLETE,
                length: core::mem::size_of::<DrmEventVblank>() as u32,
            },
            user_data: 0x1234,
            tv_sec: 1,
            tv_usec: 500,
            sequence: 42,
            crtc_id: 1,
        };
        events.lock().push_back(event);
        assert_eq!(events.lock().len(), 1);
        let popped = events.lock().pop_front().unwrap();
        assert_eq!(popped.user_data, 0x1234);
        assert_eq!(popped.sequence, 42);
        assert_eq!(popped.base.type_, DRM_EVENT_FLIP_COMPLETE);
    }
}

