// SPDX-License-Identifier: GPL-2.0

//! KMS object model: the display pipelines a driver exposes and the DRM
//! object tables (framebuffers, CRTCs, encoders, connectors) the ioctls act
//! on, mirroring `struct drm_device`'s mode_config.
//!
//! The KMS core is written once for every driver: drivers describe their
//! pipeline hardware in [`KmsPipeline`] entries and plug in a
//! [`DisplayDriver`] callback, and the ioctl layer (`kms::ioctl`) drives
//! everything through [`KmsState`].

use alloc::{collections::BTreeMap, sync::Arc, vec::Vec};
use core::sync::atomic::{AtomicU32, Ordering};
use ostd::sync::SpinLock;

use crate::drivers::drm::GemObject;
use crate::uapi::drm;
use crate::uapi::errno::{Errno, Result};

/// Static description of one display pipeline (connector/encoder/CRTC trio).
pub struct KmsPipeline {
    /// The modes user space can select; index 0 is the current mode.
    pub modes: Vec<drm::DrmModeModeInfo>,
    /// `DRM_MODE_CONNECTOR_*` class of the connector.
    pub connector_type: u32,
    /// `DRM_MODE_ENCODER_*` class of the encoder.
    pub encoder_type: u32,
    /// Reportable physical sink size in millimeters.
    pub mm_width: u32,
    pub mm_height: u32,
}

impl KmsPipeline {
    /// Smallest and largest mode geometry, for `GETRESOURCES` card limits.
    pub fn dimension_range(&self) -> (u32, u32, u32, u32) {
        let mut min_w = u32::MAX;
        let mut max_w = 0;
        let mut min_h = u32::MAX;
        let mut max_h = 0;
        for mode in &self.modes {
            min_w = min_w.min(mode.hdisplay as u32);
            max_w = max_w.max(mode.hdisplay as u32);
            min_h = min_h.min(mode.vdisplay as u32);
            max_h = max_h.max(mode.vdisplay as u32);
        }
        (min_w, max_w, min_h, max_h)
    }
}

/// The hardware callbacks plugged in by the display driver.
pub trait DisplayDriver: Send + Sync {
    /// Displays `fb` on pipeline `pipeline`, applying the given mode.
    fn set_scanout(
        &self,
        pipeline: usize,
        fb: &Framebuffer,
        mode: &drm::DrmModeModeInfo,
    ) -> Result<()>;

    /// Stops displaying on pipeline `pipeline`.
    fn disable_scanout(&self, pipeline: usize) -> Result<()>;

    /// Refresh hook after user space marks an on-screen framebuffer dirty
    /// (`DIRTYFB`); drivers pushing contents on demand flush here.
    fn refresh(&self, pipeline: usize, fb: &Framebuffer) -> Result<()>;

    /// Refresh hook for a partial damage rectangle `(x, y, width, height)`
    /// within `fb`. Defaults to refreshing the entire framebuffer.
    fn refresh_rect(
        &self,
        pipeline: usize,
        fb: &Framebuffer,
        _x: u32,
        _y: u32,
        _width: u32,
        _height: u32,
    ) -> Result<()> {
        self.refresh(pipeline, fb)
    }

    /// Invoked when a framebuffer is unregistered via `RMFB` so the driver
    /// can detach and release any host-side resources bound to `fb`.
    fn release_fb(&self, _fb: &Framebuffer) {}

    /// Updates or hides the hardware cursor image on `pipeline`.
    #[expect(clippy::too_many_arguments)]
    fn set_cursor(
        &self,
        _pipeline: usize,
        _bo: Option<&Arc<GemObject>>,
        _width: u32,
        _height: u32,
        _x: i32,
        _y: i32,
        _hot_x: i32,
        _hot_y: i32,
    ) -> Result<()> {
        Ok(())
    }

    /// Moves the hardware cursor on `pipeline` to `(x, y)`.
    fn move_cursor(&self, _pipeline: usize, _x: i32, _y: i32) -> Result<()> {
        Ok(())
    }
}

/// A registered framebuffer: geometry plus the scanout buffer it slices out.
#[derive(Debug)]
pub struct Framebuffer {
    pub id: u32,
    pub object: Arc<GemObject>,
    pub width: u32,
    pub height: u32,
    pub pitch: u32,
    pub format: u32,
}

