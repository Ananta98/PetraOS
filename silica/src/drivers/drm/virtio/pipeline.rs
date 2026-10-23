// SPDX-License-Identifier: GPL-2.0

//! 2D resource lifecycle, damage transfer offset calculation, and scanout/cursor
//! state tracking for `virtio-gpu`.
//!
//! Translates high-level KMS operations (`attach`, `detach`, `scanout`, `flush`,
//! `cursor`) into validated `virtio-gpu` wire commands while maintaining the
//! mapping between DRM framebuffers/cursor buffers and host resource IDs.

use alloc::collections::BTreeMap;

use super::protocol::{
    AttachBackingSingle, CMD_MOVE_CURSOR, CMD_RESOURCE_ATTACH_BACKING, CMD_RESOURCE_CREATE_2D,
    CMD_RESOURCE_DETACH_BACKING, CMD_RESOURCE_FLUSH, CMD_RESOURCE_UNREF, CMD_SET_SCANOUT,
    CMD_TRANSFER_TO_HOST_2D, CMD_UPDATE_CURSOR, CURSOR_SIZE, CtrlHdr, CursorPos, DetachBacking,
    GpuFormat, MAX_DIMENSION, MAX_SCANOUTS, MemEntry, PCI_VENDOR_ID_VIRTIO, Rect, ResourceCreate2d,
    ResourceFlush, ResourceUnref, SetScanout, TransferToHost2d, UpdateCursor,
};
use crate::uapi::errno::{Errno, Result};

/// How the host device interprets `TransferToHost2d::offset` (`virtio 1.2` §5.7.6.8).
///
/// QEMU and the VirtIO specification interpret `offset` as the byte offset of
/// the rectangle's top-left pixel inside the resource backing store
/// (`y * pitch + x * bpp`). `crosvm` adds the rectangle origin a second time
/// internally, requiring `offset = 0` (`BackingStart`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TransferOffset {
    /// Byte offset of `(rect.x, rect.y)` within the backing buffer (QEMU / spec).
    #[default]
    RectangleStart,
    /// Start of the backing buffer (`0`), used by `crosvm`.
    BackingStart,
}

impl TransferOffset {
    /// Selects the transfer offset convention from PCI device and subsystem IDs.
    ///
    /// `crosvm` repeats the VirtIO PCI device ID (`0x1af4:0x1050`) in the PCI
    /// subsystem IDs, whereas QEMU uses `0x1af4:0x1100`.
    pub const fn for_device(device_id: u16, subsystem_vendor: u16, subsystem_id: u16) -> Self {
        if subsystem_vendor == PCI_VENDOR_ID_VIRTIO && subsystem_id == device_id {
            Self::BackingStart
        } else {
            Self::RectangleStart
        }
    }

    /// Computes the byte offset for `rect` within a surface of `pitch` bytes per row.
    pub const fn compute_offset(self, rect: Rect, pitch: u32, bytes_per_pixel: u32) -> u64 {
        match self {
            Self::RectangleStart => {
                (rect.y as u64) * (pitch as u64) + (rect.x as u64) * (bytes_per_pixel as u64)
            }
            Self::BackingStart => 0,
        }
    }
}

/// Geometry and host identity of an attached 2D resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceGeometry {
    pub resource_id: u32,
    pub format: GpuFormat,
    pub width: u32,
    pub height: u32,
    pub pitch: u32,
}

impl ResourceGeometry {
    /// Clamps a requested damage region `(x, y, width, height)` to this resource's
    /// bounds. Returns `None` if the resulting intersection is empty.
    pub fn clamp_rect(&self, x: u32, y: u32, width: u32, height: u32) -> Option<Rect> {
        let start_x = x.min(self.width);
        let start_y = y.min(self.height);
        let end_x = x.saturating_add(width).min(self.width);
        let end_y = y.saturating_add(height).min(self.height);
        let clamped_w = end_x.saturating_sub(start_x);
        let clamped_h = end_y.saturating_sub(start_y);
        if clamped_w == 0 || clamped_h == 0 {
            return None;
        }
        Some(Rect {
            x: start_x,
            y: start_y,
            width: clamped_w,
            height: clamped_h,
        })
    }

    /// Full-surface rectangle `(0, 0, width, height)`.
    pub const fn full_rect(&self) -> Rect {
        Rect::sized(self.width, self.height)
    }
}

