use crate::vm::vma::VmaManager;
use alloc::sync::Arc;
use core::cmp;
use ostd::Error;
use ostd::mm::{PAGE_SIZE, PageFlags, Vaddr};
use xmas_elf::ElfFile;
use xmas_elf::program::{ProgramHeader, Type};

/// Metadata for a loaded ELF executable image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoadedElf {
    /// Initial user instruction pointer from the ELF header.
    pub entry: Vaddr,
    /// Lowest mapped user virtual address.
    pub load_start: Vaddr,
    /// First byte after the highest mapped user virtual address.
    pub load_end: Vaddr,
}

/// Load an ELF executable into `vm` and return its entry metadata.
///
/// Segment parsing is delegated to `xmas-elf`; this code validates bounds,
/// maps page-aligned regions, copies file-backed bytes, and leaves BSS zeroed
/// by [`VmaManager::map_region`].
pub fn load_elf_image(vm: &Arc<VmaManager>, elf_image: &[u8]) -> Result<LoadedElf, Error> {
    let elf = ElfFile::new(elf_image).map_err(|_| Error::InvalidArgs)?;
    let mut load_start = usize::MAX;
    let mut load_end = 0usize;

    for ph in elf.program_iter() {
        if ph.get_type().map_err(|_| Error::InvalidArgs)? != Type::Load {
            continue;
        }

        let file_size = checked_usize(ph.file_size())?;
        let mem_size = checked_usize(ph.mem_size())?;
        if file_size > mem_size {
            return Err(Error::InvalidArgs);
        }
        if mem_size == 0 {
            continue;
        }

        let file_offset = checked_usize(ph.offset())?;
        let file_end = file_offset
            .checked_add(file_size)
            .ok_or(Error::InvalidArgs)?;
        if file_end > elf_image.len() {
            return Err(Error::InvalidArgs);
        }

        let segment_start = checked_usize(ph.virtual_addr())?;
        let segment_end = segment_start
            .checked_add(mem_size)
            .ok_or(Error::InvalidArgs)?;
        let map_start = align_down(segment_start);
        let map_end = align_up(segment_end)?;
        let map_size = map_end.checked_sub(map_start).ok_or(Error::InvalidArgs)?;

        let original_flags = page_flags_from_elf(ph);
        vm.map_region(map_start, map_size, original_flags | PageFlags::W)?;
        if file_size > 0 {
            vm.copy_to_user(segment_start, &elf_image[file_offset..file_end])?;
        }
        if !original_flags.contains(PageFlags::W) {
            vm.mprotect(map_start, map_size, original_flags)?;
        }

        load_start = cmp::min(load_start, map_start);
        load_end = cmp::max(load_end, map_end);
    }

    if load_end == 0 {
        return Err(Error::InvalidArgs);
    }

    Ok(LoadedElf {
        entry: checked_usize(elf.header.pt2.entry_point())?,
        load_start,
        load_end,
    })
}

fn page_flags_from_elf(ph: ProgramHeader<'_>) -> PageFlags {
    let flags = ph.flags();
    let mut page_flags = PageFlags::empty();
    if flags.is_read() || flags.is_write() {
        page_flags |= PageFlags::R;
    }
    if flags.is_write() {
        page_flags |= PageFlags::W;
    }
    if flags.is_execute() {
        page_flags |= PageFlags::X;
    }
    page_flags
}

fn checked_usize(value: u64) -> Result<usize, Error> {
    usize::try_from(value).map_err(|_| Error::InvalidArgs)
}

fn align_down(addr: Vaddr) -> Vaddr {
    addr / PAGE_SIZE * PAGE_SIZE
}

fn align_up(addr: Vaddr) -> Result<Vaddr, Error> {
    addr.checked_add(PAGE_SIZE - 1)
        .map(align_down)
        .ok_or(Error::InvalidArgs)
}
