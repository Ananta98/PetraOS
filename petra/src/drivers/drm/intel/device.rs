// SPDX-License-Identifier: GPL-2.0

//! Intel `i915` PCI device probe, DRM/KMS integration, and GEM/GGTT binding.
//!
//! [`IntelPciDriver`] binds Intel Integrated Graphics controllers on the PCI
//! bus, maps `BAR0` (`GTTMMADR`: MMIO registers + Global GTT) and `BAR2`
//! (`GMADR`: Graphics Aperture), discovers active display pipes and EDID
//! timings, and registers an [`IntelGpu`] card with the DRM subsystem.

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use ostd::io::IoMem;
use ostd::mm::CachePolicy;
use spin::{Mutex, Once};

use super::display::{
    IntelCursorState, IntelPipeConfig, disable_primary_plane, discover_pipes, kms_pipeline_for,
    move_cursor_plane, program_cursor_plane, program_primary_plane, sync_cursor_to_aperture,
    sync_damage_to_scanout,
};
use super::gtt::GgttManager;
use super::regs::{
    CURSOR_SIZE, GGTT_SURFACE_ALIGN, IntelGen, PCI_CLASS_DISPLAY, PCI_SUBCLASS_DISPLAY_OTHER,
    PCI_SUBCLASS_DISPLAY_VGA, PCI_VENDOR_ID_INTEL, SCANOUT_STRIDE_ALIGN,
};
use crate::drivers::bus::pci::{BarType, PCI_ANY_ID, PciBus, PciDevice, PciDeviceId};
use crate::drivers::drm::{
    DisplayDriver, DrmDevice, DrmFile, Framebuffer, GemObject, KmsPipeline, KmsState,
    register_drm_device,
};
use crate::drivers::{BusDevice, Driver};
use crate::uapi::drm;
use crate::uapi::errno::{Errno, Result};

/// Maximum aperture window size mapped from `BAR2` (64 MiB) to avoid exhausting
/// kernel virtual address space on large resizable apertures.
const MAX_APERTURE_MAP_BYTES: usize = 64 * 1024 * 1024;

/// One active Intel Integrated Graphics DRM/KMS device.
pub struct IntelGpu {
    device_id: u16,
    revision: u8,
    generation: IntelGen,
    mmio: Option<IoMem>,
    aperture: Option<IoMem>,
    ggtt: Mutex<GgttManager>,
    pipes: Vec<IntelPipeConfig>,
    cursors: Mutex<Vec<IntelCursorState>>,
    tiling: Mutex<BTreeMap<u32, (u32, u32)>>,
    kms: Once<&'static KmsState>,
}

impl IntelGpu {
    /// Probes an Intel PCI display controller and initializes its GGTT and display pipes.
    pub fn from_pci(pci_dev: &PciDevice) -> Result<Arc<Self>> {
        pci_dev.enable_memory_space();
        pci_dev.enable_bus_mastering();

        let generation = IntelGen::from_device_id(pci_dev.device_id);
        let (mmio, bar0_size) = Self::map_bar0(pci_dev);
        let aperture = Self::map_bar2(pci_dev);

        let pipes = discover_pipes(generation, mmio.as_ref());
        if pipes.is_empty() {
            crate::return_errno!(ENODEV, "i915: no active display pipes found");
        }

        let cursors = (0..pipes.len())
            .map(|_| IntelCursorState::default())
            .collect();

        ostd::info!(
            "i915: initialized {} (device {:04x}, rev {:02x}, {} pipe(s), BAR0 {} KiB)",
            generation.label(),
            pci_dev.device_id,
            pci_dev.revision,
            pipes.len(),
            bar0_size / 1024
        );

        Ok(Arc::new(Self {
            device_id: pci_dev.device_id,
            revision: pci_dev.revision,
            generation,
            mmio,
            aperture,
            ggtt: Mutex::new(GgttManager::new(generation, bar0_size)),
            pipes,
            cursors: Mutex::new(cursors),
            tiling: Mutex::new(BTreeMap::new()),
            kms: Once::new(),
        }))
    }

    /// PCI device ID (`0x8086:XXXX`).
    pub const fn device_id(&self) -> u16 {
        self.device_id
    }

    /// PCI hardware revision ID.
    pub const fn revision(&self) -> u8 {
        self.revision
    }

    /// Hardware generation classification.
    pub const fn generation(&self) -> IntelGen {
        self.generation
    }

    /// Records GEM object tiling parameters (`I915_GEM_SET_TILING`).
    pub fn set_object_tiling(&self, obj_id: u32, mode: u32, stride: u32) {
        if mode == drm::I915_TILING_NONE {
            self.tiling.lock().remove(&obj_id);
        } else {
            self.tiling.lock().insert(obj_id, (mode, stride));
        }
    }