/// Per-scanout hardware cursor state.
#[derive(Debug, Clone, Copy, Default)]
pub struct CursorState {
    pub scanout_id: u32,
    pub resource_id: u32,
    pub bo_paddr: usize,
    pub x: u32,
    pub y: u32,
    pub hot_x: u32,
    pub hot_y: u32,
}

impl CursorState {
    /// Returns the active cursor resource ID if it already backs `paddr`.
    pub const fn matching_resource(&self, paddr: usize) -> Option<u32> {
        if self.resource_id != 0 && self.bo_paddr == paddr {
            Some(self.resource_id)
        } else {
            None
        }
    }
}

/// Stateful 2D resource and cursor pipeline tracker for one `virtio-gpu` device.
pub struct Pipeline {
    next_resource_id: u32,
    transfer_offset: TransferOffset,
    fb_resources: BTreeMap<u32, ResourceGeometry>,
    cursors: [CursorState; MAX_SCANOUTS],
}

impl Pipeline {
    /// Creates a new pipeline configured with `transfer_offset`.
    pub fn new(transfer_offset: TransferOffset) -> Self {
        let cursors = core::array::from_fn(|scanout| CursorState {
            scanout_id: scanout as u32,
            ..CursorState::default()
        });
        Self {
            // Host resource ID 0 is reserved by the spec to mean "disabled / no resource".
            next_resource_id: 1,
            transfer_offset,
            fb_resources: BTreeMap::new(),
            cursors,
        }
    }

    /// Allocates a fresh non-zero host resource ID.
    pub fn alloc_resource_id(&mut self) -> Result<u32> {
        let id = self.next_resource_id;
        self.next_resource_id = self
            .next_resource_id
            .checked_add(1)
            .ok_or(Errno::ENOSPC)?;
        Ok(id)
    }

    /// Looks up the attached [`ResourceGeometry`] for framebuffer `fb_id`.
    pub fn fb_resource(&self, fb_id: u32) -> Option<ResourceGeometry> {
        self.fb_resources.get(&fb_id).copied()
    }

    /// Records a newly created and backed resource for framebuffer `fb_id`.
    pub fn register_fb_resource(&mut self, fb_id: u32, geometry: ResourceGeometry) {
        self.fb_resources.insert(fb_id, geometry);
    }

    /// Removes and returns the [`ResourceGeometry`] associated with `fb_id`, if any.
    pub fn take_fb_resource(&mut self, fb_id: u32) -> Option<ResourceGeometry> {
        self.fb_resources.remove(&fb_id)
    }

    /// Returns a reference to the cursor state for `scanout`.
    pub fn cursor(&self, scanout: usize) -> Option<&CursorState> {
        self.cursors.get(scanout)
    }

    /// Returns a mutable reference to the cursor state for `scanout`.
    pub fn cursor_mut(&mut self, scanout: usize) -> Option<&mut CursorState> {
        self.cursors.get_mut(scanout)
    }

    /// Computes the byte offset for a `TRANSFER_TO_HOST_2D` of `rect` on `geometry`.
    pub const fn transfer_offset_for(&self, geometry: &ResourceGeometry, rect: Rect) -> u64 {
        self.transfer_offset.compute_offset(
            rect,
            geometry.pitch,
            geometry.format.bytes_per_pixel(),
        )
    }
}

// ----------------------------------------------------------------------------
// Wire command constructors
// ----------------------------------------------------------------------------

/// Builds a `VIRTIO_GPU_CMD_RESOURCE_CREATE_2D` command after validating dimensions.
pub fn build_create_2d(
    resource_id: u32,
    format: GpuFormat,
    width: u32,
    height: u32,
) -> Result<ResourceCreate2d> {
    if resource_id == 0
        || width == 0
        || height == 0
        || width > MAX_DIMENSION
        || height > MAX_DIMENSION
    {
        crate::return_errno!(EINVAL, "invalid 2D resource dimensions");
    }
    Ok(ResourceCreate2d {
        hdr: CtrlHdr::new(CMD_RESOURCE_CREATE_2D),
        resource_id,
        format: format as u32,
        width,
        height,
    })
}

