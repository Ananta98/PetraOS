// SPDX-License-Identifier: GPL-2.0

//! Intel Display Engine: pipe discovery, GMBUS EDID probing, primary plane
//! scanout programming, and hardware cursor control.

use alloc::vec;
use alloc::vec::Vec;
use ostd::io::IoMem;
use ostd::mm::VmIo;

use super::regs::{
    CUR_MODE_64_ARGB_AX, CUR_MODE_DISABLE, CUR_MODE_MASK, CUR_PIPE_SELECT_MASK,
    CUR_PIPE_SELECT_SHIFT, DSPCNTR_FORMAT_BGRA8888, DSPCNTR_FORMAT_BGRX8888, DSPCNTR_FORMAT_MASK,
    DSPCNTR_TILED, EDID_I2C_ADDR, GMBUS_BYTE_COUNT_SHIFT, GMBUS_CYCLE_INDEX, GMBUS_CYCLE_WAIT,
    GMBUS_HW_RDY, GMBUS_HW_WAIT_PHASE, GMBUS_PINS_PROBE_ORDER, GMBUS_SATOER, GMBUS_SLAVE_ADDR_SHIFT,
    GMBUS_SLAVE_READ, GMBUS_SW_CLR_INT, GMBUS_SW_RDY, GMBUS0, GMBUS1, GMBUS2, GMBUS3, GMBUS4,
    IntelGen, PIPECONF_ACTIVE, PIPECONF_ENABLE, PLANE_CTL_ALPHA_MASK,
    PLANE_CTL_ALPHA_PREMULTIPLIED, PLANE_CTL_FORMAT_MASK, PLANE_CTL_FORMAT_XRGB8888,
    PLANE_CTL_TILED_MASK, PLANE_ENABLE, Pipe, SCANOUT_STRIDE_ALIGN, SURFACE_ADDR_MASK,
    encode_cursor_pos,
};
use crate::drivers::drm::{
    EDID_BLOCK_BYTES, Framebuffer, GemObject, KmsPipeline, build_scanout_modes, get_framebuffer,
};
use crate::uapi::drm;
use crate::uapi::errno::Result;

/// Discovered hardware state for one active Intel display pipe.
#[derive(Debug, Clone)]
pub struct IntelPipeConfig {
    pub pipe: Pipe,
    pub width: u32,
    pub height: u32,
    pub initial_surf: u32,
    pub modes: Vec<drm::DrmModeModeInfo>,
}

/// Runtime cursor state for a display pipe.
#[derive(Debug, Clone, Copy, Default)]
pub struct IntelCursorState {
    pub obj_id: u32,
    pub gtt_offset: u32,
    pub hot_x: i32,
    pub hot_y: i32,
    pub x: i32,
    pub y: i32,
}

/// Discovers active display pipes from `mmio` (programmed by UEFI GOP / VBIOS)
/// and probes GMBUS for monitor EDID timings. Falls back to `PipeA` with the
/// boot framebuffer resolution if no hardware pipe reports active.
pub fn discover_pipes(generation: IntelGen, mmio: Option<&IoMem>) -> Vec<IntelPipeConfig> {
    let mut pipes = Vec::new();
    let edid = mmio.and_then(probe_edid_gmbus);

    if let Some(io) = mmio {
        for pipe in Pipe::ALL {
            if let Some(cfg) = probe_single_pipe(generation, io, pipe, edid.as_deref()) {
                pipes.push(cfg);
            }
        }
    }

    if pipes.is_empty() {
        let (width, height) = get_framebuffer()
            .map(|fb| (fb.width() as u32, fb.height() as u32))
            .unwrap_or((1024, 768));
        let modes = build_scanout_modes(width, height, edid.as_deref());
        pipes.push(IntelPipeConfig {
            pipe: Pipe::PipeA,
            width,
            height,
            initial_surf: 0,
            modes,
        });
    }

    pipes
}

