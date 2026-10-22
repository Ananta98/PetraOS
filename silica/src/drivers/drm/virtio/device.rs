// SPDX-License-Identifier: GPL-2.0

//! The virtio-gpu control plane.
//!
//! [`VirtioGpuDevice`] is the PCI-facing glue; it builds one [`VirtioGpu`]
//! per attached transport. [`VirtioGpu`] turns the KMS scanout callbacks
//! into the 2D command stream of the device (display info, resource
//! create/attach, set scanout, flush) while tracking dumb buffers as
//! resources keyed by their framebuffer id.

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::mem::size_of;
use ostd::mm::{HasPaddr, VmIo, dma::DmaCoherent, io::util::HasVmReaderWriter};
use spin::{Mutex, Once};

use crate::drivers::bus::virtio::{SubmitBuffer, VirtQueue, VirtioDevice, VirtioTransport};
use crate::drivers::drm::core::{DrmDevice, register_drm_device};
use crate::drivers::drm::gem::GemObject;
use crate::drivers::drm::kms::{DisplayDriver, Framebuffer, KmsPipeline, KmsState};
use crate::uapi::drm;
use crate::uapi::errno::{Errno, Result};

// 2D control commands (`VIRTIO_GPU_CMD_*`) and responses.
const VG_CMD_GET_DISPLAY_INFO: u32 = 0x0100;
const VG_CMD_RESOURCE_CREATE_2D: u32 = 0x0101;
const VG_CMD_SET_SCANOUT: u32 = 0x0103;
const VG_CMD_RESOURCE_FLUSH: u32 = 0x0104;
const VG_CMD_TRANSFER_TO_HOST_2D: u32 = 0x0105;
const VG_CMD_RESOURCE_ATTACH_BACKING: u32 = 0x0106;
const VG_RESP_OK_NODATA: u32 = 0x1100;
const VG_RESP_OK_DISPLAY_INFO: u32 = 0x1101;
/// `VIRTIO_GPU_MAX_SCANOUTS`.
const VG_MAX_SCANOUTS: usize = 16;

/// `struct virtio_gpu_ctrl_hdr`, the first 24 bytes in both directions.
#[repr(C)]
#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    zerocopy::FromBytes,
    zerocopy::IntoBytes,
    zerocopy::KnownLayout,
    zerocopy::Immutable,
)]
struct VgCtrlHdr {
    kind: u32,
    flags: u32,
    fence_id: u64,
    ctx_id: u32,
    pad: u32,
}

/// `struct virtio_gpu_rect`.
#[repr(C)]
#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    zerocopy::FromBytes,
    zerocopy::IntoBytes,
    zerocopy::KnownLayout,
    zerocopy::Immutable,
)]
struct VgRect {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

/// `struct virtio_gpu_resource_create_2d`.
#[repr(C)]
#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    zerocopy::FromBytes,
    zerocopy::IntoBytes,
    zerocopy::KnownLayout,
    zerocopy::Immutable,
)]
struct VgResourceCreate2d {
    hdr: VgCtrlHdr,
    resource_id: u32,
    format: u32,
    width: u32,
    height: u32,
}

/// `struct virtio_gpu_resource_attach_backing` + one `mem_entry`.
#[repr(C)]
#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    zerocopy::FromBytes,
    zerocopy::IntoBytes,
    zerocopy::KnownLayout,
    zerocopy::Immutable,
)]
struct VgAttachBacking {
    hdr: VgCtrlHdr,
    resource_id: u32,
    nr_entries: u32,
    entry_addr: u64,
    entry_len: u32,
    entry_pad: u32,
}

/// `struct virtio_gpu_set_scanout`.
#[repr(C)]
#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    zerocopy::FromBytes,
    zerocopy::IntoBytes,
    zerocopy::KnownLayout,
    zerocopy::Immutable,
)]
struct VgSetScanout {
    hdr: VgCtrlHdr,
    rect: VgRect,
    scanout_id: u32,
    resource_id: u32,
}

/// `struct virtio_gpu_resource_flush`.
#[repr(C)]
#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    zerocopy::FromBytes,
    zerocopy::IntoBytes,
    zerocopy::KnownLayout,
    zerocopy::Immutable,
)]
struct VgResourceFlush {
    hdr: VgCtrlHdr,
    rect: VgRect,
    resource_id: u32,
    pad: u32,
}

