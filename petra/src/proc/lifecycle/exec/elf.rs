// SPDX-License-Identifier: GPL-2.0

//! ELF binary parsing and segment mapping for process address spaces.

use alloc::{string::String, sync::Arc, vec::Vec};
use xmas_elf::{
    ElfFile,
    header::{Class, Data, Machine, Type as HeaderType},
    program::Type,
};

use crate::{
    uapi::errno::{Errno, Result},
    utils::UserPtr,
    vm::{VMAR_CAP_ADDR, VMAR_LOWEST_ADDR, Vmar, flags::MmapFlags, perms::VmPerms},
};

/// Default base address for loading Position-Independent Executables (PIE, ET_DYN).
pub const DEFAULT_PIE_LOAD_BIAS: usize = 0x0000_5555_5555_0000;

/// Default base address for loading the dynamic linker interpreter (`PT_INTERP`, e.g. ld-musl).
pub const DEFAULT_INTERP_LOAD_BIAS: usize = 0x0000_7f00_0000_0000;

/// Metadata collected when loading an ELF image into a VMAR.
#[derive(Debug, Clone)]
pub struct ElfLoadInfo {
    /// Virtual address of the binary's entry point (adjusted by `load_bias`).
    pub entry_point: usize,
    /// Virtual address where the program headers table resides in memory (`AT_PHDR`).
    pub phdr_addr: usize,
    /// Size in bytes of each program header entry (`AT_PHENT`).
    pub phent_size: usize,
    /// Number of program headers (`AT_PHNUM`).
    pub phnum: usize,
    /// Path to the dynamic linker interpreter if `PT_INTERP` was present.
    pub interp_path: Option<String>,
    /// Load bias applied to segments.
    pub load_bias: usize,
    /// End address of the highest mapped segment, used to initialize process `brk`.
    pub brk_init: usize,
}