impl Framebuffer {
    /// Bytes per pixel of `format`, for bound validation.
    pub fn bpp(&self) -> u32 {
        match self.format {
            drm::DRM_FORMAT_XRGB8888 | drm::DRM_FORMAT_ARGB8888 => 32,
            drm::DRM_FORMAT_RGB565 | drm::DRM_FORMAT_XRGB1555 => 16,
            _ => 0,
        }
    }
}

/// Per-pipeline object ids and current scanout state.
struct PipelineState {
    crtc_id: u32,
    encoder_id: u32,
    connector_id: u32,
    primary_plane_id: u32,
    /// The displayed framebuffer, `None` while disabled.
    fb: SpinLock<Option<Arc<Framebuffer>>>,
}

/// The complete KMS object tables of one DRM device.
pub struct KmsState {
    driver: Arc<dyn DisplayDriver>,
    pipelines: Vec<KmsPipeline>,
    states: Vec<PipelineState>,
    fbs: SpinLock<BTreeMap<u32, Arc<Framebuffer>>>,
    next_id: AtomicU32,
}

impl KmsState {
    /// Builds the object tables for `pipelines`, binding them to `driver`.
    pub fn new<Driver: DisplayDriver + 'static>(
        driver: Arc<Driver>,
        pipelines: Vec<KmsPipeline>,
    ) -> Arc<Self> {
        let next_id = AtomicU32::new(1);
        let states = pipelines
            .iter()
            .map(|_| PipelineState {
                crtc_id: next_id.fetch_add(1, Ordering::Relaxed),
                encoder_id: next_id.fetch_add(1, Ordering::Relaxed),
                connector_id: next_id.fetch_add(1, Ordering::Relaxed),
                primary_plane_id: next_id.fetch_add(1, Ordering::Relaxed),
                fb: SpinLock::new(None),
            })
            .collect();
        Arc::new(Self {
            driver: driver as Arc<dyn DisplayDriver>,
            pipelines,
            states,
            fbs: SpinLock::new(BTreeMap::new()),
            next_id,
        })
    }

    /// Returns true if the ioctl number is in the mode-setting range
    /// (`0xA0..=0xBF`), excluding the dumb-buffer range (`0xB2..=0xB4`).
    pub fn is_kms_ioctl(cmd: u32) -> bool {
        let nr = cmd & 0xFF;
        (0xA0..=0xBF).contains(&nr) && !(0xB2..=0xB4).contains(&nr)
    }

    /// The pipeline index owning CRTC id `id`, if known.
    pub fn pipeline_of_crtc(&self, id: u32) -> Option<usize> {
        self.states.iter().position(|s| s.crtc_id == id)
    }

    /// CRTC ids, indexed by pipeline.
    pub fn crtc_ids(&self) -> Vec<u32> {
        self.states.iter().map(|s| s.crtc_id).collect()
    }

    pub fn encoder_ids(&self) -> Vec<u32> {
        self.states.iter().map(|s| s.encoder_id).collect()
    }

    pub fn connector_ids(&self) -> Vec<u32> {
        self.states.iter().map(|s| s.connector_id).collect()
    }

    /// Pipeline index owning connector id `id`, if known.
    pub fn pipeline_of_connector(&self, id: u32) -> Option<usize> {
        self.states.iter().position(|s| s.connector_id == id)
    }

    /// Pipeline index owning encoder id `id`, if known.
    pub fn pipeline_of_encoder(&self, id: u32) -> Option<usize> {
        self.states.iter().position(|s| s.encoder_id == id)
    }

    /// Pipeline index owning primary-plane id `id`, if known.
    pub fn pipeline_of_plane(&self, id: u32) -> Option<usize> {
        self.states.iter().position(|s| s.primary_plane_id == id)
    }

    pub fn pipeline_count(&self) -> usize {
        self.pipelines.len()
    }

    pub fn pipeline_modes(&self, pipeline: usize) -> &[drm::DrmModeModeInfo] {
        &self.pipelines[pipeline].modes
    }

    pub fn pipeline_geometry(&self, pipeline: usize) -> (u32, u32, u32, u32) {
        self.pipelines[pipeline].dimension_range()
    }

    pub fn pipeline_connector(&self, pipeline: usize) -> (u32, u32, u32, u32) {
        let info = &self.pipelines[pipeline];
        (info.connector_type, 1, info.mm_width, info.mm_height)
    }

    pub fn pipeline_encoder(&self, pipeline: usize) -> (u32, u32, u32) {
        let state = &self.states[pipeline];
        (
            self.pipelines[pipeline].encoder_type,
            state.crtc_id,
            1u32 << pipeline as u32,
        )
    }

    /// The framebuffer displayed by `pipeline`, if enabled.
    pub fn active_fb(&self, pipeline: usize) -> Option<Arc<Framebuffer>> {
        self.states[pipeline].fb.lock().clone()
    }

    /// Displays `fb` on `pipeline` with `mode` and remembers it.
    pub fn set_scanout(
        &self,
        pipeline: usize,
        fb: Arc<Framebuffer>,
        mode: &drm::DrmModeModeInfo,
    ) -> Result<()> {
        self.driver.set_scanout(pipeline, &fb, mode)?;
        *self.states[pipeline].fb.lock() = Some(fb);
        Ok(())
    }

    /// Disables `pipeline`'s scanout.
    pub fn disable(&self, pipeline: usize) -> Result<()> {
        let res = self.driver.disable_scanout(pipeline);
        *self.states[pipeline].fb.lock() = None;
        res
    }

    /// Marks the recompose-half of `DIRTYFB` for the active pipeline of `fb`.
    pub fn refresh(&self, fb: &Framebuffer) -> Result<()> {
        let pipeline = self
            .states
            .iter()
            .position(|s| s.fb.lock().as_ref().is_some_and(|f| f.id == fb.id));
        if let Some(pipeline) = pipeline {
            self.driver.refresh(pipeline, fb)?;
        }
        Ok(())
    }

    /// Refreshes a partial damage rectangle `(x, y, width, height)` of `fb`
    /// on its active pipeline.
    pub fn refresh_rect(
        &self,
        fb: &Framebuffer,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    ) -> Result<()> {
        let pipeline = self
            .states
            .iter()
            .position(|s| s.fb.lock().as_ref().is_some_and(|f| f.id == fb.id));
        if let Some(pipeline) = pipeline {
            self.driver
                .refresh_rect(pipeline, fb, x, y, width, height)?;
        }
        Ok(())
    }

    /// Updates the hardware cursor image (and position/hotspot) on `pipeline`.
    #[expect(clippy::too_many_arguments)]
    pub fn set_cursor(
        &self,
        pipeline: usize,
        bo: Option<&Arc<GemObject>>,
        width: u32,
        height: u32,
        x: i32,
        y: i32,
        hot_x: i32,
        hot_y: i32,
    ) -> Result<()> {
        self.driver
            .set_cursor(pipeline, bo, width, height, x, y, hot_x, hot_y)
    }

    /// Moves the hardware cursor on `pipeline` to `(x, y)`.
    pub fn move_cursor(&self, pipeline: usize, x: i32, y: i32) -> Result<()> {
        self.driver.move_cursor(pipeline, x, y)
    }

    /// The registered framebuffer with object id `id`.
    pub fn fb(&self, id: u32) -> Option<Arc<Framebuffer>> {
        self.fbs.lock().get(&id).cloned()
    }

    /// All registered framebuffer ids.
    pub fn fb_ids(&self) -> Vec<u32> {
        self.fbs.lock().keys().copied().collect()
    }

    /// Registers a framebuffer, assigning it a fresh object id.
    pub fn add_fb(&self, mut fb: Framebuffer) -> u32 {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        fb.id = id;
        self.fbs.lock().insert(id, Arc::new(fb));
        id
    }

    /// Unregisters a framebuffer; fails while a CRTC still displays it.
    pub fn remove_fb(&self, id: u32) -> Result<Arc<Framebuffer>> {
        if self
            .states
            .iter()
            .any(|s| s.fb.lock().as_ref().is_some_and(|f| f.id == id))
        {
            crate::return_errno!(EINVAL, "framebuffer is in use on a CRTC");
        }
        let fb = self.fbs.lock().remove(&id).ok_or(Errno::EINVAL)?;
        self.driver.release_fb(&fb);
        Ok(fb)
    }
}
