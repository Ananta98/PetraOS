/// VirtIO GPU Device Implementation and GpuDriver Trait Integration.

use super::regs::*;
use super::virtqueue::Virtqueue;
use crate::drivers::char::virtio_console::VirtioBar;
use crate::drivers::gpu::GpuDriver;
use crate::drivers::gpu::framebuffer::{Framebuffer, PixelFormat, VideoMode};

use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use ostd::mm::dma::DmaCoherent;
use ostd::mm::{HasDaddr, HasSize, VmIo};
use ostd::sync::SpinLock;

/// Standard supported video modes for VirtIO GPU.
static SUPPORTED_MODES: [VideoMode; 3] = [
    VideoMode {
        width: 1024,
        height: 768,
        pitch: 1024 * 4,
        bpp: 32,
        format: PixelFormat::Rgba8888,
    },
    VideoMode {
        width: 800,
        height: 600,
        pitch: 800 * 4,
        bpp: 32,
        format: PixelFormat::Rgba8888,
    },
    VideoMode {
        width: 640,
        height: 480,
        pitch: 640 * 4,
        bpp: 32,
        format: PixelFormat::Rgba8888,
    },
];

pub enum VirtioGpuDeviceInner {
    Physical {
        bar: VirtioBar,
        control_vq: Virtqueue,
        cmd_dma: DmaCoherent,
        fb_dma: DmaCoherent,
        fb: Arc<Framebuffer>,
        mode: VideoMode,
        resource_id: u32,
    },
    Simulated {
        fb: Arc<Framebuffer>,
        mode: VideoMode,
    },
}

pub struct VirtioGpuDevice {
    pub name: String,
    pub inner: SpinLock<VirtioGpuDeviceInner>,
}

impl VirtioGpuDevice {
    /// Helper to safely serialize a VirtioGpuTransferToHost2d struct without unsafe.
    fn serialize_transfer_to_host_2d(
        type_: u32,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        offset: u64,
        resource_id: u32,
    ) -> [u8; 48] {
        let mut buf = [0u8; 48];
        buf[0..4].copy_from_slice(&type_.to_le_bytes()); // hdr.type
        buf[4..8].copy_from_slice(&0u32.to_le_bytes()); // hdr.flags
        buf[8..16].copy_from_slice(&0u64.to_le_bytes()); // hdr.fence_id
        buf[16..20].copy_from_slice(&0u32.to_le_bytes()); // hdr.ctx_id
        buf[20..24].copy_from_slice(&0u32.to_le_bytes()); // hdr.padding
        buf[24..28].copy_from_slice(&x.to_le_bytes());
        buf[28..32].copy_from_slice(&y.to_le_bytes());
        buf[32..36].copy_from_slice(&width.to_le_bytes());
        buf[36..40].copy_from_slice(&height.to_le_bytes());
        buf[40..48].copy_from_slice(&offset.to_le_bytes());
        buf[48..52].copy_from_slice(&resource_id.to_le_bytes());
        buf[52..56].copy_from_slice(&0u32.to_le_bytes()); // padding
        buf
    }

    /// Helper to safely serialize a VirtioGpuResourceFlush struct without unsafe.
    fn serialize_resource_flush(
        type_: u32,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        resource_id: u32,
    ) -> [u8; 44] {
        let mut buf = [0u8; 44];
        buf[0..4].copy_from_slice(&type_.to_le_bytes()); // hdr.type
        buf[4..8].copy_from_slice(&0u32.to_le_bytes()); // hdr.flags
        buf[8..16].copy_from_slice(&0u64.to_le_bytes()); // hdr.fence_id
        buf[16..20].copy_from_slice(&0u32.to_le_bytes()); // hdr.ctx_id
        buf[20..24].copy_from_slice(&0u32.to_le_bytes()); // hdr.padding
        buf[24..28].copy_from_slice(&x.to_le_bytes());
        buf[28..32].copy_from_slice(&y.to_le_bytes());
        buf[32..36].copy_from_slice(&width.to_le_bytes());
        buf[36..40].copy_from_slice(&height.to_le_bytes());
        buf[40..44].copy_from_slice(&resource_id.to_le_bytes());
        buf
    }