/// Builds a `VIRTIO_GPU_CMD_RESOURCE_ATTACH_BACKING` command for a single contiguous buffer.
pub fn build_attach_backing(resource_id: u32, paddr: u64, size: usize) -> Result<AttachBackingSingle> {
    let length = u32::try_from(size).map_err(|_| Errno::EINVAL)?;
    if resource_id == 0 || length == 0 {
        crate::return_errno!(EINVAL, "invalid backing buffer parameters");
    }
    Ok(AttachBackingSingle {
        hdr: CtrlHdr::new(CMD_RESOURCE_ATTACH_BACKING),
        resource_id,
        nr_entries: 1,
        entry: MemEntry {
            addr: paddr,
            length,
            pad: 0,
        },
    })
}

/// Builds a `VIRTIO_GPU_CMD_RESOURCE_DETACH_BACKING` command.
pub const fn build_detach_backing(resource_id: u32) -> DetachBacking {
    DetachBacking {
        hdr: CtrlHdr::new(CMD_RESOURCE_DETACH_BACKING),
        resource_id,
        pad: 0,
    }
}

/// Builds a `VIRTIO_GPU_CMD_RESOURCE_UNREF` command.
pub const fn build_unref(resource_id: u32) -> ResourceUnref {
    ResourceUnref {
        hdr: CtrlHdr::new(CMD_RESOURCE_UNREF),
        resource_id,
        pad: 0,
    }
}

/// Builds a `VIRTIO_GPU_CMD_SET_SCANOUT` command.
pub const fn build_set_scanout(scanout_id: u32, resource_id: u32, rect: Rect) -> SetScanout {
    SetScanout {
        hdr: CtrlHdr::new(CMD_SET_SCANOUT),
        rect,
        scanout_id,
        resource_id,
    }
}

/// Builds a `VIRTIO_GPU_CMD_TRANSFER_TO_HOST_2D` command.
pub const fn build_transfer_2d(resource_id: u32, rect: Rect, offset: u64) -> TransferToHost2d {
    TransferToHost2d {
        hdr: CtrlHdr::new(CMD_TRANSFER_TO_HOST_2D),
        rect,
        offset,
        resource_id,
        pad: 0,
    }
}

/// Builds a `VIRTIO_GPU_CMD_RESOURCE_FLUSH` command.
pub const fn build_flush(resource_id: u32, rect: Rect) -> ResourceFlush {
    ResourceFlush {
        hdr: CtrlHdr::new(CMD_RESOURCE_FLUSH),
        rect,
        resource_id,
        pad: 0,
    }
}

/// Builds a `VIRTIO_GPU_CMD_UPDATE_CURSOR` command for `state`.
pub const fn build_update_cursor(state: &CursorState) -> UpdateCursor {
    UpdateCursor {
        hdr: CtrlHdr::new(CMD_UPDATE_CURSOR),
        pos: CursorPos {
            scanout_id: state.scanout_id,
            x: state.x,
            y: state.y,
            pad: 0,
        },
        resource_id: state.resource_id,
        hot_x: state.hot_x,
        hot_y: state.hot_y,
        pad: 0,
    }
}

/// Builds a `VIRTIO_GPU_CMD_MOVE_CURSOR` command for `state`.
pub const fn build_move_cursor(state: &CursorState) -> UpdateCursor {
    UpdateCursor {
        hdr: CtrlHdr::new(CMD_MOVE_CURSOR),
        pos: CursorPos {
            scanout_id: state.scanout_id,
            x: state.x,
            y: state.y,
            pad: 0,
        },
        resource_id: state.resource_id,
        hot_x: state.hot_x,
        hot_y: state.hot_y,
        pad: 0,
    }
}

/// Validates that a cursor plane dimensions pair matches the `64x64` standard
/// required by `virtio-gpu`.
pub fn validate_cursor_dimensions(width: u32, height: u32, bo_size: usize) -> Result<()> {
    if width != CURSOR_SIZE || height != CURSOR_SIZE {
        crate::return_errno!(EINVAL, "virtio-gpu cursor must be 64x64");
    }
    let required_bytes = (CURSOR_SIZE as usize) * (CURSOR_SIZE as usize) * 4;
    if bo_size < required_bytes {
        crate::return_errno!(EINVAL, "cursor buffer smaller than 64x64x4 bytes");
    }
    Ok(())
}
