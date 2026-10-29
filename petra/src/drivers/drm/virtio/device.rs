// SPDX-License-Identifier: GPL-2.0

//! `virtio-gpu` PCI device glue and KMS [`DisplayDriver`] implementation.
//!
//! [`VirtioGpuDevice`] registers with the VirtIO bus and constructs one
//! [`VirtioGpu`] instance per attached PCI transport. [`VirtioGpu`] bridges
//! the kernel's DRM/KMS callbacks to the wire protocol in [`super::protocol`],
//! the resource/cursor pipeline in [`super::pipeline`], and the EDID/mode
//! synthesizer in [`super::modes`].

use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::mem::size_of;
use ostd::mm::{HasPaddr, VmIo, VmReader, dma::DmaCoherent, io::util::HasVmReaderWriter};
use spin::{Mutex, Once};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

use super::modes::build_scanout_modes;
use super::pipeline::{
    Pipeline, ResourceGeometry, TransferOffset, build_attach_backing, build_create_2d,
    build_detach_backing, build_flush, build_move_cursor, build_set_scanout, build_transfer_2d,
    build_unref, build_update_cursor, validate_cursor_dimensions,
};
use super::protocol::{
    CMD_GET_DISPLAY_INFO, CMD_GET_EDID, CONFIG_EVENTS_CLEAR, CONFIG_EVENTS_READ, CONTROL_QUEUE,
    CURSOR_QUEUE, CURSOR_SIZE, CtrlHdr, DisplayOne, EDID_MAX_BYTES, EVENT_DISPLAY, FEATURE_EDID,
    GetEdid, GpuConfig, GpuFormat, PCI_DEVICE_ID_VIRTIO_GPU, RESP_OK_DISPLAY_INFO, RESP_OK_EDID,
    RESP_OK_NODATA, Rect, RespDisplayInfo, RespEdid, UpdateCursor, check_response_code,
};
use crate::drivers::bus::virtio::{
    SubmitBuffer, VIRTIO_F_VERSION_1, VirtQueue, VirtioDevice, VirtioTransport,
};
use crate::drivers::drm::{
    DisplayDriver, DrmDevice, Framebuffer, GemObject, KmsPipeline, KmsState, register_drm_device,
};
use crate::uapi::drm;
use crate::uapi::errno::{Errno, Result};

/// Coherent DMA staging pages used to serialize control and cursor commands.
struct CommandStaging {
    request_dma: DmaCoherent,
    response_dma: DmaCoherent,
    cursor_dma: DmaCoherent,
}

impl CommandStaging {
    fn new() -> Result<Self> {
        Ok(Self {
            request_dma: DmaCoherent::alloc(1, false).map_err(|_| Errno::ENOMEM)?,
            response_dma: DmaCoherent::alloc(1, false).map_err(|_| Errno::ENOMEM)?,
            cursor_dma: DmaCoherent::alloc(1, false).map_err(|_| Errno::ENOMEM)?,
        })
    }
}

/// One live `virtio-gpu` instance bound to a PCI transport.
pub struct VirtioGpu {
    transport: Arc<VirtioTransport>,
    control_queue: Arc<VirtQueue>,
    cursor_queue: Option<Arc<VirtQueue>>,
    staging: Mutex<CommandStaging>,
    pipeline: Mutex<Pipeline>,
    config: GpuConfig,
    kms: Once<&'static KmsState>,
}

impl VirtioGpu {
    /// Initializes a `VirtioGpu` instance over an activated [`VirtioTransport`].
    fn new(transport: &Arc<VirtioTransport>) -> Result<Arc<Self>> {
        let Some(control_queue) = transport.queue(CONTROL_QUEUE) else {
            ostd::error!(
                "virtio-gpu: missing control queue (activated queues = {})",
                transport.queues().len()
            );
            crate::return_errno!(ENODEV, "virtio-gpu does not expose a control queue");
        };
        let cursor_queue = transport.queue(CURSOR_QUEUE);
        let config = GpuConfig::read(transport)?;
        let transfer_offset = TransferOffset::for_device(
            transport.pci_device_id(),
            transport.pci_subsystem_vendor_id(),
            transport.pci_subsystem_id(),
        );

        let gpu = Arc::new(Self {
            transport: Arc::clone(transport),
            control_queue,
            cursor_queue,
            staging: Mutex::new(CommandStaging::new()?),
            pipeline: Mutex::new(Pipeline::new(transfer_offset)),
            config,
            kms: Once::new(),
        });
        gpu.acknowledge_events();
        Ok(gpu)
    }

