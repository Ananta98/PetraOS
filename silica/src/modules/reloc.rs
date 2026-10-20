// SPDX-License-Identifier: GPL-2.0

//! x86_64 ELF relocation support for loadable kernel modules.
//!
//! Applies RELA entries of both relocatable objects (`ET_REL`) and
//! position-independent shared objects (`ET_DYN`) against the loaded image.

use alloc::collections::btree_map::BTreeMap;
use xmas_elf::{
    ElfFile, P64,
    sections::{Rela, SHN_ABS, SHN_COMMON, SHN_UNDEF, SectionData, ShType},
    symbol_table::{DynEntry64, Entry, Entry64},
};

use crate::api::errno::{Errno, Result};
use crate::return_errno;

use super::symbols::lookup_symbol;

// x86_64 relocation types (System V ABI).
const R_X86_64_NONE: u32 = 0;
const R_X86_64_64: u32 = 1;
const R_X86_64_PC32: u32 = 2;
const R_X86_64_GLOB_DAT: u32 = 6;
const R_X86_64_JUMP_SLOT: u32 = 7;
const R_X86_64_RELATIVE: u32 = 8;
const R_X86_64_32S: u32 = 11;
const R_X86_64_PC64: u32 = 24;

/// Everything the relocation engine needs to know about the loaded image.
pub(crate) struct ImageLayout {
    /// Start address of the relocated image in kernel virtual memory.
    pub base: usize,
    /// ET_DYN only: ELF virtual address corresponding to `base`, i.e. the
    /// page-aligned minimum PT_LOAD vaddr. Always 0 for ET_REL.
    pub vaddr_min: usize,
    /// True when the object is `ET_REL` (per-section relocation layout).
    pub is_rel: bool,
    /// ET_REL only: section index -> offset of the section inside the image.
    pub section_offsets: BTreeMap<u16, usize>,
}

impl ImageLayout {
    /// Translates an ET_DYN ELF virtual address into the corresponding
    /// address inside the loaded image.
    pub(crate) fn translate(&self, vaddr: usize) -> Result<usize> {
        let rel = vaddr.checked_sub(self.vaddr_min).ok_or(Errno::ENOEXEC)?;
        self.base.checked_add(rel).ok_or(Errno::ENOEXEC)
    }
}