    /// Queries GEM object tiling parameters (`I915_GEM_GET_TILING`).
    pub fn object_tiling(&self, obj_id: u32) -> (u32, u32) {
        self.tiling
            .lock()
            .get(&obj_id)
            .copied()
            .unwrap_or((drm::I915_TILING_NONE, 0))
    }

    /// If `object` backs an currently active CRTC framebuffer, synchronizes its
    /// contents to the scanout surface.
    pub fn flush_object_if_scanned_out(&self, object: &GemObject) {
        let Some(kms) = self.kms.get().copied() else {
            return;
        };
        for pipeline in 0..self.pipes.len() {
            if let Some(fb) = kms.active_fb(pipeline)
                && fb.object.id == object.id
            {
                let _ = self.refresh(pipeline, &fb);
            }
        }
    }

    /// Builds and leaks the static [`KmsState`] for this device's pipes.
    fn build_kms(self: &Arc<Self>) -> &'static KmsState {
        let pipelines: Vec<KmsPipeline> = self.pipes.iter().map(kms_pipeline_for).collect();
        let state = KmsState::new(Arc::clone(self), pipelines);
        Box::leak(Box::new(state))
    }

    fn map_bar0(pci_dev: &PciDevice) -> (Option<IoMem>, usize) {
        let Some(bar) = pci_dev.bars[0]
            .filter(|b| matches!(b.bar_type, BarType::Memory32 | BarType::Memory64) && b.size > 0)
        else {
            return (None, 0);
        };
        let start = bar.address as usize;
        let size = bar.size as usize;
        let io = IoMem::acquire(start..start + size).ok();
        (io, size)
    }

    fn map_bar2(pci_dev: &PciDevice) -> Option<IoMem> {
        let bar = pci_dev.bars[2]
            .filter(|b| matches!(b.bar_type, BarType::Memory32 | BarType::Memory64) && b.size > 0)?;
        let start = bar.address as usize;
        let size = (bar.size as usize).min(MAX_APERTURE_MAP_BYTES);
        IoMem::acquire_with_cache_policy(start..start + size, CachePolicy::WriteCombining)
            .or_else(|_| IoMem::acquire(start..start + size))
            .ok()
    }
}

impl DisplayDriver for IntelGpu {
    fn set_scanout(
        &self,
        pipeline: usize,
        fb: &Framebuffer,
        mode: &drm::DrmModeModeInfo,
    ) -> Result<()> {
        let pipe_cfg = self.pipes.get(pipeline).ok_or(Errno::EINVAL)?;
        let binding = self
            .ggtt
            .lock()
            .bind_object(self.mmio.as_ref(), &fb.object)?;

        sync_damage_to_scanout(
            fb,
            self.aperture.as_ref(),
            pipe_cfg.initial_surf as usize,
            0,
            0,
            fb.width,
            fb.height,
        )?;

        if let Some(ref mmio) = self.mmio {
            program_primary_plane(
                self.generation,
                mmio,
                pipe_cfg.pipe,
                fb,
                mode,
                binding.gtt_offset,
            )?;
        }
        Ok(())
    }

    fn disable_scanout(&self, pipeline: usize) -> Result<()> {
        let pipe_cfg = self.pipes.get(pipeline).ok_or(Errno::EINVAL)?;
        if let Some(ref mmio) = self.mmio {
            disable_primary_plane(self.generation, mmio, pipe_cfg.pipe);
        }
        Ok(())
    }

    fn refresh(&self, pipeline: usize, fb: &Framebuffer) -> Result<()> {
        self.refresh_rect(pipeline, fb, 0, 0, fb.width, fb.height)
    }

    fn refresh_rect(
        &self,
        pipeline: usize,
        fb: &Framebuffer,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    ) -> Result<()> {
        let pipe_cfg = self.pipes.get(pipeline).ok_or(Errno::EINVAL)?;
        let binding = self
            .ggtt
            .lock()
            .bind_object(self.mmio.as_ref(), &fb.object)?;

        sync_damage_to_scanout(
            fb,
            self.aperture.as_ref(),
            pipe_cfg.initial_surf as usize,
            x,
            y,
            width,
            height,
        )?;

        if let Some(ref mmio) = self.mmio
            && let Some(mode) = pipe_cfg.modes.first()
        {
            program_primary_plane(
                self.generation,
                mmio,
                pipe_cfg.pipe,
                fb,
                mode,
                binding.gtt_offset,
            )?;
        }
        Ok(())
    }

    fn release_fb(&self, fb: &Framebuffer) {
        self.ggtt.lock().unbind_object(fb.object.id);
        self.tiling.lock().remove(&fb.object.id);
    }