    /// Clears any pending device configuration events (such as `EVENT_DISPLAY`).
    pub fn acknowledge_events(&self) {
        if let Ok(events) = self.transport.device_read32(CONFIG_EVENTS_READ)
            && events & EVENT_DISPLAY != 0
        {
            let _ = self
                .transport
                .device_write32(CONFIG_EVENTS_CLEAR, events & EVENT_DISPLAY);
        }
    }

    /// Submits `request` on the control queue and reads back a typed `R` response.
    fn submit_control<T, R>(&self, request: &T) -> Result<R>
    where
        T: IntoBytes + Immutable,
        R: FromBytes + IntoBytes + KnownLayout + Immutable + Copy,
    {
        let req_bytes = request.as_bytes();
        let resp_len = size_of::<R>();
        if req_bytes.len() > ostd::mm::PAGE_SIZE || resp_len > ostd::mm::PAGE_SIZE {
            crate::return_errno!(EINVAL, "virtio-gpu command exceeds DMA page size");
        }

        let staging = self.staging.lock();
        staging
            .request_dma
            .writer()
            .write(&mut VmReader::from(req_bytes));

        self.control_queue.submit_wait(&[
            SubmitBuffer {
                addr: staging.request_dma.paddr() as u64,
                len: req_bytes.len() as u32,
                writable: false,
            },
            SubmitBuffer {
                addr: staging.response_dma.paddr() as u64,
                len: resp_len as u32,
                writable: true,
            },
        ])?;

        staging
            .response_dma
            .read_val::<R>(0)
            .map_err(|_| Errno::EIO)
    }

    /// Submits `request` on the control queue and requires `RESP_OK_NODATA`.
    fn submit_control_nodata<T: IntoBytes + Immutable>(&self, request: &T) -> Result<()> {
        let hdr: CtrlHdr = self.submit_control(request)?;
        check_response_code(hdr.kind, RESP_OK_NODATA)
    }

    /// Submits a cursor command on the cursor queue (or falls back to the control
    /// queue if the device only exposed one queue).
    fn submit_cursor(&self, cmd: &UpdateCursor) -> Result<()> {
        let Some(ref cursor_q) = self.cursor_queue else {
            return Ok(());
        };
        let bytes = cmd.as_bytes();
        let staging = self.staging.lock();
        staging
            .cursor_dma
            .writer()
            .write(&mut VmReader::from(bytes));

        cursor_q
            .submit_wait(&[SubmitBuffer {
                addr: staging.cursor_dma.paddr() as u64,
                len: bytes.len() as u32,
                writable: false,
            }])
            .map(|_| ())
    }

    /// Queries `VIRTIO_GPU_CMD_GET_DISPLAY_INFO` and returns enabled scanouts.
    fn query_display_info(&self) -> Result<Vec<(u32, DisplayOne)>> {
        let req = CtrlHdr::new(CMD_GET_DISPLAY_INFO);
        let info: RespDisplayInfo = self.submit_control(&req)?;
        check_response_code(info.hdr.kind, RESP_OK_DISPLAY_INFO)?;

        let max_scanouts = (self.config.num_scanouts as usize).min(info.pmodes.len());
        let enabled = info.pmodes[..max_scanouts]
            .iter()
            .copied()
            .enumerate()
            .filter(|(_, pmode)| pmode.enabled != 0 && pmode.rect.width > 0 && pmode.rect.height > 0)
            .map(|(idx, pmode)| (idx as u32, pmode))
            .collect();
        Ok(enabled)
    }

    /// Queries `VIRTIO_GPU_CMD_GET_EDID` for `scanout` when `FEATURE_EDID` is negotiated.
    fn query_edid(&self, scanout: u32) -> Option<Vec<u8>> {
        if self.transport.negotiated_features() & FEATURE_EDID == 0 {
            return None;
        }
        let req = GetEdid {
            hdr: CtrlHdr::new(CMD_GET_EDID),
            scanout,
            pad: 0,
        };
        let resp: RespEdid = self.submit_control(&req).ok()?;
        check_response_code(resp.hdr.kind, RESP_OK_EDID).ok()?;
        let size = (resp.size as usize).min(EDID_MAX_BYTES);
        if size == 0 {
            return None;
        }
        Some(resp.edid[..size].to_vec())
    }

    /// Creates a 2D host resource and attaches `object`'s physical backing.
    /// Automatically rolls back with `RESOURCE_UNREF` if `ATTACH_BACKING` fails.
    fn create_and_attach_resource(
        &self,
        resource_id: u32,
        object: &GemObject,
        format: GpuFormat,
        width: u32,
        height: u32,
    ) -> Result<()> {
        let create_cmd = build_create_2d(resource_id, format, width, height)?;
        let attach_cmd = build_attach_backing(resource_id, object.paddr() as u64, object.size())?;

        self.submit_control_nodata(&create_cmd)?;
        if let Err(err) = self.submit_control_nodata(&attach_cmd) {
            let _ = self.submit_control_nodata(&build_unref(resource_id));
            return Err(err);
        }
        Ok(())
    }

