// SPDX-License-Identifier: MPL-2.0

//! Framebuffer Hardware Abstraction.
//!
//! Based on Asterinas framebuffer abstraction.
//! Provides memory-mapped linear framebuffer access, offset calculations,
//! hardware colormap emulation, and low-level blitting.

use alloc::{vec, vec::Vec};
use ostd::{
    boot::boot_info,
    io::IoMem,
    mm::{CachePolicy, HasSize, VmIo},
    sync::Mutex,
};

use super::pixel::{Pixel, PixelFormat, RenderedPixel};
use crate::api::errno::{Errno, Result};

use alloc::sync::Arc;
use spin::Once;

pub static FRAMEBUFFER: Once<Arc<FrameBuffer>> = Once::new();

pub const MAX_CMAP_SIZE: usize = 256;

/// A single entry in the colormap with 16-bit color values (Linux standard).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Default)]
pub struct ColorMapEntry {
    pub red: u16,
    pub green: u16,
    pub blue: u16,
    pub transp: u16,
}

#[derive(Clone, Debug, Default)]
struct FbCmap {
    entries: Vec<ColorMapEntry>,
}

/// The physical linear framebuffer.
#[derive(Debug)]
pub struct FrameBuffer {
    io_mem: IoMem,
    pub base_address: usize,
    pub total_size: usize,
    pub width: usize,
    pub height: usize,
    pub line_size: usize,
    pub pixel_format: PixelFormat,
    cmap: Mutex<FbCmap>,
}

pub type Framebuffer = FrameBuffer;

impl FrameBuffer {
    /// Probes the linear framebuffer provided by boot information.
    pub fn probe() -> Result<Self> {
        let Some(arg) = boot_info().framebuffer_arg else {
            ostd::warn!("Framebuffer: No framebuffer argument found in boot info");
            return Err(Errno::ENODEV);
        };

        if arg.address == 0 || arg.width == 0 || arg.height == 0 {
            ostd::error!("Framebuffer: Invalid framebuffer parameters: {:?}", arg);
            return Err(Errno::EINVAL);
        }

        let pixel_format = match arg.bpp {
            8 => PixelFormat::Grayscale8,
            16 => PixelFormat::Rgb565,
            24 => PixelFormat::Rgb888,
            32 => PixelFormat::BgrReserved,
            _ => {
                ostd::warn!(
                    "Framebuffer: Unsupported BPP {}, defaulting to 32 BPP",
                    arg.bpp
                );
                PixelFormat::BgrReserved
            }
        };

        let line_size = arg
            .width
            .checked_mul(pixel_format.nbytes())
            .ok_or(Errno::EINVAL)?;
        let total_size = arg.height.checked_mul(line_size).ok_or(Errno::EINVAL)?;
        let range = arg.address..(arg.address + total_size);

        ostd::info!(
            "Framebuffer: Acquiring MMIO at {:#x}..{:#x} ({}x{} @ {} bpp)",
            range.start,
            range.end,
            arg.width,
            arg.height,
            arg.bpp
        );

        let io_mem = IoMem::acquire_with_cache_policy(range.clone(), CachePolicy::WriteCombining)
            .or_else(|_| IoMem::acquire(range))
            .map_err(|_| {
                ostd::error!("Framebuffer: Failed to acquire MMIO memory region");
                Errno::EIO
            })?;

        let fb = Self {
            io_mem,
            base_address: arg.address,
            total_size,
            width: arg.width,
            height: arg.height,
            line_size,
            pixel_format,
            cmap: Mutex::new(FbCmap::default()),
        };

        fb.clear();
        Ok(fb)
    }

    /// Returns the width of the framebuffer in pixels.
    pub fn width(&self) -> usize {
        self.width
    }

    /// Returns the height of the framebuffer in pixels.
    pub fn height(&self) -> usize {
        self.height
    }

    /// Returns the line size of the framebuffer in bytes.
    pub fn line_size(&self) -> usize {
        self.line_size
    }

    /// Returns a reference to the `IoMem` instance of the framebuffer.
    pub fn io_mem(&self) -> &IoMem {
        &self.io_mem
    }

    /// Returns the pixel format of the framebuffer.
    pub fn pixel_format(&self) -> PixelFormat {
        self.pixel_format
    }

    /// Renders the pixel according to the pixel format of the framebuffer.
    pub fn render_pixel(&self, pixel: Pixel) -> RenderedPixel {
        pixel.render(self.pixel_format)
    }