    /// Flush local framebuffer updates to the physical VirtIO GPU host display.
    pub fn flush_to_host(&self) -> Result<(), ostd::Error> {
        let mut inner = self.inner.lock();
        match &mut *inner {
            VirtioGpuDeviceInner::Physical {
                bar,
                control_vq,
                cmd_dma,
                fb_dma,
                fb,
                mode,
                resource_id,
            } => {
                // Copy current framebuffer pixel buffer into DMA coherent buffer
                let pixels = fb.pixels.lock();
                let copy_len = core::cmp::min(pixels.len(), fb_dma.size());
                fb_dma.write_bytes(0, &pixels[..copy_len])?;

                // 1. Submit TRANSFER_TO_HOST_2D command
                let cmd_bytes = Self::serialize_transfer_to_host_2d(
                    VIRTIO_GPU_CMD_TRANSFER_TO_HOST_2D,
                    0,
                    0,
                    mode.width,
                    mode.height,
                    0,
                    *resource_id,
                );
                cmd_dma.write_bytes(0, &cmd_bytes)?;

                let cmd_paddr = cmd_dma.daddr() as u64;
                control_vq.set_desc(0, cmd_paddr, cmd_bytes.len() as u32, 0, 0)?;
                control_vq.push_avail(0)?;
                bar.write_u16(LEGACY_QUEUE_NOTIFY as u16, 0);

                // 2. Submit RESOURCE_FLUSH command
                let flush_bytes = Self::serialize_resource_flush(
                    VIRTIO_GPU_CMD_RESOURCE_FLUSH,
                    0,
                    0,
                    mode.width,
                    mode.height,
                    *resource_id,
                );
                cmd_dma.write_bytes(256, &flush_bytes)?;

                let flush_paddr = cmd_dma.daddr() as u64 + 256;
                control_vq.set_desc(1, flush_paddr, flush_bytes.len() as u32, 0, 0)?;
                control_vq.push_avail(1)?;
                bar.write_u16(LEGACY_QUEUE_NOTIFY as u16, 0);

                Ok(())
            }
            VirtioGpuDeviceInner::Simulated { .. } => Ok(()),
        }
    }
}

impl GpuDriver for VirtioGpuDevice {
    fn name(&self) -> &str {
        &self.name
    }

    fn current_mode(&self) -> VideoMode {
        let inner = self.inner.lock();
        match &*inner {
            VirtioGpuDeviceInner::Physical { mode, .. } => *mode,
            VirtioGpuDeviceInner::Simulated { mode, .. } => *mode,
        }
    }

    fn set_mode(&self, mode: VideoMode) -> Result<(), ostd::Error> {
        let mut inner = self.inner.lock();
        match &mut *inner {
            VirtioGpuDeviceInner::Physical {
                fb,
                mode: current_mode,
                ..
            } => {
                if !SUPPORTED_MODES.contains(&mode) {
                    return Err(ostd::Error::InvalidArgs);
                }
                *current_mode = mode;
                let new_size = (mode.pitch * mode.height) as usize;
                let mut pixels = fb.pixels.lock();
                pixels.resize(new_size, 0);
                Ok(())
            }
            VirtioGpuDeviceInner::Simulated {
                fb,
                mode: current_mode,
            } => {
                if !SUPPORTED_MODES.contains(&mode) {
                    return Err(ostd::Error::InvalidArgs);
                }
                *current_mode = mode;
                let new_size = (mode.pitch * mode.height) as usize;
                let mut pixels = fb.pixels.lock();
                pixels.resize(new_size, 0);
                Ok(())
            }
        }
    }

    fn supported_modes(&self) -> &[VideoMode] {
        &SUPPORTED_MODES
    }

    fn framebuffer(&self) -> Arc<Framebuffer> {
        let inner = self.inner.lock();
        match &*inner {
            VirtioGpuDeviceInner::Physical { fb, .. } => fb.clone(),
            VirtioGpuDeviceInner::Simulated { fb, .. } => fb.clone(),
        }
    }
}