    /// Detaches backing memory from `resource_id` and unreferences it on the host.
    /// If `DETACH_BACKING` fails, `RESOURCE_UNREF` is skipped so the host does
    /// not access unreferenced state inconsistently.
    fn detach_and_unref_resource(&self, resource_id: u32) {
        if resource_id == 0 {
            return;
        }
        if self
            .submit_control_nodata(&build_detach_backing(resource_id))
            .is_ok()
        {
            let _ = self.submit_control_nodata(&build_unref(resource_id));
        }
    }

    /// Returns the [`ResourceGeometry`] for `fb`, creating and attaching the
    /// host 2D resource on first use.
    fn ensure_fb_resource(&self, fb: &Framebuffer) -> Result<ResourceGeometry> {
        if let Some(existing) = self.pipeline.lock().fb_resource(fb.id) {
            return Ok(existing);
        }

        let format = GpuFormat::from_drm_fourcc(fb.format)?;
        let resource_id = self.pipeline.lock().alloc_resource_id()?;
        self.create_and_attach_resource(resource_id, &fb.object, format, fb.width, fb.height)?;

        let geometry = ResourceGeometry {
            resource_id,
            format,
            width: fb.width,
            height: fb.height,
            pitch: fb.pitch,
        };
        self.pipeline.lock().register_fb_resource(fb.id, geometry);
        Ok(geometry)
    }

    /// Transfers `rect` of `geometry` from guest DMA memory to the host resource
    /// and flushes the region to any bound scanouts.
    fn transfer_and_flush(&self, geometry: &ResourceGeometry, rect: Rect) -> Result<()> {
        let offset = self.pipeline.lock().transfer_offset_for(geometry, rect);
        self.submit_control_nodata(&build_transfer_2d(geometry.resource_id, rect, offset))?;
        self.submit_control_nodata(&build_flush(geometry.resource_id, rect))
    }

    /// Probes enabled scanouts and their EDID blobs to build the KMS state.
    fn build_kms(self: &Arc<Self>) -> Result<&'static KmsState> {
        let scanouts = self.query_display_info()?;
        ostd::info!("virtio-gpu: {} enabled scanout(s) detected", scanouts.len());
        if scanouts.is_empty() {
            crate::return_errno!(ENODEV, "virtio-gpu host exposes no enabled scanout");
        }

        let pipelines: Vec<KmsPipeline> = scanouts
            .into_iter()
            .map(|(scanout_id, pmode)| {
                let (width, height) = (pmode.rect.width, pmode.rect.height);
                let edid = self.query_edid(scanout_id);
                let modes = build_scanout_modes(width, height, edid.as_deref());
                KmsPipeline {
                    modes,
                    connector_type: drm::DRM_MODE_CONNECTOR_VIRTUAL,
                    encoder_type: drm::DRM_MODE_ENCODER_VIRTUAL,
                    mm_width: 320,
                    mm_height: 240,
                }
            })
            .collect();

        let state = KmsState::new(Arc::clone(self), pipelines);
        Ok(Box::leak(Box::new(state)))
    }
}

impl DisplayDriver for VirtioGpu {
    fn set_scanout(
        &self,
        pipeline: usize,
        fb: &Framebuffer,
        mode: &drm::DrmModeModeInfo,
    ) -> Result<()> {
        let geometry = self.ensure_fb_resource(fb)?;
        let rect = Rect::sized(u32::from(mode.hdisplay), u32::from(mode.vdisplay));
        if !rect.fits(geometry.width, geometry.height) {
            crate::return_errno!(EINVAL, "mode dimensions exceed framebuffer resource");
        }
        let offset = self.pipeline.lock().transfer_offset_for(&geometry, rect);
        self.submit_control_nodata(&build_transfer_2d(geometry.resource_id, rect, offset))?;
        self.submit_control_nodata(&build_set_scanout(
            pipeline as u32,
            geometry.resource_id,
            rect,
        ))?;
        self.submit_control_nodata(&build_flush(geometry.resource_id, rect))
    }

    fn disable_scanout(&self, pipeline: usize) -> Result<()> {
        self.submit_control_nodata(&build_set_scanout(pipeline as u32, 0, Rect::default()))
    }

    fn refresh(&self, _pipeline: usize, fb: &Framebuffer) -> Result<()> {
        let geometry = self.ensure_fb_resource(fb)?;
        self.transfer_and_flush(&geometry, geometry.full_rect())
    }

    fn refresh_rect(
        &self,
        _pipeline: usize,
        fb: &Framebuffer,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    ) -> Result<()> {
        let geometry = self.ensure_fb_resource(fb)?;
        let Some(rect) = geometry.clamp_rect(x, y, width, height) else {
            return Ok(());
        };
        self.transfer_and_flush(&geometry, rect)
    }

