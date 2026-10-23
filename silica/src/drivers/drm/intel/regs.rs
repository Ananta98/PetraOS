// SPDX-License-Identifier: GPL-2.0

//! Intel Integrated Graphics (`i915`) hardware register definitions and
//! generation classification.
//!
//! Covers MMIO register offsets and bitfields for:
//! - PCI configuration and Graphics Translation Table (`GGTT`) BAR layout
//! - Display Pipe (`PIPEA`, `PIPEB`, `PIPEC`) timing and configuration
//! - Pre-Gen9 Primary Plane (`DSPCNTR`) and Gen9+ Universal Plane (`PLANE_CTL`)
//! - Hardware Cursor Plane (`CURCNTR`, `CURBASE`, `CURPOS`)
//! - GMBUS / DDC I2C controller for monitor EDID probing

/// Intel PCI Vendor ID.
pub const PCI_VENDOR_ID_INTEL: u16 = 0x8086;

/// PCIDisplay class (`0x03`), VGA-compatible subclass (`0x00`), and generic display (`0x80`).
pub const PCI_CLASS_DISPLAY: u8 = 0x03;
pub const PCI_SUBCLASS_DISPLAY_VGA: u8 = 0x00;
pub const PCI_SUBCLASS_DISPLAY_OTHER: u8 = 0x80;

/// PCI Config register: Graphics Mode Control (`GMCH_CTRL`, 16-bit).
pub const PCI_GMCH_CTRL: u8 = 0x50;
/// PCI Config register: Base of Data Stolen Memory (`BDSM`, 32-bit).
pub const PCI_BDSM: u8 = 0x5C;

/// Surface base addresses (`DSPSURF`, `PLANE_SURF`, `CURBASE`) must be 64 KiB aligned.
pub const GGTT_SURFACE_ALIGN: usize = 64 * 1024;
/// Linear scanout stride (`DSPSTRIDE`, `PLANE_STRIDE`) must be a multiple of 64 bytes.
pub const SCANOUT_STRIDE_ALIGN: u32 = 64;
/// Standard Intel hardware cursor size (`64x64` ARGB8888).
pub const CURSOR_SIZE: u32 = 64;

// ----------------------------------------------------------------------------
// Global Graphics Translation Table (GGTT) PTE flags
// ----------------------------------------------------------------------------

/// Page table entry present/valid bit.
pub const GGTT_PTE_VALID: u64 = 1 << 0;
/// Gen8+ Write-Back / LLC cacheable bits in GGTT PTE.
pub const GGTT_PTE_CACHE_LLC: u64 = (1 << 1) | (1 << 2);

/// Graphics Flush Control register in MMIO space (`GFX_FLSH_CNTL_GEN6`).
pub const REG_GFX_FLSH_CNTL: usize = 0x101008;
pub const GFX_FLSH_CNTL_EN: u32 = 1 << 0;

// ----------------------------------------------------------------------------
// Intel GPU Hardware Generation
// ----------------------------------------------------------------------------

/// Broad Intel display/graphics architecture generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum IntelGen {
    /// Gen7: Ivy Bridge, Haswell (`0x0152..0x0D26`).
    Gen7,
    /// Gen8: Broadwell, Cherryview (`0x1602..0x22B3`).
    Gen8,
    /// Gen9: Skylake, Kaby Lake, Coffee Lake, Comet Lake (`0x1902..0x9BC8`).
    Gen9,
    /// Gen11: Ice Lake, Elkhart Lake, Jasper Lake (`0x8A50..0x4E71`).
    Gen11,
    /// Gen12+: Tiger Lake, Alder Lake, Raptor Lake, Meteor Lake, Xe (`0x4680..`, `0x9A40..`).
    Gen12,
}

impl IntelGen {
    /// Classifies an Intel GPU PCI device ID into its display architecture generation.
    pub const fn from_device_id(device_id: u16) -> Self {
        let prefix = device_id & 0xFF00;
        match prefix {
            0x0100 | 0x0400 | 0x0A00 | 0x0C00 | 0x0D00 => Self::Gen7,
            0x1600 | 0x2200 => Self::Gen8,
            0x1900 | 0x3100 | 0x3E00 | 0x5900 | 0x5A00 | 0x8700 | 0x9B00 => Self::Gen9,
            0x4500 | 0x4E00 | 0x8A00 => Self::Gen11,
            0x4600 | 0x4C00 | 0x5600 | 0x7D00 | 0x9A00 | 0xA700 => Self::Gen12,
            _ => Self::Gen9,
        }
    }