fn probe_single_pipe(
    _generation: IntelGen,
    mmio: &IoMem,
    pipe: Pipe,
    edid: Option<&[u8]>,
) -> Option<IntelPipeConfig> {
    let pipeconf = mmio.read_val::<u32>(pipe.reg_pipeconf()).ok()?;
    if pipeconf == 0xFFFF_FFFF || (pipeconf & (PIPECONF_ENABLE | PIPECONF_ACTIVE) == 0) {
        return None;
    }

    let srcsz = mmio.read_val::<u32>(pipe.reg_pipe_srcsz()).unwrap_or(0);
    let mut width = ((srcsz >> 16) & 0x1FFF) + 1;
    let mut height = (srcsz & 0x1FFF) + 1;

    if width <= 64 || height <= 64 || width > 8192 || height > 8192 {
        let htotal = mmio.read_val::<u32>(pipe.reg_htotal()).unwrap_or(0);
        let vtotal = mmio.read_val::<u32>(pipe.reg_vtotal()).unwrap_or(0);
        width = (htotal & 0x1FFF) + 1;
        height = (vtotal & 0x1FFF) + 1;
    }

    if width <= 64 || height <= 64 || width > 8192 || height > 8192 {
        return None;
    }

    let initial_surf = mmio
        .read_val::<u32>(pipe.reg_dspsurf())
        .unwrap_or(0)
        & SURFACE_ADDR_MASK;
    let modes = build_scanout_modes(width, height, edid);

    Some(IntelPipeConfig {
        pipe,
        width,
        height,
        initial_surf,
        modes,
    })
}

/// Builds a [`KmsPipeline`] description for an [`IntelPipeConfig`].
pub fn kms_pipeline_for(cfg: &IntelPipeConfig) -> KmsPipeline {
    KmsPipeline {
        modes: cfg.modes.clone(),
        connector_type: drm::DRM_MODE_CONNECTOR_HDMIA,
        encoder_type: drm::DRM_MODE_ENCODER_TMDS,
        mm_width: 340,
        mm_height: 190,
    }
}

/// Programs the primary plane of `pipe` to scan out `fb` from `gtt_offset`.
pub fn program_primary_plane(
    generation: IntelGen,
    mmio: &IoMem,
    pipe: Pipe,
    fb: &Framebuffer,
    mode: &drm::DrmModeModeInfo,
    gtt_offset: u32,
) -> Result<()> {
    if fb.pitch % SCANOUT_STRIDE_ALIGN != 0 {
        crate::return_errno!(EINVAL, "Intel scanout pitch must be 64-byte aligned");
    }

    let width = u32::from(mode.hdisplay).min(fb.width);
    let height = u32::from(mode.vdisplay).min(fb.height);
    if width == 0 || height == 0 {
        crate::return_errno!(EINVAL, "invalid scanout dimensions");
    }

    // Ensure the pipe source size matches the active mode.
    let srcsz = ((width - 1) << 16) | (height - 1);
    let _ = mmio.write_val::<u32>(pipe.reg_pipe_srcsz(), &srcsz);

    let pipeconf = mmio.read_val::<u32>(pipe.reg_pipeconf()).unwrap_or(0);
    if pipeconf != 0xFFFF_FFFF && (pipeconf & PIPECONF_ENABLE == 0) {
        let enabled = pipeconf | PIPECONF_ENABLE;
        let _ = mmio.write_val::<u32>(pipe.reg_pipeconf(), &enabled);
    }

    let surf_addr = gtt_offset & SURFACE_ADDR_MASK;

    if generation.uses_universal_planes() {
        let mut ctl = mmio.read_val::<u32>(pipe.reg_plane_ctl()).unwrap_or(0);
        if ctl == 0xFFFF_FFFF {
            ctl = 0;
        }
        ctl &= !(PLANE_CTL_FORMAT_MASK | PLANE_CTL_TILED_MASK | PLANE_CTL_ALPHA_MASK);
        ctl |= PLANE_ENABLE | PLANE_CTL_FORMAT_XRGB8888;
        if fb.format == drm::DRM_FORMAT_ARGB8888 {
            ctl |= PLANE_CTL_ALPHA_PREMULTIPLIED;
        }

        // On Gen9+, PLANE_STRIDE is programmed in units of 64-byte chunks for linear surfaces.
        let stride_chunks = fb.pitch / SCANOUT_STRIDE_ALIGN;
        let plane_size = ((height - 1) << 16) | (width - 1);

        let _ = mmio.write_val::<u32>(pipe.reg_plane_ctl(), &ctl);
        let _ = mmio.write_val::<u32>(pipe.reg_plane_stride(), &stride_chunks);
        let _ = mmio.write_val::<u32>(pipe.reg_plane_pos(), &0u32);
        let _ = mmio.write_val::<u32>(pipe.reg_plane_size(), &plane_size);
        let _ = mmio.write_val::<u32>(pipe.reg_plane_offset(), &0u32);
        // Writing PLANE_SURF arms the double-buffered register update.
        let _ = mmio.write_val::<u32>(pipe.reg_plane_surf(), &surf_addr);
        let _ = mmio.read_val::<u32>(pipe.reg_plane_surf());
    } else {
        let mut cntr = mmio.read_val::<u32>(pipe.reg_dspcntr()).unwrap_or(0);
        if cntr == 0xFFFF_FFFF {
            cntr = 0;
        }
        cntr &= !(DSPCNTR_FORMAT_MASK | DSPCNTR_TILED);
        cntr |= PLANE_ENABLE
            | if fb.format == drm::DRM_FORMAT_ARGB8888 {
                DSPCNTR_FORMAT_BGRA8888
            } else {
                DSPCNTR_FORMAT_BGRX8888
            };

        let _ = mmio.write_val::<u32>(pipe.reg_dspcntr(), &cntr);
        let _ = mmio.write_val::<u32>(pipe.reg_dspstride(), &fb.pitch);
        let _ = mmio.write_val::<u32>(pipe.reg_dsplinoff(), &0u32);
        // Writing DSPSURF arms the double-buffered register update.
        let _ = mmio.write_val::<u32>(pipe.reg_dspsurf(), &surf_addr);
        let _ = mmio.read_val::<u32>(pipe.reg_dspsurf());
    }

    Ok(())
}

