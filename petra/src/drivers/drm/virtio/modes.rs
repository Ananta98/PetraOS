// SPDX-License-Identifier: GPL-2.0

//! Display mode synthesis and EDID timing extraction for `virtio-gpu`.
//!
//! When `VIRTIO_GPU_F_EDID` is negotiated, the host provides a 128-byte (or
//! multi-block) EDID blob per scanout describing the host monitor's preferred
//! timing and refresh rate. This module validates the EDID, extracts detailed
//! timing descriptors and the preferred refresh rate, and populates each
//! virtual connector's mode list with properly timed [`drm::DrmModeModeInfo`]
//! entries alongside standard virtual fallback resolutions.

use alloc::format;
use alloc::vec::Vec;

use crate::uapi::drm;

/// Size in bytes of one EDID block.
pub const EDID_BLOCK_BYTES: usize = 128;

/// Standard 8-byte EDID 1.x header.
const EDID_HEADER: [u8; 8] = [0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00];

/// Offsets of the four 18-byte descriptor blocks inside the base EDID block.
const EDID_DESCRIPTOR_OFFSETS: [usize; 4] = [54, 72, 90, 108];

/// Offset of the extension block count in the base EDID block.
const EDID_EXTENSION_COUNT_OFFSET: usize = 126;

/// DisplayID extension block tag.
const DISPLAYID_EXT_TAG: u8 = 0x70;

/// DisplayID Type I detailed timing data block tag.
const DISPLAYID_TYPE_I_TAG: u8 = 0x03;

/// Default refresh rate in millihertz (60 Hz) when no EDID timing is available.
pub const DEFAULT_REFRESH_MHZ: u32 = 60_000;

/// Maximum plausible refresh rate in millihertz (1000 Hz).
const MAX_REFRESH_MHZ: u32 = 1_000_000;

/// Standard virtual display resolutions offered alongside the native scanout size.
pub const STANDARD_VIRTUAL_SIZES: [(u32, u32); 10] = [
    (3840, 2160),
    (2560, 1440),
    (1920, 1200),
    (1920, 1080),
    (1680, 1050),
    (1600, 900),
    (1280, 1024),
    (1280, 720),
    (1024, 768),
    (800, 600),
];

/// Builds the complete list of [`drm::DrmModeModeInfo`] entries for a scanout
/// whose host-reported preferred geometry is `width x height`.
pub fn build_scanout_modes(
    width: u32,
    height: u32,
    edid_bytes: Option<&[u8]>,
) -> Vec<drm::DrmModeModeInfo> {
    let mut modes = Vec::new();
    let mut refresh_mhz = DEFAULT_REFRESH_MHZ;

    if let Some(bytes) = edid_bytes {
        let edid_refresh = edid_preferred_refresh_mhz(bytes);
        if edid_refresh > 0 {
            refresh_mhz = edid_refresh;
        }
        for mode in edid_detailed_modes(bytes) {
            push_unique_mode(&mut modes, mode);
        }
    }

    let has_preferred_native = modes.iter().any(|mode| {
        mode.type_ & drm::DRM_MODE_TYPE_PREFERRED != 0
            && u32::from(mode.hdisplay) == width
            && u32::from(mode.vdisplay) == height
    });

    let native_mode = synthetic_mode(width, height, refresh_mhz, !has_preferred_native);
    push_unique_mode(&mut modes, native_mode);

    for (std_width, std_height) in STANDARD_VIRTUAL_SIZES {
        if std_width <= width && std_height <= height {
            let fallback = synthetic_mode(std_width, std_height, DEFAULT_REFRESH_MHZ, false);
            push_unique_mode(&mut modes, fallback);
        }
    }

    // Ensure the preferred mode appears at index 0 as expected by KMS callers.
    if let Some(pref_idx) = modes
        .iter()
        .position(|m| m.type_ & drm::DRM_MODE_TYPE_PREFERRED != 0)
        && pref_idx > 0
    {
        modes.swap(0, pref_idx);
    }

    modes
}

/// Synthesizes a [`drm::DrmModeModeInfo`] for `width x height` at `refresh_mhz`
/// using standard virtual blanking intervals.
pub fn synthetic_mode(
    width: u32,
    height: u32,
    refresh_mhz: u32,
    preferred: bool,
) -> drm::DrmModeModeInfo {
    let refresh_mhz = if refresh_mhz == 0 {
        DEFAULT_REFRESH_MHZ
    } else {
        refresh_mhz
    };
    let clamp_u16 = |value: u32| u16::try_from(value).unwrap_or(u16::MAX);

    let hdisplay = clamp_u16(width);
    let hsync_start = clamp_u16(width.saturating_add(16));
    let hsync_end = clamp_u16(width.saturating_add(48));
    let htotal = clamp_u16(width.saturating_add(160));

    let vdisplay = clamp_u16(height);
    let vsync_start = clamp_u16(height.saturating_add(3));
    let vsync_end = clamp_u16(height.saturating_add(9));
    let vtotal = clamp_u16(height.saturating_add(23));

    let total_pixels = u64::from(htotal) * u64::from(vtotal);
    let clock_khz =
        u32::try_from(total_pixels * u64::from(refresh_mhz) / 1_000_000).unwrap_or(0);
    let vrefresh = (refresh_mhz + 500) / 1_000;

    let mut type_ = drm::DRM_MODE_TYPE_DRIVER;
    if preferred {
        type_ |= drm::DRM_MODE_TYPE_PREFERRED;
    }

    let mut mode = drm::DrmModeModeInfo {
        clock: clock_khz,
        hdisplay,
        hsync_start,
        hsync_end,
        htotal,
        hskew: 0,
        vdisplay,
        vsync_start,
        vsync_end,
        vtotal,
        vscan: 0,
        vrefresh,
        flags: drm::DRM_MODE_FLAG_NHSYNC | drm::DRM_MODE_FLAG_PVSYNC,
        type_,
        name: [0u8; drm::DRM_DISPLAY_MODE_LEN],
    };
    fill_mode_name(&mut mode.name, width, height);
    mode
}