/// `struct virtio_gpu_transfer_to_host_2d`.
#[repr(C)]
#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    zerocopy::FromBytes,
    zerocopy::IntoBytes,
    zerocopy::KnownLayout,
    zerocopy::Immutable,
)]
struct VgTransferToHost2d {
    hdr: VgCtrlHdr,
    rect: VgRect,
    offset: u64,
    resource_id: u32,
    pad: u32,
}

/// `struct virtio_gpu_display_one` from `resp_display_info`.
#[repr(C)]
#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    zerocopy::FromBytes,
    zerocopy::IntoBytes,
    zerocopy::KnownLayout,
    zerocopy::Immutable,
)]
struct VgDisplayOne {
    rect: VgRect,
    enabled: u32,
    flags: u32,
}

/// `struct virtio_gpu_resp_display_info` (response of GET_DISPLAY_INFO).
#[repr(C)]
#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    zerocopy::FromBytes,
    zerocopy::IntoBytes,
    zerocopy::KnownLayout,
    zerocopy::Immutable,
)]
struct VgDisplayInfo {
    hdr: VgCtrlHdr,
    num_scanouts: u32,
    pad: u32,
    pmodes: [VgDisplayOne; VG_MAX_SCANOUTS],
}

/// The virtio-gpu 2D formats matching the scanout-capable DRM formats.
const VG_FORMAT_B8G8R8A8_UNORM: u32 = 1;
const VG_FORMAT_B8G8R8X8_UNORM: u32 = 2;

/// The control path of the device is queue 0; the cursor queue is unused.
const CONTROL_QUEUE: usize = 0;

/// One live virtio-gpu instance (one attached PCI device).
pub struct VirtioGpu {
    transport: Arc<VirtioTransport>,
    /// Control queue 0.
    control: Arc<VirtQueue>,
    /// Staging DMA buffer for command structs (whole page).
    request_dma: DmaCoherent,
    /// Staging DMA buffer for responses (display info needs 416 bytes).
    response_dma: DmaCoherent,
    /// Next 2D resource id; 0 is disabled/reserved on the device side.
    next_resource_id: Mutex<u32>,
    /// Framebuffer id → host resource id.
    resources: Mutex<BTreeMap<u32, u32>>,
    /// The KMS object tables (leaked; see [`DrmDevice::kms`]).
    kms: Once<&'static KmsState>,
}

impl VirtioGpu {
    fn new(transport: &Arc<VirtioTransport>) -> Result<Arc<Self>> {
        let Some(control) = transport.queue(CONTROL_QUEUE) else {
            ostd::info!(
                "virtio-gpu: no control queue; activated = {}",
                transport.queues().len()
            );
            crate::return_errno!(ENODEV, "device does not expose its control queue");
        };
        Ok(Arc::new(Self {
            transport: Arc::clone(transport),
            control,
            request_dma: DmaCoherent::alloc(1, false).map_err(|_| Errno::ENOMEM)?,
            response_dma: DmaCoherent::alloc(1, false).map_err(|_| Errno::ENOMEM)?,
            next_resource_id: Mutex::new(1),
            resources: Mutex::new(BTreeMap::new()),
            kms: Once::new(),
        }))
    }

    /// Submits `request` and expects a `resp_len`-byte response.
    fn ctrl<T: zerocopy::IntoBytes + zerocopy::Immutable>(
        &self,
        request: &T,
        resp_len: usize,
    ) -> Result<()> {
        let bytes = request.as_bytes();
        if bytes.len() > ostd::mm::PAGE_SIZE || resp_len > ostd::mm::PAGE_SIZE {
            crate::return_errno!(EINVAL, "command overflows the staging page");
        }
        {
            let mut writer = self.request_dma.writer();
            writer.write(&mut ostd::mm::VmReader::from(bytes));
        }
        self.control
            .submit_wait(&[
                SubmitBuffer {
                    addr: self.request_dma.paddr() as u64,
                    len: bytes.len() as u32,
                    writable: false,
                },
                SubmitBuffer {
                    addr: self.response_dma.paddr() as u64,
                    len: resp_len as u32,
                    writable: true,
                },
            ])
            .map(|_| ())
    }

    fn ctrl_with_response<
        T: zerocopy::IntoBytes + zerocopy::Immutable,
        R: zerocopy::FromBytes
            + zerocopy::IntoBytes
            + zerocopy::KnownLayout
            + zerocopy::Immutable
            + Copy,
    >(
        &self,
        request: &T,
    ) -> Result<R> {
        self.ctrl(request, size_of::<R>())?;
        self.response_dma.read_val::<R>(0).map_err(|_| Errno::EIO)
    }