/// Disables the primary plane on `pipe`.
pub fn disable_primary_plane(generation: IntelGen, mmio: &IoMem, pipe: Pipe) {
    if generation.uses_universal_planes() {
        let ctl = mmio
            .read_val::<u32>(pipe.reg_plane_ctl())
            .unwrap_or(0)
            & !PLANE_ENABLE;
        let _ = mmio.write_val::<u32>(pipe.reg_plane_ctl(), &ctl);
        let _ = mmio.write_val::<u32>(pipe.reg_plane_surf(), &0u32);
        let _ = mmio.read_val::<u32>(pipe.reg_plane_surf());
    } else {
        let cntr = mmio
            .read_val::<u32>(pipe.reg_dspcntr())
            .unwrap_or(0)
            & !PLANE_ENABLE;
        let _ = mmio.write_val::<u32>(pipe.reg_dspcntr(), &cntr);
        let _ = mmio.write_val::<u32>(pipe.reg_dspsurf(), &0u32);
        let _ = mmio.read_val::<u32>(pipe.reg_dspsurf());
    }
}

/// Programs or hides the hardware `64x64` ARGB cursor plane on `pipe`.
pub fn program_cursor_plane(mmio: &IoMem, pipe: Pipe, cursor: &IntelCursorState) {
    if cursor.gtt_offset == 0 {
        let cntr = mmio
            .read_val::<u32>(pipe.reg_curcntr())
            .unwrap_or(0)
            & !CUR_MODE_MASK;
        let _ = mmio.write_val::<u32>(pipe.reg_curcntr(), &(cntr | CUR_MODE_DISABLE));
        let _ = mmio.write_val::<u32>(pipe.reg_curbase(), &0u32);
        let _ = mmio.read_val::<u32>(pipe.reg_curbase());
        return;
    }

    let draw_x = cursor.x.saturating_sub(cursor.hot_x);
    let draw_y = cursor.y.saturating_sub(cursor.hot_y);
    let pos_reg = encode_cursor_pos(draw_x, draw_y);

    let mut cntr = mmio.read_val::<u32>(pipe.reg_curcntr()).unwrap_or(0);
    if cntr == 0xFFFF_FFFF {
        cntr = 0;
    }
    cntr &= !(CUR_MODE_MASK | CUR_PIPE_SELECT_MASK);
    cntr |= CUR_MODE_64_ARGB_AX | ((pipe as u32) << CUR_PIPE_SELECT_SHIFT);

    let base_addr = cursor.gtt_offset & SURFACE_ADDR_MASK;
    let _ = mmio.write_val::<u32>(pipe.reg_curcntr(), &cntr);
    let _ = mmio.write_val::<u32>(pipe.reg_curpos(), &pos_reg);
    // Writing CURBASE arms the hardware cursor update.
    let _ = mmio.write_val::<u32>(pipe.reg_curbase(), &base_addr);
    let _ = mmio.read_val::<u32>(pipe.reg_curbase());
}