/// Extracts the preferred vertical refresh rate in millihertz from `edid_bytes`.
/// Returns `0` if `edid_bytes` is not a valid EDID 1.x base block or contains
/// no usable timing descriptor.
pub fn edid_preferred_refresh_mhz(edid_bytes: &[u8]) -> u32 {
    let Some(base) = valid_base_block(edid_bytes) else {
        return 0;
    };

    for offset in EDID_DESCRIPTOR_OFFSETS {
        if let Some(block) = base.get(offset..offset + 18)
            && (block[0] != 0 || block[1] != 0)
            && let Some(mode) = parse_detailed_timing(block, true)
        {
            return mode_refresh_mhz(&mode);
        }
    }

    let extensions = usize::from(base.get(EDID_EXTENSION_COUNT_OFFSET).copied().unwrap_or(0));
    for ext_index in 1..=extensions {
        let start = ext_index * EDID_BLOCK_BYTES;
        let Some(ext_block) = edid_bytes.get(start..start + EDID_BLOCK_BYTES) else {
            break;
        };
        if ext_block.first() == Some(&DISPLAYID_EXT_TAG)
            && block_checksum_valid(ext_block)
            && let Some(refresh) = parse_displayid_preferred_refresh(ext_block)
        {
            return refresh;
        }
    }

    0
}

/// Extracts all valid 18-byte detailed timing descriptors from the base EDID block.
pub fn edid_detailed_modes(edid_bytes: &[u8]) -> Vec<drm::DrmModeModeInfo> {
    let mut modes = Vec::new();
    let Some(base) = valid_base_block(edid_bytes) else {
        return modes;
    };

    let mut first = true;
    for offset in EDID_DESCRIPTOR_OFFSETS {
        if let Some(block) = base.get(offset..offset + 18)
            && let Some(mode) = parse_detailed_timing(block, first)
        {
            modes.push(mode);
            first = false;
        }
    }
    modes
}

fn valid_base_block(edid_bytes: &[u8]) -> Option<&[u8]> {
    let base = edid_bytes.get(..EDID_BLOCK_BYTES)?;
    if base.get(..EDID_HEADER.len()) != Some(&EDID_HEADER[..]) {
        return None;
    }
    if !block_checksum_valid(base) || base.get(18) != Some(&1) {
        return None;
    }
    Some(base)
}

fn block_checksum_valid(block: &[u8]) -> bool {
    block.iter().fold(0u8, |sum, &byte| sum.wrapping_add(byte)) == 0
}