    fn release_fb(&self, fb: &Framebuffer) {
        let removed = self.pipeline.lock().take_fb_resource(fb.id);
        if let Some(geometry) = removed {
            self.detach_and_unref_resource(geometry.resource_id);
        }
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
        let pos_x = x.max(0) as u32;
        let pos_y = y.max(0) as u32;
        let hotspot_x = hot_x.max(0) as u32;
        let hotspot_y = hot_y.max(0) as u32;

        let Some(object) = bo else {
            let (old_res, cmd) = {
                let mut pipe = self.pipeline.lock();
                let state = pipe.cursor_mut(pipeline).ok_or(Errno::EINVAL)?;
                let prev = state.resource_id;
                state.resource_id = 0;
                state.bo_paddr = 0;
                state.x = pos_x;
                state.y = pos_y;
                state.hot_x = hotspot_x;
                state.hot_y = hotspot_y;
                (prev, build_update_cursor(state))
            };
            self.submit_cursor(&cmd)?;
            if old_res != 0 {
                self.detach_and_unref_resource(old_res);
            }
            return Ok(());
        };

        validate_cursor_dimensions(width, height, object.size())?;
        let paddr = object.paddr();
        let existing_res = self
            .pipeline
            .lock()
            .cursor(pipeline)
            .ok_or(Errno::EINVAL)?
            .matching_resource(paddr);

        let (resource_id, old_res_to_free) = if let Some(res_id) = existing_res {
            (res_id, 0)
        } else {
            let new_res = self.pipeline.lock().alloc_resource_id()?;
            self.create_and_attach_resource(
                new_res,
                object,
                GpuFormat::B8G8R8A8Unorm,
                CURSOR_SIZE,
                CURSOR_SIZE,
            )?;
            let prev = self
                .pipeline
                .lock()
                .cursor(pipeline)
                .map(|c| c.resource_id)
                .unwrap_or(0);
            (new_res, prev)
        };

        let cursor_rect = Rect::sized(CURSOR_SIZE, CURSOR_SIZE);
        self.submit_control_nodata(&build_transfer_2d(resource_id, cursor_rect, 0))?;

        let cmd = {
            let mut pipe = self.pipeline.lock();
            let state = pipe.cursor_mut(pipeline).ok_or(Errno::EINVAL)?;
            state.resource_id = resource_id;
            state.bo_paddr = paddr;
            state.x = pos_x;
            state.y = pos_y;
            state.hot_x = hotspot_x;
            state.hot_y = hotspot_y;
            build_update_cursor(state)
        };
        self.submit_cursor(&cmd)?;

        if old_res_to_free != 0 && old_res_to_free != resource_id {
            self.detach_and_unref_resource(old_res_to_free);
        }
        Ok(())
    }

    fn move_cursor(&self, pipeline: usize, x: i32, y: i32) -> Result<()> {
        let cmd = {
            let mut pipe = self.pipeline.lock();
            let state = pipe.cursor_mut(pipeline).ok_or(Errno::EINVAL)?;
            state.x = x.max(0) as u32;
            state.y = y.max(0) as u32;
            if state.resource_id == 0 {
                return Ok(());
            }
            build_move_cursor(state)
        };
        self.submit_cursor(&cmd)
    }
}

impl DrmDevice for VirtioGpu {
    fn driver_name(&self) -> &'static str {
        "virtio-gpu"
    }

    fn driver_info(&self) -> (String, String) {
        (
            format!("20260101"),
            format!("Virtio GPU 2D (KMS + EDID + Cursor)"),
        )
    }

    fn kms(&self) -> Option<&'static KmsState> {
        self.kms.get().copied()
    }
}

/// VirtIO bus registration adapter for `virtio-gpu`.
pub struct VirtioGpuDevice;

impl VirtioDevice for VirtioGpuDevice {
    fn device_id(&self) -> u16 {
        PCI_DEVICE_ID_VIRTIO_GPU
    }

    fn name(&self) -> &'static str {
        "virtio-gpu"
    }

    /// Requests both the control queue (0) and the cursor queue (1).
    fn queue_count(&self) -> usize {
        2
    }

    fn accept_features(&self, device_features: u64) -> u64 {
        device_features & (VIRTIO_F_VERSION_1 | FEATURE_EDID)
    }

    fn on_init(&self, transport: &Arc<VirtioTransport>) -> Result<()> {
        let gpu = VirtioGpu::new(transport)?;
        let kms = gpu.build_kms()?;
        gpu.kms.call_once(|| kms);
        let _card = register_drm_device(gpu);
        Ok(())
    }
}