    fn response_kind(&self) -> Result<u32> {
        self.response_dma
            .read_val::<VgCtrlHdr>(0)
            .map(|hdr| hdr.kind)
            .map_err(|_| Errno::EIO)
    }

    /// Submits `request` and requires `VIRTIO_GPU_RESP_OK_NODATA`.
    fn ctrl_ok<T: zerocopy::IntoBytes + zerocopy::Immutable>(&self, request: &T) -> Result<()> {
        self.ctrl(request, size_of::<VgCtrlHdr>())?;
        if self.response_kind()? != VG_RESP_OK_NODATA {
            crate::return_errno!(EIO, "device rejected the command");
        }
        Ok(())
    }

    /// `VIRTIO_GPU_CMD_GET_DISPLAY_INFO`.
    fn get_display_info(&self) -> Result<Vec<VgDisplayOne>> {
        let info: VgDisplayInfo = self.ctrl_with_response(&VgCtrlHdr {
            kind: VG_CMD_GET_DISPLAY_INFO,
            ..Default::default()
        })?;
        if info.hdr.kind != VG_RESP_OK_DISPLAY_INFO {
            crate::return_errno!(EIO, "display info protocol error");
        }
        Ok(info
            .pmodes
            .iter()
            .copied()
            .filter(|pmode| pmode.enabled == 1 && pmode.rect.width > 0 && pmode.rect.height > 0)
            .collect())
    }

    /// Creates a 2D resource covering `geometry` backed by `object`.
    fn create_resource(
        &self,
        object: &GemObject,
        format: u32,
        width: u32,
        height: u32,
    ) -> Result<u32> {
        let resource_id = {
            let mut next = self.next_resource_id.lock();
            let id = *next;
            *next += 1;
            id
        };
        self.ctrl_ok(&VgResourceCreate2d {
            hdr: VgCtrlHdr {
                kind: VG_CMD_RESOURCE_CREATE_2D,
                ..Default::default()
            },
            resource_id,
            format,
            width,
            height,
        })?;
        self.ctrl_ok(&VgAttachBacking {
            hdr: VgCtrlHdr {
                kind: VG_CMD_RESOURCE_ATTACH_BACKING,
                ..Default::default()
            },
            resource_id,
            nr_entries: 1,
            entry_addr: object.paddr() as u64,
            entry_len: object.size() as u32,
            entry_pad: 0,
        })?;
        Ok(resource_id)
    }

    fn set_scanout_cmd(&self, scanout: u32, resource: u32, rect: VgRect) -> Result<()> {
        self.ctrl_ok(&VgSetScanout {
            hdr: VgCtrlHdr {
                kind: VG_CMD_SET_SCANOUT,
                ..Default::default()
            },
            rect,
            scanout_id: scanout,
            resource_id: resource,
        })
    }

    fn flush_cmd(&self, resource: u32, rect: VgRect) -> Result<()> {
        self.ctrl_ok(&VgResourceFlush {
            hdr: VgCtrlHdr {
                kind: VG_CMD_RESOURCE_FLUSH,
                ..Default::default()
            },
            rect,
            resource_id: resource,
            pad: 0,
        })
    }

    fn transfer_cmd(&self, resource: u32, rect: VgRect, offset: u64) -> Result<()> {
        self.ctrl_ok(&VgTransferToHost2d {
            hdr: VgCtrlHdr {
                kind: VG_CMD_TRANSFER_TO_HOST_2D,
                ..Default::default()
            },
            rect,
            offset,
            resource_id: resource,
            pad: 0,
        })
    }

    /// The host resource of `fb`, created on first use. Dumb buffers are
    /// whole-object 2D surfaces, so one framebuffer is exactly one resource.
    fn resource_for_fb(&self, fb: &Framebuffer) -> Result<u32> {
        if let Some(id) = self.resources.lock().get(&fb.id) {
            return Ok(*id);
        }
        let format = match fb.format {
            drm::DRM_FORMAT_XRGB8888 => VG_FORMAT_B8G8R8X8_UNORM,
            drm::DRM_FORMAT_ARGB8888 => VG_FORMAT_B8G8R8A8_UNORM,
            _ => crate::return_errno!(EINVAL, "format not supported by the host"),
        };
        let resource = self.create_resource(&fb.object, format, fb.width, fb.height)?;
        self.resources.lock().insert(fb.id, resource);
        Ok(resource)
    }