    /// Whether this generation uses 64-bit (8-byte) GGTT PTEs (Gen8 and newer).
    pub const fn uses_64bit_gtt_pte(self) -> bool {
        matches!(
            self,
            Self::Gen8 | Self::Gen9 | Self::Gen11 | Self::Gen12
        )
    }

    /// Whether this generation uses Skylake-style universal plane registers (`PLANE_*`).
    pub const fn uses_universal_planes(self) -> bool {
        matches!(self, Self::Gen9 | Self::Gen11 | Self::Gen12)
    }

    /// Human-readable generation label.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Gen7 => "Gen7 (IvyBridge/Haswell)",
            Self::Gen8 => "Gen8 (Broadwell)",
            Self::Gen9 => "Gen9 (Skylake/CoffeeLake)",
            Self::Gen11 => "Gen11 (IceLake)",
            Self::Gen12 => "Gen12+ (TigerLake/AlderLake/Xe)",
        }
    }
}

// ----------------------------------------------------------------------------
// Display Pipes (`PIPE_A`, `PIPE_B`, `PIPE_C`)
// ----------------------------------------------------------------------------

/// Display pipes available on Intel display engines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
pub enum Pipe {
    PipeA = 0,
    PipeB = 1,
    PipeC = 2,
}

impl Pipe {
    pub const ALL: [Pipe; 3] = [Self::PipeA, Self::PipeB, Self::PipeC];

    /// Base offset for pipe timing registers (`0x60000 + pipe * 0x1000`).
    pub const fn timing_base(self) -> usize {
        0x60000 + (self as usize) * 0x1000
    }

    /// Base offset for pipe configuration and Plane/Cursor registers (`0x70000 + pipe * 0x1000`).
    pub const fn display_base(self) -> usize {
        0x70000 + (self as usize) * 0x1000
    }

    /// `HTOTAL`: `[31:16]` `htotal - 1`, `[15:0]` `hdisplay - 1`.
    pub const fn reg_htotal(self) -> usize {
        self.timing_base()
    }

    /// `HBLANK`: `[31:16]` `hblank_end - 1`, `[15:0]` `hblank_start - 1`.
    pub const fn reg_hblank(self) -> usize {
        self.timing_base() + 0x04
    }

    /// `HSYNC`: `[31:16]` `hsync_end - 1`, `[15:0]` `hsync_start - 1`.
    pub const fn reg_hsync(self) -> usize {
        self.timing_base() + 0x08
    }

    /// `VTOTAL`: `[31:16]` `vtotal - 1`, `[15:0]` `vdisplay - 1`.
    pub const fn reg_vtotal(self) -> usize {
        self.timing_base() + 0x0C
    }

    /// `VBLANK`: `[31:16]` `vblank_end - 1`, `[15:0]` `vblank_start - 1`.
    pub const fn reg_vblank(self) -> usize {
        self.timing_base() + 0x10
    }

    /// `VSYNC`: `[31:16]` `vsync_end - 1`, `[15:0]` `vsync_start - 1`.
    pub const fn reg_vsync(self) -> usize {
        self.timing_base() + 0x14
    }

    /// `PIPE_SRCSZ`: `[31:16]` `(width - 1)`, `[15:0]` `(height - 1)`.
    pub const fn reg_pipe_srcsz(self) -> usize {
        self.timing_base() + 0x1C
    }

    /// `PIPECONF` / `TRANS_CONF`: bit 31 enables the pipe/transcoder.
    pub const fn reg_pipeconf(self) -> usize {
        self.display_base() + 0x008
    }

    // ------------------------------------------------------------------------
    // Pre-Gen9 Primary Plane (`DSPCNTR`, `DSPSTRIDE`, `DSPSURF`)
    // ------------------------------------------------------------------------

    /// `DSPCNTR` (Primary Plane Control on Gen7/Gen8).
    pub const fn reg_dspcntr(self) -> usize {
        self.display_base() + 0x180
    }

    /// `DSPLINOFF` (Linear Offset on Gen7/Gen8).
    pub const fn reg_dsplinoff(self) -> usize {
        self.display_base() + 0x184
    }

    /// `DSPSTRIDE` (Primary Plane Stride in bytes on Gen7/Gen8).
    pub const fn reg_dspstride(self) -> usize {
        self.display_base() + 0x188
    }

    /// `DSPSURF` (Primary Plane GGTT Surface Base Address on Gen7/Gen8; write arms update).
    pub const fn reg_dspsurf(self) -> usize {
        self.display_base() + 0x19C
    }