    /// Calculates the offset of a pixel at the specified coordinate.
    pub fn calc_offset(&self, x: usize, y: usize) -> PixelOffset<'_> {
        PixelOffset {
            fb: self,
            offset: (x * self.pixel_format.nbytes() + y * self.line_size) as isize,
        }
    }

    /// Writes a pixel at the specified position.
    pub fn write_pixel_at(&self, offset: PixelOffset, pixel: RenderedPixel) -> Result<()> {
        self.write_bytes_at(offset.as_usize(), pixel.as_slice())
    }

    /// Writes raw bytes at the specified offset.
    pub fn write_bytes_at(&self, offset: usize, bytes: &[u8]) -> Result<()> {
        if offset + bytes.len() > self.total_size {
            return Err(Errno::EINVAL);
        }
        self.io_mem
            .write_bytes(offset, bytes)
            .map_err(|_| Errno::EIO)
    }

    /// Writes a 32-bit pixel value directly at the specified offset.
    #[inline(always)]
    pub fn write_pixel_u32(&self, offset: usize, pixel: u32) -> Result<()> {
        if offset + 4 > self.total_size {
            return Err(Errno::EINVAL);
        }
        self.io_mem
            .write_val(offset, &pixel)
            .map_err(|_| Errno::EIO)
    }

    /// Writes a 16-bit pixel value directly at the specified offset.
    #[inline(always)]
    pub fn write_pixel_u16(&self, offset: usize, pixel: u16) -> Result<()> {
        if offset + 2 > self.total_size {
            return Err(Errno::EINVAL);
        }
        self.io_mem
            .write_val(offset, &pixel)
            .map_err(|_| Errno::EIO)
    }

    /// Writes raw bytes at the specified offset (alias for write_bytes_at).
    pub fn write_bytes(&self, offset: usize, bytes: &[u8]) -> Result<()> {
        self.write_bytes_at(offset, bytes)
    }

    /// Read raw bytes from the specified offset.
    pub fn read_bytes(&self, offset: usize, buf: &mut [u8]) -> Result<()> {
        if offset + buf.len() > self.total_size {
            return Err(Errno::EINVAL);
        }
        self.io_mem.read_bytes(offset, buf).map_err(|_| Errno::EIO)
    }

    /// Clears the framebuffer with black color.
    pub fn clear(&self) {
        let frame = vec![0u8; self.io_mem.size()];
        let _ = self.write_bytes_at(0, &frame);
    }

    /// Sets color map entries starting from the given index.
    pub fn set_color_map(&self, start: usize, entries: &[ColorMapEntry]) -> Result<()> {
        if start > MAX_CMAP_SIZE || entries.len() > MAX_CMAP_SIZE - start {
            return Err(Errno::EINVAL);
        }

        let mut cmap = self.cmap.lock();
        let required_len = start + entries.len();
        if cmap.entries.len() < required_len {
            cmap.entries.resize(required_len, ColorMapEntry::default());
        }

        cmap.entries[start..start + entries.len()].copy_from_slice(entries);
        Ok(())
    }

    /// Gets color map entries from the given range.
    pub fn get_color_map(&self, start: usize, len: usize) -> Option<Vec<ColorMapEntry>> {
        let cmap = self.cmap.lock();
        if start >= cmap.entries.len() || len > cmap.entries.len() - start {
            return None;
        }
        Some(cmap.entries[start..start + len].to_vec())
    }
}

/// The offset of a pixel in the framebuffer.
#[derive(Clone, Copy, Debug)]
pub struct PixelOffset<'a> {
    fb: &'a FrameBuffer,
    offset: isize,
}

impl PixelOffset<'_> {
    /// Adds the specified delta to the x coordinate.
    pub fn x_add(&mut self, x_delta: isize) {
        let delta = x_delta * self.fb.pixel_format.nbytes() as isize;
        self.offset += delta;
    }

    /// Adds the specified delta to the y coordinate.
    pub fn y_add(&mut self, y_delta: isize) {
        let delta = y_delta * self.fb.line_size as isize;
        self.offset += delta;
    }

    /// Returns the offset value as a `usize`.
    pub fn as_usize(&self) -> usize {
        self.offset as usize
    }
}

pub fn init() -> Result<()> {
    let fb = FrameBuffer::probe()?;
    let fb_arc = Arc::new(fb);
    let _ = FRAMEBUFFER.call_once(|| Arc::clone(&fb_arc));
    ostd::info!("Framebuffer: Physical framebuffer successfully initialized");
    Ok(())
}

pub fn get_framebuffer() -> Option<Arc<FrameBuffer>> {
    FRAMEBUFFER.get().cloned()
}