/// A symbol table in either form a module can carry.
enum Symtab<'a> {
    Static(&'a [Entry64]),
    Dynamic(&'a [DynEntry64]),
}

impl<'a> Symtab<'a> {
    fn len(&self) -> usize {
        match self {
            Self::Static(entries) => entries.len(),
            Self::Dynamic(entries) => entries.len(),
        }
    }
}

fn symtab_at<'a>(elf: &'a ElfFile<'a>, index: usize) -> Result<Option<Symtab<'a>>> {
    if index == 0 {
        return Ok(None);
    }
    let header = elf
        .section_header(index as u16)
        .map_err(|_| Errno::ENOEXEC)?;
    match header.get_data(elf).map_err(|_| Errno::ENOEXEC)? {
        SectionData::SymbolTable64(entries) => Ok(Some(Symtab::Static(entries))),
        SectionData::DynSymbolTable64(entries) => Ok(Some(Symtab::Dynamic(entries))),
        _ => return_errno!(ENOEXEC, "RELA section does not link to a symbol table"),
    }
}

/// Resolves the address `S` of a symbol entry against the loaded layout.
pub(crate) fn symbol_address<E: Entry>(
    elf: &ElfFile,
    sym: &E,
    layout: &ImageLayout,
) -> Result<u64> {
    let shndx = sym.shndx();
    if shndx == SHN_UNDEF {
        let name = sym.get_name(elf).map_err(|_| {
            ostd::error!("Module relocation references an unnamed undefined symbol");
            Errno::ENOENT
        })?;
        return lookup_symbol(name).map(|addr| addr as u64).ok_or_else(|| {
            ostd::error!("Unknown symbol \"{}\" in module relocation", name);
            Errno::ENOENT
        });
    }
    if shndx == SHN_ABS {
        return Ok(sym.value());
    }
    if shndx == SHN_COMMON {
        return_errno!(ENOEXEC, "module defines SHN_COMMON symbols (unsupported)");
    }
    if layout.is_rel {
        let offset = layout
            .section_offsets
            .get(&shndx)
            .ok_or_else(|| {
                ostd::error!("Module symbol references non-loaded section #{}", shndx);
                Errno::ENOEXEC
            })?
            .checked_add(sym.value() as usize)
            .ok_or(Errno::ENOEXEC)?;
        Ok(layout.base as u64 + offset as u64)
    } else {
        layout.translate(sym.value() as usize).map(|a| a as u64)
    }
}

/// Writes `value` at absolute address `place` inside the module image.
fn write_entry(image: &mut [u8], place: usize, value: u64, width: usize) -> Result<()> {
    let Some(adjusted) = place.checked_sub(image.as_ptr() as usize) else {
        return_errno!(ENOEXEC, "relocation target outside module image");
    };
    let end = adjusted.checked_add(width).ok_or(Errno::ENOEXEC)?;
    if end > image.len() {
        return_errno!(ENOEXEC, "relocation target outside module image");
    }
    image[adjusted..end].copy_from_slice(&value.to_le_bytes()[..width]);
    Ok(())
}

/// Applies every SHT_RELA section of `elf` to the relocated `image`.
pub(crate) fn apply_relocations(
    elf: &ElfFile,
    image: &mut [u8],
    layout: &ImageLayout,
) -> Result<()> {
    let section_count = elf.header.pt2.sh_count() as usize;
    for index in 1..section_count {
        let section = elf
            .section_header(index as u16)
            .map_err(|_| Errno::ENOEXEC)?;
        if section.get_type() != Ok(ShType::Rela) {
            continue;
        }
        let relas = match section.get_data(elf) {
            Ok(SectionData::Rela64(relas)) => relas,
            _ => return_errno!(ENOEXEC, "malformed RELA section"),
        };
        let symtab = symtab_at(elf, section.link() as usize)?;
        // ET_REL: r_offset is relative to the relocation target section
        // (sh_info). Relocations against non-loaded sections (e.g. debug
        // data) are skipped, mirroring the kernel's partial loader behavior.
        let target_offset = if layout.is_rel {
            match layout.section_offsets.get(&(section.info() as u16)) {
                Some(&offset) => offset,
                None => continue,
            }
        } else {
            0
        };

        for rela in relas {
            let reloc_type = rela.get_type();
            if reloc_type == R_X86_64_NONE {
                continue;
            }

            let addend = rela.get_addend();
            let place = if layout.is_rel {
                layout
                    .base
                    .checked_add(target_offset)
                    .and_then(|a| a.checked_add(rela.get_offset() as usize))
                    .ok_or(Errno::ENOEXEC)?
            } else {
                // ET_DYN: r_offset is an absolute module virtual address.
                layout.translate(rela.get_offset() as usize)?
            };

            let (value, width) = match reloc_type {
                R_X86_64_RELATIVE => {
                    // Implicit addend: value = load bias + addend.
                    let bias = layout
                        .base
                        .checked_sub(layout.vaddr_min)
                        .ok_or(Errno::ENOEXEC)? as u64;
                    (bias.wrapping_add(addend), 8)
                }
                R_X86_64_64 => (value_of(symtab.as_ref(), elf, rela, layout)? + addend, 8),
                R_X86_64_GLOB_DAT | R_X86_64_JUMP_SLOT => {
                    (value_of(symtab.as_ref(), elf, rela, layout)?, 8)
                }
                R_X86_64_PC32 | R_X86_64_32S => {
                    let pc_relative = (value_of(symtab.as_ref(), elf, rela, layout)? as i64)
                        .wrapping_add(addend as i64)
                        .wrapping_sub(place as i64) as u64;
                    (pc_relative, 4)
                }
                R_X86_64_PC64 => {
                    let pc_relative = (value_of(symtab.as_ref(), elf, rela, layout)? as i64)
                        .wrapping_add(addend as i64)
                        .wrapping_sub(place as i64) as u64;
                    (pc_relative, 8)
                }
                other => {
                    ostd::error!("Unsupported module relocation type {}", other);
                    return_errno!(ENOEXEC, "unsupported relocation type");
                }
            };
            write_entry(image, place, value, width)?;
        }
    }
    Ok(())
}

/// Computes `S` (the resolved symbol address) for a relocation entry.
///
/// Tables without a linked symbol table can only carry `R_X86_64_RELATIVE`
/// entries (which never consult symbols); anything else is rejected.
fn value_of(
    symtab: Option<&Symtab<'_>>,
    elf: &ElfFile,
    rela: &Rela<P64>,
    layout: &ImageLayout,
) -> Result<u64> {
    let Some(symtab) = symtab else {
        return_errno!(
            ENOEXEC,
            "relocation requires symbols without a symbol table"
        );
    };
    let index = rela.get_symbol_table_index() as usize;
    if index >= symtab.len() {
        return_errno!(ENOEXEC, "relocation symbol index out of bounds");
    }
    match symtab {
        Symtab::Static(entries) => symbol_address(elf, &entries[index], layout),
        Symtab::Dynamic(entries) => symbol_address(elf, &entries[index], layout),
    }
}