    // ------------------------------------------------------------------------
    // Gen9+ Skylake Universal Plane 1 (`PLANE_CTL_1_*`, `PLANE_SURF_1_*`)
    // ------------------------------------------------------------------------

    /// `PLANE_CTL_1` (Universal Plane 1 Control on Gen9+).
    pub const fn reg_plane_ctl(self) -> usize {
        self.display_base() + 0x180
    }

    /// `PLANE_STRIDE_1` (Universal Plane 1 Stride in 64-byte chunks on Gen9+).
    pub const fn reg_plane_stride(self) -> usize {
        self.display_base() + 0x188
    }

    /// `PLANE_POS_1` (Universal Plane 1 Position `(y << 16) | x` on Gen9+).
    pub const fn reg_plane_pos(self) -> usize {
        self.display_base() + 0x18C
    }

    /// `PLANE_SIZE_1` (Universal Plane 1 Size `((height - 1) << 16) | (width - 1)` on Gen9+).
    pub const fn reg_plane_size(self) -> usize {
        self.display_base() + 0x190
    }

    /// `PLANE_SURF_1` (Universal Plane 1 GGTT Surface Address on Gen9+; write arms update).
    pub const fn reg_plane_surf(self) -> usize {
        self.display_base() + 0x19C
    }

    /// `PLANE_OFFSET_1` (Universal Plane 1 `(start_y << 16) | start_x` on Gen9+).
    pub const fn reg_plane_offset(self) -> usize {
        self.display_base() + 0x1A4
    }

    // ------------------------------------------------------------------------
    // Hardware Cursor Plane (`CURCNTR`, `CURBASE`, `CURPOS`)
    // ------------------------------------------------------------------------

    /// `CURCNTR` (Cursor Control).
    pub const fn reg_curcntr(self) -> usize {
        self.display_base() + 0x080
    }

    /// `CURBASE` (Cursor GGTT Base Address; write arms update).
    pub const fn reg_curbase(self) -> usize {
        self.display_base() + 0x084
    }

    /// `CURPOS` (Cursor Position).
    pub const fn reg_curpos(self) -> usize {
        self.display_base() + 0x088
    }
}

// ----------------------------------------------------------------------------
// Pipe & Plane Control Bitfields
// ----------------------------------------------------------------------------

/// `PIPECONF_ENABLE`: bit 31 enables the display pipe.
pub const PIPECONF_ENABLE: u32 = 1 << 31;
/// `PIPECONF_ACTIVE`: bit 30 indicates the pipe is active in hardware.
pub const PIPECONF_ACTIVE: u32 = 1 << 30;

/// `DISPLAY_PLANE_ENABLE` / `PLANE_CTL_ENABLE`: bit 31 enables the primary plane.
pub const PLANE_ENABLE: u32 = 1 << 31;

/// Pre-Gen9 `DSPCNTR` pixel format mask (`[29:26]`).
pub const DSPCNTR_FORMAT_MASK: u32 = 0xF << 26;
/// Pre-Gen9 `DSPCNTR` 32-bit BGRX8888 (`0b0110 << 26`).
pub const DSPCNTR_FORMAT_BGRX8888: u32 = 0x6 << 26;
/// Pre-Gen9 `DSPCNTR` 32-bit BGRA8888 (`0b0111 << 26`).
pub const DSPCNTR_FORMAT_BGRA8888: u32 = 0x7 << 26;
/// Pre-Gen9 `DSPCNTR` tiled surface bit (bit 10; 0 = linear).
pub const DSPCNTR_TILED: u32 = 1 << 10;

/// Gen9+ `PLANE_CTL` source pixel format mask (`[27:24]`).
pub const PLANE_CTL_FORMAT_MASK: u32 = 0xF << 24;
/// Gen9+ `PLANE_CTL` 32-bit RGBX8888 / BGRX8888 (`0b0100 << 24`).
pub const PLANE_CTL_FORMAT_XRGB8888: u32 = 0x4 << 24;
/// Gen9+ `PLANE_CTL` tiling mask (`[12:10]`; 0 = linear memory).
pub const PLANE_CTL_TILED_MASK: u32 = 0x7 << 10;
/// Gen9+ `PLANE_CTL` alpha mode mask (`[5:4]`; `0b10` = premultiplied, `0b00` = disabled).
pub const PLANE_CTL_ALPHA_MASK: u32 = 0x3 << 4;
pub const PLANE_CTL_ALPHA_PREMULTIPLIED: u32 = 0x2 << 4;