/// Updates only the position of an active hardware cursor on `pipe`.
pub fn move_cursor_plane(mmio: &IoMem, pipe: Pipe, cursor: &IntelCursorState) {
    if cursor.gtt_offset == 0 {
        return;
    }
    let draw_x = cursor.x.saturating_sub(cursor.hot_x);
    let draw_y = cursor.y.saturating_sub(cursor.hot_y);
    let pos_reg = encode_cursor_pos(draw_x, draw_y);
    let base_addr = cursor.gtt_offset & SURFACE_ADDR_MASK;

    let _ = mmio.write_val::<u32>(pipe.reg_curpos(), &pos_reg);
    let _ = mmio.write_val::<u32>(pipe.reg_curbase(), &base_addr);
    let _ = mmio.read_val::<u32>(pipe.reg_curbase());
}

/// Copies a damage rectangle `(x, y, width, height)` from `fb.object` into
/// the active linear scanout aperture (`aperture`) or the boot framebuffer so
/// updates are visible even when scanning out from stolen memory.
pub fn sync_damage_to_scanout(
    fb: &Framebuffer,
    aperture: Option<&IoMem>,
    dst_surf_offset: usize,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
) -> Result<()> {
    let start_x = x.min(fb.width);
    let start_y = y.min(fb.height);
    let end_x = x.saturating_add(width).min(fb.width);
    let end_y = y.saturating_add(height).min(fb.height);
    if start_x >= end_x || start_y >= end_y {
        return Ok(());
    }

    let bpp_bytes = (fb.bpp() / 8).max(4) as usize;
    let copy_row_bytes = (end_x - start_x) as usize * bpp_bytes;
    let src_pitch = fb.pitch as usize;
    let mut row_buf = vec![0u8; copy_row_bytes];

    if let Some(ap) = aperture {
        for row in start_y..end_y {
            let byte_off = (row as usize) * src_pitch + (start_x as usize) * bpp_bytes;
            fb.object.read_at(byte_off, &mut row_buf)?;
            let dst_off = dst_surf_offset.saturating_add(byte_off);
            let _ = ap.write_bytes(dst_off, &row_buf);
        }
        return Ok(());
    }

    if let Some(boot_fb) = get_framebuffer() {
        let dst_pitch = boot_fb.line_size();
        let max_rows = end_y.min(boot_fb.height() as u32);
        let max_cols_bytes = ((end_x.min(boot_fb.width() as u32) - start_x) as usize) * bpp_bytes;
        if max_cols_bytes == 0 {
            return Ok(());
        }
        for row in start_y..max_rows {
            let src_off = (row as usize) * src_pitch + (start_x as usize) * bpp_bytes;
            let dst_off = (row as usize) * dst_pitch + (start_x as usize) * bpp_bytes;
            fb.object.read_at(src_off, &mut row_buf[..max_cols_bytes])?;
            let _ = boot_fb.write_bytes_at(dst_off, &row_buf[..max_cols_bytes]);
        }
    }

    Ok(())
}