/// Parses an 18-byte EDID detailed timing descriptor into a [`drm::DrmModeModeInfo`].
fn parse_detailed_timing(block: &[u8], preferred: bool) -> Option<drm::DrmModeModeInfo> {
    let byte = |idx: usize| block.get(idx).copied().map(u32::from);
    let clock_10khz = byte(0)? | (byte(1)? << 8);
    let flags_byte = byte(17)?;
    // Skip display descriptors (clock == 0) and interlaced timings (bit 7).
    if clock_10khz == 0 || flags_byte & 0x80 != 0 {
        return None;
    }

    let hactive = byte(2)? | ((byte(4)? >> 4) << 8);
    let hblank = byte(3)? | ((byte(4)? & 0x0F) << 8);
    let vactive = byte(5)? | ((byte(7)? >> 4) << 8);
    let vblank = byte(6)? | ((byte(7)? & 0x0F) << 8);
    if hactive == 0 || vactive == 0 || hblank == 0 || vblank == 0 {
        return None;
    }

    let hsync_offset = byte(8)? | ((byte(11)? >> 6) << 8);
    let hsync_pulse = byte(9)? | (((byte(11)? >> 4) & 0x03) << 8);
    let vsync_offset = (byte(10)? >> 4) | (((byte(11)? >> 2) & 0x03) << 4);
    let vsync_pulse = (byte(10)? & 0x0F) | ((byte(11)? & 0x03) << 4);

    let hsync_start = hactive.checked_add(hsync_offset)?;
    let hsync_end = hsync_start.checked_add(hsync_pulse)?;
    let htotal = hactive.checked_add(hblank)?.max(hsync_end + 1);

    let vsync_start = vactive.checked_add(vsync_offset)?;
    let vsync_end = vsync_start.checked_add(vsync_pulse)?;
    let vtotal = vactive.checked_add(vblank)?.max(vsync_end + 1);

    let clock_khz = clock_10khz.checked_mul(10)?;
    let total_pixels = u64::from(htotal) * u64::from(vtotal);
    let refresh_mhz =
        u32::try_from((u64::from(clock_khz) * 1_000_000 + total_pixels / 2) / total_pixels)
            .ok()
            .filter(|&rate| (1..=MAX_REFRESH_MHZ).contains(&rate))?;

    let mut flags = 0;
    if flags_byte & 0x02 != 0 {
        flags |= drm::DRM_MODE_FLAG_PHSYNC;
    } else {
        flags |= drm::DRM_MODE_FLAG_NHSYNC;
    }
    if flags_byte & 0x04 != 0 {
        flags |= drm::DRM_MODE_FLAG_PVSYNC;
    } else {
        flags |= drm::DRM_MODE_FLAG_NVSYNC;
    }

    let mut type_ = drm::DRM_MODE_TYPE_DRIVER;
    if preferred {
        type_ |= drm::DRM_MODE_TYPE_PREFERRED;
    }

    let mut mode = drm::DrmModeModeInfo {
        clock: clock_khz,
        hdisplay: u16::try_from(hactive).ok()?,
        hsync_start: u16::try_from(hsync_start).ok()?,
        hsync_end: u16::try_from(hsync_end).ok()?,
        htotal: u16::try_from(htotal).ok()?,
        hskew: 0,
        vdisplay: u16::try_from(vactive).ok()?,
        vsync_start: u16::try_from(vsync_start).ok()?,
        vsync_end: u16::try_from(vsync_end).ok()?,
        vtotal: u16::try_from(vtotal).ok()?,
        vscan: 0,
        vrefresh: (refresh_mhz + 500) / 1_000,
        flags,
        type_,
        name: [0u8; drm::DRM_DISPLAY_MODE_LEN],
    };
    fill_mode_name(&mut mode.name, hactive, vactive);
    Some(mode)
}

fn parse_displayid_preferred_refresh(block: &[u8]) -> Option<u32> {
    let section_len = usize::from(*block.get(2)?);
    let end = (5 + section_len).min(EDID_BLOCK_BYTES - 1);
    let mut offset = 5;
    while offset + 3 <= end {
        let tag = *block.get(offset)?;
        let len = usize::from(*block.get(offset + 2)?);
        if offset + 3 + len > end {
            return None;
        }
        let payload = block.get(offset + 3..offset + 3 + len)?;
        if tag == DISPLAYID_TYPE_I_TAG && len >= 20 {
            let byte = |idx: usize| payload.get(idx).copied().map(u32::from);
            if byte(3)? & 0x10 != 0 {
                return None;
            }
            let clock_10khz = (byte(0)? | (byte(1)? << 8) | (byte(2)? << 16)) + 1;
            let hactive = (byte(4)? | (byte(5)? << 8)) + 1;
            let hblank = (byte(6)? | (byte(7)? << 8)) + 1;
            let vactive = (byte(12)? | (byte(13)? << 8)) + 1;
            let vblank = (byte(14)? | (byte(15)? << 8)) + 1;
            let total = u64::from(hactive.checked_add(hblank)?)
                * u64::from(vactive.checked_add(vblank)?);
            if total == 0 {
                return None;
            }
            let refresh_mhz = (u64::from(clock_10khz) * 10_000_000 + total / 2) / total;
            return u32::try_from(refresh_mhz)
                .ok()
                .filter(|&rate| (1..=MAX_REFRESH_MHZ).contains(&rate));
        }
        offset += 3 + len;
    }
    None
}

fn mode_refresh_mhz(mode: &drm::DrmModeModeInfo) -> u32 {
    let total = u64::from(mode.htotal) * u64::from(mode.vtotal);
    if total == 0 || mode.clock == 0 {
        return mode.vrefresh.saturating_mul(1_000);
    }
    u32::try_from((u64::from(mode.clock) * 1_000_000 + total / 2) / total)
        .unwrap_or(mode.vrefresh.saturating_mul(1_000))
}

fn fill_mode_name(name: &mut [u8; drm::DRM_DISPLAY_MODE_LEN], width: u32, height: u32) {
    let label = format!("{width}x{height}");
    let used = label.len().min(drm::DRM_DISPLAY_MODE_LEN - 1);
    name[..used].copy_from_slice(&label.as_bytes()[..used]);
}

fn push_unique_mode(modes: &mut Vec<drm::DrmModeModeInfo>, candidate: drm::DrmModeModeInfo) {
    if let Some(existing) = modes.iter_mut().find(|m| {
        m.hdisplay == candidate.hdisplay
            && m.vdisplay == candidate.vdisplay
            && m.vrefresh == candidate.vrefresh
    }) {
        if candidate.type_ & drm::DRM_MODE_TYPE_PREFERRED != 0 {
            existing.type_ |= drm::DRM_MODE_TYPE_PREFERRED;
        }
        return;
    }
    modes.push(candidate);
}