    fn set_cursor(
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
        let pipe_cfg = self.pipes.get(pipeline).ok_or(Errno::EINVAL)?;

        let Some(object) = bo else {
            let old_obj_id = {
                let mut cursors = self.cursors.lock();
                let state = cursors.get_mut(pipeline).ok_or(Errno::EINVAL)?;
                let prev = state.obj_id;
                state.obj_id = 0;
                state.gtt_offset = 0;
                state.x = x;
                state.y = y;
                state.hot_x = hot_x;
                state.hot_y = hot_y;
                if let Some(ref mmio) = self.mmio {
                    program_cursor_plane(mmio, pipe_cfg.pipe, state);
                }
                prev
            };
            if old_obj_id != 0 {
                self.ggtt.lock().unbind_object(old_obj_id);
            }
            return Ok(());
        };

        if width != CURSOR_SIZE || height != CURSOR_SIZE {
            crate::return_errno!(EINVAL, "i915 hardware cursor must be 64x64");
        }
        if object.size() < (CURSOR_SIZE as usize) * (CURSOR_SIZE as usize) * 4 {
            crate::return_errno!(EINVAL, "i915 cursor buffer smaller than 64x64x4");
        }

        let binding = self.ggtt.lock().bind_object(self.mmio.as_ref(), object)?;
        sync_cursor_to_aperture(object, self.aperture.as_ref(), binding.gtt_offset as usize);

        let old_obj_id = {
            let mut cursors = self.cursors.lock();
            let state = cursors.get_mut(pipeline).ok_or(Errno::EINVAL)?;
            let prev = state.obj_id;
            state.obj_id = object.id;
            state.gtt_offset = binding.gtt_offset;
            state.x = x;
            state.y = y;
            state.hot_x = hot_x;
            state.hot_y = hot_y;
            if let Some(ref mmio) = self.mmio {
                program_cursor_plane(mmio, pipe_cfg.pipe, state);
            }
            prev
        };

        if old_obj_id != 0 && old_obj_id != object.id {
            self.ggtt.lock().unbind_object(old_obj_id);
        }
        Ok(())
    }

    fn move_cursor(&self, pipeline: usize, x: i32, y: i32) -> Result<()> {
        let pipe_cfg = self.pipes.get(pipeline).ok_or(Errno::EINVAL)?;
        let mut cursors = self.cursors.lock();
        let state = cursors.get_mut(pipeline).ok_or(Errno::EINVAL)?;
        state.x = x;
        state.y = y;
        if let Some(ref mmio) = self.mmio {
            move_cursor_plane(mmio, pipe_cfg.pipe, state);
        }
        Ok(())
    }
}

impl DrmDevice for IntelGpu {
    fn driver_name(&self) -> &'static str {
        "i915"
    }

    fn driver_info(&self) -> (String, String) {
        (
            format!("20260101"),
            format!("Intel {} Graphics (KMS + GEM + GGTT)", self.generation.label()),
        )
    }

    fn driver_version(&self) -> (u32, u32, u32) {
        (1, 6, 0)
    }

    fn kms(&self) -> Option<&'static KmsState> {
        self.kms.get().copied()
    }

    fn create_object(&self, size: usize) -> Result<Arc<GemObject>> {
        if size == 0 {
            crate::return_errno!(EINVAL, "GEM object size cannot be zero");
        }
        let aligned = size.div_ceil(GGTT_SURFACE_ALIGN) * GGTT_SURFACE_ALIGN;
        GemObject::alloc(aligned)
    }

    fn dumb_pitch(&self, width: u32, bpp: u32) -> u32 {
        let raw_pitch = width.saturating_mul(bpp.div_ceil(8));
        raw_pitch.div_ceil(SCANOUT_STRIDE_ALIGN) * SCANOUT_STRIDE_ALIGN
    }

    fn driver_ioctl(&self, file: &mut DrmFile, cmd: u32, arg: usize) -> Result<usize> {
        super::ioctl::dispatch(self, file, cmd, arg)
    }
}

/// PCI match table for Intel Integrated and Discrete Display Controllers.
static INTEL_PCI_IDS: [PciDeviceId; 2] = [
    PciDeviceId::new(
        PCI_VENDOR_ID_INTEL,
        PCI_ANY_ID,
        PCI_CLASS_DISPLAY as u16,
        PCI_SUBCLASS_DISPLAY_VGA as u16,
        PCI_ANY_ID,
    ),
    PciDeviceId::new(
        PCI_VENDOR_ID_INTEL,
        PCI_ANY_ID,
        PCI_CLASS_DISPLAY as u16,
        PCI_SUBCLASS_DISPLAY_OTHER as u16,
        PCI_ANY_ID,
    ),
];

/// PCI bus driver for Intel `i915` Graphics.
pub struct IntelPciDriver;

impl Driver<PciBus> for IntelPciDriver {
    fn name(&self) -> &str {
        "i915"
    }

    fn match_data(&self) -> &[PciDeviceId] {
        &INTEL_PCI_IDS
    }

    fn on_probe(&self, device: &BusDevice<PciBus>) -> Result<()> {
        let gpu = IntelGpu::from_pci(device.payload())?;
        let kms = gpu.build_kms();
        gpu.kms.call_once(|| kms);
        let _minor = register_drm_device(gpu);
        Ok(())
    }
}