/// Copies a 64x64 cursor [`GemObject`] into the GGTT aperture at `dst_offset`
/// when `BAR2` is mapped.
pub fn sync_cursor_to_aperture(bo: &GemObject, aperture: Option<&IoMem>, dst_offset: usize) {
    let Some(ap) = aperture else {
        return;
    };
    let cursor_bytes = 64 * 64 * 4;
    if bo.size() < cursor_bytes {
        return;
    }
    let mut buf = vec![0u8; cursor_bytes];
    if bo.read_at(0, &mut buf).is_ok() {
        let _ = ap.write_bytes(dst_offset, &buf);
    }
}

// ----------------------------------------------------------------------------
// GMBUS EDID Probing
// ----------------------------------------------------------------------------

fn probe_edid_gmbus(mmio: &IoMem) -> Option<Vec<u8>> {
    for pin in GMBUS_PINS_PROBE_ORDER {
        if let Some(edid) = read_edid_on_pin(mmio, pin) {
            return Some(edid);
        }
    }
    None
}

fn read_edid_on_pin(mmio: &IoMem, pin: u32) -> Option<Vec<u8>> {
    // Mask interrupts and clear any sticky software error on GMBUS.
    let _ = mmio.write_val::<u32>(GMBUS4, &0u32);
    let _ = mmio.write_val::<u32>(GMBUS1, &GMBUS_SW_CLR_INT);
    let _ = mmio.write_val::<u32>(GMBUS1, &0u32);
    let _ = mmio.write_val::<u32>(GMBUS0, &pin);

    // Request 128-byte EDID base block from DDC slave 0x50 at index 0.
    let cmd = GMBUS_SW_RDY
        | GMBUS_CYCLE_WAIT
        | GMBUS_CYCLE_INDEX
        | ((EDID_BLOCK_BYTES as u32) << GMBUS_BYTE_COUNT_SHIFT)
        | (EDID_I2C_ADDR << GMBUS_SLAVE_ADDR_SHIFT)
        | GMBUS_SLAVE_READ;
    mmio.write_val::<u32>(GMBUS1, &cmd).ok()?;

    let mut edid = vec![0u8; EDID_BLOCK_BYTES];
    for chunk in edid.chunks_exact_mut(4) {
        if !wait_gmbus_hw_ready(mmio) {
            reset_gmbus(mmio);
            return None;
        }
        let word = mmio.read_val::<u32>(GMBUS3).ok()?;
        chunk.copy_from_slice(&word.to_le_bytes());
    }

    let _ = wait_gmbus_wait_phase(mmio);
    let stop_cmd = GMBUS_SW_RDY | (4 << 25); // GMBUS_CYCLE_STOP
    let _ = mmio.write_val::<u32>(GMBUS1, &stop_cmd);
    let _ = mmio.write_val::<u32>(GMBUS0, &0u32);

    // Check for valid EDID 1.x header before returning.
    if edid.get(..8) == Some(&[0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00]) {
        Some(edid)
    } else {
        None
    }
}

fn wait_gmbus_hw_ready(mmio: &IoMem) -> bool {
    for _ in 0..2_000 {
        let Ok(status) = mmio.read_val::<u32>(GMBUS2) else {
            return false;
        };
        if status == 0xFFFF_FFFF || (status & GMBUS_SATOER != 0) {
            return false;
        }
        if status & GMBUS_HW_RDY != 0 {
            return true;
        }
        core::hint::spin_loop();
    }
    false
}

fn wait_gmbus_wait_phase(mmio: &IoMem) -> bool {
    for _ in 0..1_000 {
        let Ok(status) = mmio.read_val::<u32>(GMBUS2) else {
            return false;
        };
        if status & GMBUS_SATOER != 0 {
            return false;
        }
        if status & GMBUS_HW_WAIT_PHASE != 0 {
            return true;
        }
        core::hint::spin_loop();
    }
    false
}

fn reset_gmbus(mmio: &IoMem) {
    let _ = mmio.write_val::<u32>(GMBUS1, &GMBUS_SW_CLR_INT);
    let _ = mmio.write_val::<u32>(GMBUS1, &0u32);
    let _ = mmio.write_val::<u32>(GMBUS0, &0u32);
}
