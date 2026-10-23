// SPDX-License-Identifier: GPL-2.0

//! Intel Global Graphics Translation Table (`GGTT`) and Graphics Aperture manager.
//!
//! On Intel GPUs (`Gen7` through `Gen12+`), `BAR0` (`GTTMMADR`) is divided into
//! two halves:
//! - Lower half (`0 .. bar0_size / 2`): MMIO register space
//! - Upper half (`bar0_size / 2 .. bar0_size`): Global GTT page-table array
//!
//! Display engine planes (`DSPSURF` / `PLANE_SURF` and `CURBASE`) fetch pixels
//! using graphics virtual addresses (`gtt_offset`) translated through the GGTT.
//! [`GgttManager`] allocates 64 KiB-aligned GGTT regions for [`GemObject`]s and
//! writes their physical DMA page addresses into the hardware GGTT PTE array.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use ostd::io::IoMem;
use ostd::mm::{PAGE_SIZE, VmIo};

use super::regs::{
    GFX_FLSH_CNTL_EN, GGTT_PTE_CACHE_LLC, GGTT_PTE_VALID, GGTT_SURFACE_ALIGN, IntelGen,
    REG_GFX_FLSH_CNTL,
};
use crate::drivers::drm::GemObject;
use crate::uapi::errno::{Errno, Result};

/// Offset reserved at the start of the GGTT (16 MiB) to preserve BIOS/GOP
/// stolen-memory boot scanout surfaces during initialization.
const GGTT_RESERVED_BASE: usize = 16 * 1024 * 1024;

/// Fallback GGTT addressable capacity (256 MiB) when BAR0 does not expose a
/// writable GTT half.
const GGTT_FALLBACK_CAPACITY: usize = 256 * 1024 * 1024;

/// A bound [`GemObject`] region inside the Global GTT.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GgttBinding {
    /// 64 KiB-aligned graphics virtual address (in bytes from start of GGTT).
    pub gtt_offset: u32,
    /// Size of the bound region in bytes (page-aligned).
    pub size: usize,
}

/// Manages GGTT virtual address allocation and PTE programming for GEM objects.
pub struct GgttManager {
    generation: IntelGen,
    /// Offset in `BAR0` where the GGTT PTE array starts (`bar0_size / 2`), or `0` if unavailable.
    gtt_bar_offset: usize,
    /// Total GGTT virtual address space covered by the table in bytes.
    total_gtt_bytes: usize,
    /// Bump pointer for the next 64 KiB-aligned allocation.
    next_offset: usize,
    /// Reclaimed `(gtt_offset, size)` ranges from released framebuffers/cursors.
    free_ranges: Vec<(usize, usize)>,
    /// Active bindings keyed by `GemObject::id`.
    bindings: BTreeMap<u32, GgttBinding>,
}

impl GgttManager {
    /// Creates a new `GgttManager` from the device generation and `BAR0` size.
    pub fn new(generation: IntelGen, bar0_size: usize) -> Self {
        let gtt_bar_offset = bar0_size / 2;
        let gtt_table_bytes = bar0_size.saturating_sub(gtt_bar_offset);
        let pte_size = if generation.uses_64bit_gtt_pte() { 8 } else { 4 };
        let mapped_bytes = (gtt_table_bytes / pte_size) * PAGE_SIZE;

        let total_gtt_bytes = if mapped_bytes > GGTT_RESERVED_BASE * 2 {
            mapped_bytes
        } else {
            GGTT_FALLBACK_CAPACITY
        };

        Self {
            generation,
            gtt_bar_offset: if mapped_bytes > GGTT_RESERVED_BASE * 2 {
                gtt_bar_offset
            } else {
                0
            },
            total_gtt_bytes,
            next_offset: GGTT_RESERVED_BASE,
            free_ranges: Vec::new(),
            bindings: BTreeMap::new(),
        }
    }