/// Loads an ELF binary image into `vmar` applying a suggested PIE load bias if the image is `ET_DYN`.
pub fn load_elf_with_bias(
    vmar: &Arc<Vmar>,
    elf_data: &[u8],
    suggested_pie_bias: usize,
) -> Result<ElfLoadInfo> {
    let aligned_buf;
    let elf_data = if !(elf_data.as_ptr() as usize).is_multiple_of(core::mem::align_of::<u64>()) {
        aligned_buf = Vec::from(elf_data);
        &aligned_buf[..]
    } else {
        elf_data
    };

    let elf = ElfFile::new(elf_data).map_err(|_| Errno::ENOEXEC)?;

    // 1. Validate ELF Header
    if elf.header.pt1.class() != Class::SixtyFour {
        crate::return_errno!(ENOEXEC, "ELF must be 64-bit");
    }
    if elf.header.pt1.data() != Data::LittleEndian {
        crate::return_errno!(ENOEXEC, "ELF must be little-endian");
    }
    if elf.header.pt2.machine().as_machine() != Machine::X86_64 {
        crate::return_errno!(ENOEXEC, "ELF target machine must be x86_64");
    }

    // Determine load bias based on ELF type
    let load_bias = match elf.header.pt2.type_().as_type() {
        HeaderType::Executable => 0,
        HeaderType::SharedObject => suggested_pie_bias,
        _ => crate::return_errno!(ENOEXEC, "ELF type must be ET_EXEC or ET_DYN"),
    };

    let entry_point = (elf.header.pt2.entry_point() as usize)
        .checked_add(load_bias)
        .ok_or(Errno::ENOEXEC)?;

    let phent_size = elf.header.pt2.ph_entry_size() as usize;
    let phnum = elf.header.pt2.ph_count() as usize;

    let mut interp_path: Option<String> = None;
    let mut phdr_addr: Option<usize> = None;
    let mut first_load_vaddr: Option<usize> = None;
    let mut max_seg_end = 0;

    // Scan program headers for PT_INTERP, PT_PHDR, and record first load segment address
    for ph in elf.program_iter() {
        match ph.get_type() {
            Ok(Type::Interp) => {
                let offset = ph.offset() as usize;
                let file_sz = ph.file_size() as usize;
                if offset.saturating_add(file_sz) <= elf_data.len() {
                    let interp_bytes = &elf_data[offset..offset + file_sz];
                    let len = interp_bytes
                        .iter()
                        .position(|&b| b == 0)
                        .unwrap_or(interp_bytes.len());
                    if let Ok(s) = core::str::from_utf8(&interp_bytes[..len]) {
                        let trimmed = s.trim();
                        if !trimmed.is_empty() {
                            interp_path = Some(String::from(trimmed));
                        }
                    }
                }
            }
            Ok(Type::Phdr) => {
                phdr_addr = Some((ph.virtual_addr() as usize).wrapping_add(load_bias));
            }
            Ok(Type::Load) => {
                if first_load_vaddr.is_none() {
                    first_load_vaddr = Some((ph.virtual_addr() as usize).wrapping_add(load_bias));
                }
            }
            _ => {}
        }
    }

    let phdr_addr = phdr_addr.unwrap_or_else(|| {
        let base = first_load_vaddr.unwrap_or(load_bias);
        base.wrapping_add(elf.header.pt2.ph_offset() as usize)
    });

    // 2. Iterate and map PT_LOAD segments
    for ph in elf.program_iter() {
        if ph.get_type() != Ok(Type::Load) {
            continue;
        }

        let raw_vaddr = ph.virtual_addr() as usize;
        let vaddr = raw_vaddr.checked_add(load_bias).ok_or(Errno::ENOEXEC)?;
        let mem_sz = ph.mem_size() as usize;
        let file_sz = ph.file_size() as usize;
        let offset = ph.offset() as usize;

        if mem_sz < file_sz {
            crate::return_errno!(ENOEXEC, "segment mem_sz < file_sz");
        }
        if mem_sz == 0 {
            continue;
        }

        let page_start = vaddr & !(ostd::mm::PAGE_SIZE - 1);
        let page_offset = vaddr - page_start;
        let total_span =
            (page_offset + mem_sz + ostd::mm::PAGE_SIZE - 1) & !(ostd::mm::PAGE_SIZE - 1);

        let seg_end = page_start + total_span;
        if seg_end > max_seg_end {
            max_seg_end = seg_end;
        }

        if page_start < VMAR_LOWEST_ADDR || page_start.saturating_add(total_span) > VMAR_CAP_ADDR {
            crate::return_errno!(
                ENOEXEC,
                "segment address range outside allowed userspace limits"
            );
        }

        let mut final_perms = VmPerms::empty();
        if ph.flags().is_read() {
            final_perms |= VmPerms::READ;
        }
        if ph.flags().is_write() {
            final_perms |= VmPerms::WRITE;
        }
        if ph.flags().is_execute() {
            final_perms |= VmPerms::EXEC;
        }

        // Map segment with staging write permission so kernel can populate and copy
        let staging_perms = final_perms | VmPerms::WRITE;
        vmar.mmap_anonymous(
            page_start,
            total_span,
            staging_perms,
            MmapFlags::PRIVATE | MmapFlags::FIXED,
        )?;

        // Populate physical frames
        vmar.populate_range(&(page_start..page_start + total_span))?;

        // Activate page table on current CPU to write file bytes
        vmar.activate();

        if file_sz > 0 {
            if offset + file_sz > elf_data.len() {
                crate::return_errno!(ENOEXEC, "ELF segment file offset out of range");
            }
            UserPtr::new(vaddr)
                .write_bytes(vmar, &elf_data[offset..offset + file_sz])
                .map_err(|_| Errno::EIO)?;
        }

        // Zero out trailing partial-page bytes between file_sz and mem_sz if within same page
        if mem_sz > file_sz {
            let bss_start = vaddr + file_sz;
            let bss_page_rem = ostd::mm::PAGE_SIZE - (bss_start & (ostd::mm::PAGE_SIZE - 1));
            let zero_len = (mem_sz - file_sz).min(bss_page_rem);
            if zero_len > 0 {
                let mut writer = vmar
                    .vm_space()
                    .writer(bss_start, zero_len)
                    .map_err(|_| Errno::EIO)?;
                let _ = writer.fill_zeros(zero_len);
            }
        }

        // Lock down permissions to enforce W^X if writable was not requested
        if !final_perms.contains(VmPerms::WRITE) {
            vmar.mprotect(final_perms, page_start..page_start + total_span)?;
        }
    }

    let brk_init = if max_seg_end > 0 {
        max_seg_end
    } else {
        load_bias + 0x0040_0000
    };

    Ok(ElfLoadInfo {
        entry_point,
        phdr_addr,
        phent_size,
        phnum,
        interp_path,
        load_bias,
        brk_init,
    })
}

/// Loads an ELF binary image into `vmar` and returns the entry point virtual address.
pub fn load_elf(vmar: &Arc<Vmar>, elf_data: &[u8]) -> Result<usize> {
    let info = load_elf_with_bias(vmar, elf_data, DEFAULT_PIE_LOAD_BIAS)?;
    vmar.init_brk(info.brk_init);
    Ok(info.entry_point)
}