    /// Builds the KMS pipelines from the enabled scanouts reported by the
    /// host and leaks the state (the trait hands out `'static`).
    fn build_kms(self: &Arc<Self>) -> Result<&'static KmsState> {
        let scanouts = self.get_display_info()?;
        ostd::info!("virtio-gpu: {} scanout(s) reported", scanouts.len());
        if scanouts.is_empty() {
            crate::return_errno!(ENODEV, "host exposes no enabled scanout");
        }
        let pipelines: alloc::vec::Vec<KmsPipeline> = scanouts
            .iter()
            .map(|pmode| {
                let (width, height) = (pmode.rect.width, pmode.rect.height);
                let mut mode = drm::DrmModeModeInfo::default();
                mode.hdisplay = width as u16;
                mode.htotal = width as u16;
                mode.vdisplay = height as u16;
                mode.vtotal = height as u16;
                mode.type_ = drm::DRM_MODE_TYPE_PREFERRED;
                let mut name = [0u8; drm::DRM_DISPLAY_MODE_LEN];
                let label = alloc::format!("{width}x{height}");
                let used = label.len().min(drm::DRM_DISPLAY_MODE_LEN - 1);
                name[..used].copy_from_slice(&label.as_bytes()[..used]);
                mode.name = name;
                KmsPipeline {
                    modes: alloc::vec![mode],
                    connector_type: drm::DRM_MODE_CONNECTOR_VIRTUAL,
                    encoder_type: drm::DRM_MODE_ENCODER_VIRTUAL,
                    // A virtual display has no measurable physical sink.
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
    /// Programs `pipeline` with `fb`, then flushes it to the host surface.
    fn set_scanout(
        &self,
        pipeline: usize,
        fb: &Framebuffer,
        mode: &crate::uapi::drm::DrmModeModeInfo,
    ) -> Result<()> {
        let resource = self.resource_for_fb(fb)?;
        let rect = VgRect {
            x: 0,
            y: 0,
            width: mode.hdisplay as u32,
            height: mode.vdisplay as u32,
        };
        self.set_scanout_cmd(pipeline as u32, resource, rect)?;
        self.flush_cmd(resource, rect)
    }

    fn disable_scanout(&self, pipeline: usize) -> Result<()> {
        // Resource 0 is "no resource" per the spec.
        self.set_scanout_cmd(pipeline as u32, 0, VgRect::default())
    }

    /// The `DIRTYFB` half: copy the guest buffer up, then flush it out.
    fn refresh(&self, pipeline: usize, fb: &Framebuffer) -> Result<()> {
        let resource = self.resource_for_fb(fb)?;
        let rect = VgRect {
            x: 0,
            y: 0,
            width: fb.width,
            height: fb.height,
        };
        self.transfer_cmd(resource, rect, 0)?;
        self.flush_cmd(resource, rect)?;
        let _ = pipeline; // the flush covers the whole buffer surface
        Ok(())
    }
}

impl DrmDevice for VirtioGpu {
    fn driver_name(&self) -> &'static str {
        "virtio-gpu"
    }

    fn driver_info(&self) -> (alloc::string::String, alloc::string::String) {
        (
            alloc::format!("20260101"),
            alloc::format!("Virtio GPU 2D (dumb buffers)"),
        )
    }

    fn kms(&self) -> Option<&'static KmsState> {
        self.kms.get().copied()
    }
}

/// The PCI-facing registration glue for the virtio bus.
pub struct VirtioGpuDevice;

impl VirtioDevice for VirtioGpuDevice {
    fn device_id(&self) -> u16 {
        PCI_DEVICE_ID_VIRTIO_GPU
    }

    fn name(&self) -> &'static str {
        "virtio-gpu"
    }

    /// The control queue; the cursor queue is not used by this driver.
    fn queue_count(&self) -> usize {
        1
    }

    fn on_init(&self, transport: &Arc<VirtioTransport>) -> Result<()> {
        let gpu = VirtioGpu::new(transport)?;
        let kms = gpu.build_kms()?;
        gpu.kms.call_once(|| kms);
        let _card = register_drm_device(gpu);
        Ok(())
    }
}

/// virtio-gpu over PCI: `VIRTIO_ID_GPU = 16`, i.e. modern id 0x1050.
const PCI_DEVICE_ID_VIRTIO_GPU: u16 = 0x1050;