    /// Returns the existing [`GgttBinding`] for `obj`, or allocates a new
    /// 64 KiB-aligned GGTT range and programs the hardware PTEs.
    pub fn bind_object(&mut self, mmio: Option<&IoMem>, obj: &GemObject) -> Result<GgttBinding> {
        if let Some(binding) = self.bindings.get(&obj.id) {
            return Ok(*binding);
        }

        let aligned_size = obj.size().div_ceil(GGTT_SURFACE_ALIGN) * GGTT_SURFACE_ALIGN;
        let gtt_offset = self.alloc_range(aligned_size)?;

        if let Some(io) = mmio
            && self.gtt_bar_offset != 0
        {
            self.write_ptes(io, gtt_offset, obj.paddr(), obj.size());
        }

        let binding = GgttBinding {
            gtt_offset: gtt_offset as u32,
            size: aligned_size,
        };
        self.bindings.insert(obj.id, binding);
        Ok(binding)
    }

    /// Unbinds the GGTT range associated with `obj_id` and returns it to the free list.
    pub fn unbind_object(&mut self, obj_id: u32) {
        if let Some(binding) = self.bindings.remove(&obj_id) {
            self.free_ranges
                .push((binding.gtt_offset as usize, binding.size));
        }
    }

    /// Looks up the active [`GgttBinding`] for `obj_id`, if bound.
    pub fn binding_of(&self, obj_id: u32) -> Option<GgttBinding> {
        self.bindings.get(&obj_id).copied()
    }

    fn alloc_range(&mut self, aligned_size: usize) -> Result<usize> {
        if let Some(idx) = self
            .free_ranges
            .iter()
            .position(|&(_, sz)| sz >= aligned_size)
        {
            let (offset, sz) = self.free_ranges.swap_remove(idx);
            if sz > aligned_size {
                self.free_ranges
                    .push((offset + aligned_size, sz - aligned_size));
            }
            return Ok(offset);
        }

        let start = self.next_offset.div_ceil(GGTT_SURFACE_ALIGN) * GGTT_SURFACE_ALIGN;
        let end = start.checked_add(aligned_size).ok_or(Errno::ENOMEM)?;
        if end > self.total_gtt_bytes {
            crate::return_errno!(ENOMEM, "i915 GGTT aperture exhausted");
        }
        self.next_offset = end;
        Ok(start)
    }

    /// Programs the GGTT PTEs for `[gtt_offset .. gtt_offset + size]` to point
    /// at the contiguous physical region starting at `base_paddr`.
    fn write_ptes(&self, mmio: &IoMem, gtt_offset: usize, base_paddr: usize, size: usize) {
        let first_page = gtt_offset / PAGE_SIZE;
        let num_pages = size.div_ceil(PAGE_SIZE);

        if self.generation.uses_64bit_gtt_pte() {
            for page_idx in 0..num_pages {
                let paddr = (base_paddr + page_idx * PAGE_SIZE) as u64;
                let pte = (paddr & !0xFFF) | GGTT_PTE_CACHE_LLC | GGTT_PTE_VALID;
                let pte_offset = self.gtt_bar_offset + (first_page + page_idx) * 8;
                let _ = mmio.write_val::<u64>(pte_offset, &pte);
            }
        } else {
            for page_idx in 0..num_pages {
                let paddr = base_paddr + page_idx * PAGE_SIZE;
                // Gen6/Gen7 32-bit PTE: physical address [31:12], bits [11:4] high paddr [39:32].
                let high_bits = ((paddr as u64 >> 28) & 0xFF0) as u32;
                let pte = (paddr as u32 & 0xFFFF_F000)
                    | high_bits
                    | (GGTT_PTE_CACHE_LLC as u32)
                    | (GGTT_PTE_VALID as u32);
                let pte_offset = self.gtt_bar_offset + (first_page + page_idx) * 4;
                let _ = mmio.write_val::<u32>(pte_offset, &pte);
            }
        }

        // Flush chipset GTT TLB / write buffer and perform a posting read.
        let _ = mmio.write_val::<u32>(REG_GFX_FLSH_CNTL, &GFX_FLSH_CNTL_EN);
        let _ = mmio.read_val::<u32>(REG_GFX_FLSH_CNTL);
    }
}