/// Surface base address mask (`[31:12]`, 4 KiB page frame aligned; hardware requires 64 KiB).
pub const SURFACE_ADDR_MASK: u32 = 0xFFFF_F000;

// ----------------------------------------------------------------------------
// Cursor Control & Position Bitfields
// ----------------------------------------------------------------------------

/// Cursor mode mask (`[5:0]`).
pub const CUR_MODE_MASK: u32 = 0x3F;
/// Cursor disabled.
pub const CUR_MODE_DISABLE: u32 = 0x00;
/// `64x64` 32-bpp ARGB hardware cursor mode.
pub const CUR_MODE_64_ARGB_AX: u32 = 0x27;
/// Pipe select bits in `CURCNTR` (`[29:28]`).
pub const CUR_PIPE_SELECT_SHIFT: u32 = 28;
pub const CUR_PIPE_SELECT_MASK: u32 = 0x3 << CUR_PIPE_SELECT_SHIFT;

/// Sign bit for X/Y coordinates in `CURPOS` (bit 15 of each 16-bit half).
pub const CURSOR_POS_SIGN: u32 = 1 << 15;
pub const CURSOR_POS_MAGNITUDE_MASK: u32 = 0x7FFF;

const fn encode_cursor_axis(coord: i32) -> u32 {
    if coord < 0 {
        CURSOR_POS_SIGN | ((-coord as u32) & CURSOR_POS_MAGNITUDE_MASK)
    } else {
        (coord as u32) & CURSOR_POS_MAGNITUDE_MASK
    }
}

/// Encodes signed `(x, y)` coordinates into Intel's sign-magnitude `CURPOS` register format.
pub const fn encode_cursor_pos(x: i32, y: i32) -> u32 {
    (encode_cursor_axis(y) << 16) | encode_cursor_axis(x)
}

// ----------------------------------------------------------------------------
// GMBUS / DDC I2C Controller for EDID reading (`0xC5100` PCH / `0x5100` GMCH)
// ----------------------------------------------------------------------------

/// South Display Engine (PCH / Gen7+) GMBUS base offset.
pub const PCH_GMBUS_BASE: usize = 0xC5100;

/// `GMBUS0`: Clock/Port select.
pub const GMBUS0: usize = PCH_GMBUS_BASE;
/// `GMBUS1`: Command/Status.
pub const GMBUS1: usize = PCH_GMBUS_BASE + 0x04;
/// `GMBUS2`: Status.
pub const GMBUS2: usize = PCH_GMBUS_BASE + 0x08;
/// `GMBUS3`: Data buffer (4 bytes per transfer step).
pub const GMBUS3: usize = PCH_GMBUS_BASE + 0x0C;
/// `GMBUS4`: Interrupt mask.
pub const GMBUS4: usize = PCH_GMBUS_BASE + 0x10;

/// GMBUS pin-pair selections for digital display ports (`DP`/`HDMI`/`eDP`).
pub const GMBUS_PIN_DPC: u32 = 4;
pub const GMBUS_PIN_DPB: u32 = 5;
pub const GMBUS_PIN_DPD: u32 = 6;
pub const GMBUS_PINS_PROBE_ORDER: [u32; 3] = [GMBUS_PIN_DPB, GMBUS_PIN_DPC, GMBUS_PIN_DPD];

/// `GMBUS1` command bits.
pub const GMBUS_SW_CLR_INT: u32 = 1 << 31;
pub const GMBUS_SW_RDY: u32 = 1 << 30;
pub const GMBUS_CYCLE_WAIT: u32 = 1 << 25;
pub const GMBUS_CYCLE_INDEX: u32 = 2 << 25;
pub const GMBUS_CYCLE_STOP: u32 = 4 << 25;
pub const GMBUS_BYTE_COUNT_SHIFT: u32 = 16;
pub const GMBUS_SLAVE_ADDR_SHIFT: u32 = 1;
pub const GMBUS_SLAVE_READ: u32 = 1 << 0;

/// Standard DDC/EDID 7-bit I2C slave address (`0x50`).
pub const EDID_I2C_ADDR: u32 = 0x50;

/// `GMBUS2` status bits.
pub const GMBUS_HW_RDY: u32 = 1 << 11;
pub const GMBUS_SATOER: u32 = 1 << 10;
pub const GMBUS_HW_WAIT_PHASE: u32 = 1 << 14;
