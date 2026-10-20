// SPDX-License-Identifier: GPL-2.0

//! ELF loader for loadable kernel modules.
//!
//! Supports both position-independent shared objects (`.so`/`.kso`, `ET_DYN`)
//! and relocatable object files (`.ko`, `ET_REL`). The module image is copied
//! into owned kernel memory and relocations are applied against the runtime
//! addresses of the image and the kernel symbol table.
//!
//! The loaded image currently resides in non-executable kernel memory;
//! invoking module entry points requires OSTD kernel-VA mapping support
//! (tracked as follow-up work).

use alloc::{
    borrow::ToOwned, collections::btree_map::BTreeMap, format, string::String, vec, vec::Vec,
};
use xmas_elf::{
    ElfFile,
    header::{Class, Data, Machine, Type as ElfType},
    program::Type as ProgramType,
    sections::{SectionData, ShType},
    symbol_table::Entry,
};

use crate::api::errno::{Errno, Result};
use crate::return_errno;

use super::{
    params::parse_modparams,
    registry::{MODULE_TABLE, ModuleInfo, ModuleState, register_loaded_module, rename_module},
    reloc::{ImageLayout, apply_relocations, symbol_address},
};

/// Loads and validates a kernel module from an ELF binary image.
///
/// Returns the registered module name, which is taken from `.modinfo` /
/// `.mod.*` metadata when present, or synthesized from the resolved entry
/// point otherwise.
pub fn load_module(elf_data: &[u8], cmdline: &str) -> Result<String> {
    let elf = ElfFile::new(elf_data).map_err(|_| Errno::ENOEXEC)?;
    validate_header(&elf)?;

    let metadata = parse_modinfo(&elf);
    let params = parse_modparams(cmdline);

    let (image, section_offsets, vaddr_min, is_rel) = match elf.header.pt2.type_().as_type() {
        ElfType::SharedObject => {
            let (image, vaddr_min) = load_image_dyn(&elf)?;
            (image, BTreeMap::new(), vaddr_min, false)
        }
        ElfType::Relocatable => {
            let (image, section_offsets) = load_image_rel(&elf)?;
            (image, section_offsets, 0, true)
        }
        _ => return_errno!(ENOEXEC, "module ELF type must be ET_DYN or ET_REL"),
    };

    let layout = ImageLayout {
        base: image.as_ptr() as usize,
        vaddr_min,
        is_rel,
        section_offsets,
    };

    let entry_point = resolve_entry_point(&elf, &layout)?;
    check_dependencies(&metadata.depends)?;

    let mut image = image;
    apply_relocations(&elf, &mut image, &layout)?;

    let mod_name = if metadata.name.is_empty() {
        format!("module_{:x}", entry_point)
    } else {
        metadata.name
    };

    let module_info = ModuleInfo {
        name: mod_name.clone(),
        version: metadata.version,
        description: metadata.description,
        author: metadata.author,
        load_base: layout.base,
        load_size: image.len(),
        entry_point,
        state: ModuleState::Live,
        dependencies: metadata.depends,
        params,
    };

    ostd::info!(
        "Loaded module \"{}\" (v:{}, auth:{}) at {:#x} (size: {} KB, entry: {:#x}) [cmdline: \"{}\"]",
        module_info.name,
        if module_info.version.is_empty() {
            "0.1.0"
        } else {
            &module_info.version
        },
        if module_info.author.is_empty() {
            "unknown"
        } else {
            &module_info.author
        },
        module_info.load_base,
        module_info.load_size / 1024,
        module_info.entry_point,
        cmdline
    );

    match register_loaded_module(module_info, image.into_boxed_slice()) {
        Ok(()) => Ok(mod_name),
        Err(Errno::EEXIST) => {
            ostd::error!("Module \"{}\" is already loaded", mod_name);
            Err(Errno::EEXIST)
        }
        Err(err) => Err(err),
    }
}

/// Loads a kernel module from a filesystem path on the rootfs VFS.
pub fn load_module_from_file(path: &str, cmdline: &str) -> Result<String> {
    let elf_data = crate::proc::lifecycle::read_binary_data(path)?;
    let mut mod_name = load_module(&elf_data, cmdline)?;

    // If the module didn't specify an explicit name, infer from file stem.
    if mod_name.starts_with("module_") {
        let file_name = path.rsplit('/').next().unwrap_or(path);
        let stem = file_name
            .trim_end_matches(".kso")
            .trim_end_matches(".so")
            .trim_end_matches(".ko");
        if !stem.is_empty() {
            rename_module(&mod_name, stem)?;
            mod_name = stem.to_owned();
        }
    }
    Ok(mod_name)
}

fn validate_header(elf: &ElfFile) -> Result<()> {
    if elf.header.pt1.class() != Class::SixtyFour {
        return_errno!(ENOEXEC, "module ELF must be 64-bit");
    }
    if elf.header.pt1.data() != Data::LittleEndian {
        return_errno!(ENOEXEC, "module ELF must be little-endian");
    }
    #[cfg(target_arch = "x86_64")]
    if elf.header.pt2.machine().as_machine() != Machine::X86_64 {
        return_errno!(ENOEXEC, "module ELF target machine must be x86_64");
    }
    match elf.header.pt2.type_().as_type() {
        ElfType::SharedObject | ElfType::Relocatable => Ok(()),
        _ => return_errno!(ENOEXEC, "module ELF type must be ET_DYN or ET_REL"),
    }
}

fn align_up_checked(value: usize, align: usize) -> Option<usize> {
    let value = value.checked_add(align.checked_sub(1)?)?;
    Some(value & !(align - 1))
}

fn align_down(value: usize, align: usize) -> usize {
    value & !(align - 1)
}

/// Copies every PT_LOAD segment into a zeroed image buffer.
///
/// Returns the image and the ELF virtual address that maps to offset 0 of it.
fn load_image_dyn(elf: &ElfFile) -> Result<(Vec<u8>, usize)> {
    let page_size = ostd::mm::PAGE_SIZE;
    let mut load_min = usize::MAX;
    let mut load_end = 0usize;
    let mut segments: Vec<(usize, usize, usize)> = Vec::new(); // (vaddr, file offset, filesz)

    for ph in elf.program_iter() {
        if ph.get_type() != Ok(ProgramType::Load) {
            continue;
        }
        let vaddr = ph.virtual_addr() as usize;
        let memsz = ph.mem_size() as usize;
        let filesz = ph.file_size() as usize;
        if memsz == 0 {
            continue;
        }
        if filesz > memsz {
            return_errno!(ENOEXEC, "module segment filesz exceeds memsz");
        }

        let aligned_vaddr = align_down(vaddr, page_size);
        let misalign = vaddr - aligned_vaddr;
        let span = align_up_checked(memsz.checked_add(misalign).ok_or(Errno::ENOMEM)?, page_size)
            .ok_or(Errno::ENOMEM)?;
        let end = aligned_vaddr.checked_add(span).ok_or(Errno::ENOMEM)?;

        load_min = load_min.min(aligned_vaddr);
        load_end = load_end.max(end);
        segments.push((vaddr, ph.offset() as usize, filesz));
    }

    if segments.is_empty() {
        return_errno!(ENOEXEC, "module ELF has no loadable segments");
    }

    let total_size = load_end - load_min;
    let mut image = vec![0u8; total_size];
    for (vaddr, file_offset, filesz) in &segments {
        let src = elf
            .input
            .get(*file_offset..*file_offset + *filesz)
            .ok_or(Errno::ENOEXEC)?;
        let dst = vaddr - load_min;
        image[dst..dst + *filesz].copy_from_slice(src);
    }
    Ok((image, load_min))
}

/// Lays out every `SHF_ALLOC` section of a relocatable object contiguously.
///
/// Returns the zeroed image and a per-section offset map.
fn load_image_rel(elf: &ElfFile) -> Result<(Vec<u8>, BTreeMap<u16, usize>)> {
    const SHF_ALLOC: u64 = 0x2;

    let section_count = elf.header.pt2.sh_count() as u16;
    let mut offsets = BTreeMap::new();
    let mut cursor = 0usize;

    for index in 1..section_count {
        let section = elf.section_header(index).map_err(|_| Errno::ENOEXEC)?;
        if section.flags() & SHF_ALLOC == 0 {
            continue;
        }
        let align = (section.align() as usize).max(1);
        if !align.is_power_of_two() {
            return_errno!(ENOEXEC, "module section alignment is not a power of two");
        }
        let placed = align_up_checked(cursor, align).ok_or(Errno::ENOMEM)?;
        offsets.insert(index, placed);
        cursor = placed
            .checked_add(section.size() as usize)
            .ok_or(Errno::ENOMEM)?;
    }

    if cursor == 0 {
        return_errno!(ENOEXEC, "relocatable module has no allocatable sections");
    }

    let mut image = vec![0u8; cursor];

    // Copy initialized parts; NOBITS sections (e.g. .bss) stay zero-filled.
    for index in 1..section_count {
        let Some(offset) = offsets.get(&index).copied() else {
            continue;
        };
        let section = elf.section_header(index).map_err(|_| Errno::ENOEXEC)?;
        if section.get_type() == Ok(ShType::NoBits) {
            continue;
        }
        let file_offset = section.offset() as usize;
        let size = section.size() as usize;
        let src = elf
            .input
            .get(file_offset..file_offset + size)
            .ok_or(Errno::ENOEXEC)?;
        image[offset..offset + size].copy_from_slice(src);
    }
    Ok((image, offsets))
}

const ENTRY_SYMBOL_NAMES: [&str; 3] = ["init_module", "_init", "init"];

/// Resolves the absolute address of the module init function.
///
/// ET_DYN modules use the ELF entry point when it is set; otherwise the
/// module's symbol tables are searched for a conventional init symbol
/// (`init_module`/`_init`/`init`).
fn resolve_entry_point(elf: &ElfFile, layout: &ImageLayout) -> Result<usize> {
    if !layout.is_rel {
        let vaddr = elf.header.pt2.entry_point() as usize;
        if vaddr != 0 {
            return layout.translate(vaddr);
        }
    }

    if let Some(entry) = for_init_symbol(elf, layout)? {
        return Ok(entry);
    }
    return_errno!(
        ENOEXEC,
        "module defines no entry point (init_module/_init/init)"
    );
}

/// Searches the module's symbol tables for a conventional init symbol
/// (`init_module`/`_init`/`init`) and resolves it against `layout`.
fn for_init_symbol(elf: &ElfFile, layout: &ImageLayout) -> Result<Option<usize>> {
    let section_count = elf.header.pt2.sh_count() as u16;
    for index in 1..section_count {
        let section = elf.section_header(index).map_err(|_| Errno::ENOEXEC)?;
        match section.get_data(elf) {
            Ok(SectionData::SymbolTable64(entries)) => {
                if let Some(entry) = init_in_table(elf, entries, layout)? {
                    return Ok(Some(entry));
                }
            }
            Ok(SectionData::DynSymbolTable64(entries)) => {
                if let Some(entry) = init_in_table(elf, entries, layout)? {
                    return Ok(Some(entry));
                }
            }
            _ => {}
        }
    }
    Ok(None)
}

fn init_in_table<E: Entry>(
    elf: &ElfFile,
    entries: &[E],
    layout: &ImageLayout,
) -> Result<Option<usize>> {
    for sym in entries {
        if let Ok(name) = sym.get_name(elf) {
            if ENTRY_SYMBOL_NAMES.contains(&name) {
                return Ok(Some(symbol_address(elf, sym, layout)? as usize));
            }
        }
    }
    Ok(None)
}

/// Verifies all declared module dependencies are already live.
fn check_dependencies(dependencies: &[String]) -> Result<()> {
    if dependencies.is_empty() {
        return Ok(());
    }
    let active_modules = MODULE_TABLE.lock();
    for dep in dependencies {
        if !active_modules.contains_key(dep.as_str()) {
            ostd::error!("Missing module dependency: \"{}\"", dep);
            return_errno!(ENOENT, "missing module dependency");
        }
    }
    Ok(())
}

/// Parses `.modinfo` (null-terminated `key=value` records) and the legacy
/// `.mod.*` / `.modinfo.*` string sections into a single metadata struct.
#[derive(Default)]
struct Modinfo {
    name: String,
    version: String,
    description: String,
    author: String,
    depends: Vec<String>,
}

impl Modinfo {
    fn field_is_empty(&self, key: &str) -> bool {
        match key {
            "name" => self.name.is_empty(),
            "version" => self.version.is_empty(),
            "description" => self.description.is_empty(),
            "author" => self.author.is_empty(),
            // Unknown fields are never "filled"; `.mod.*` may carry extras.
            _ => true,
        }
    }

    fn set_field(&mut self, key: &str, value: &str) {
        let value = value.trim_matches('\0').trim();
        match key {
            "name" if self.name.is_empty() => self.name = value.to_owned(),
            "version" if self.version.is_empty() => self.version = value.to_owned(),
            "description" if self.description.is_empty() => self.description = value.to_owned(),
            "author" if self.author.is_empty() => self.author = value.to_owned(),
            "depends" => {
                for dep in value.split(',') {
                    let dep = dep.trim();
                    if !dep.is_empty() && !self.depends.iter().any(|d| d == dep) {
                        self.depends.push(dep.to_owned());
                    }
                }
            }
            _ => {}
        }
    }

    fn set_field_raw(&mut self, key: &str, raw: &[u8]) {
        if let Ok(value) = core::str::from_utf8(raw) {
            self.set_field(key, value);
        }
    }
}

fn parse_modinfo(elf: &ElfFile) -> Modinfo {
    let mut metadata = Modinfo::default();
    let section_count = elf.header.pt2.sh_count() as u16;
    for index in 1..section_count {
        let Some(section) = elf.section_header(index).ok() else {
            continue;
        };
        let section_name = section.get_name(elf).unwrap_or("");
        match section_name {
            ".modinfo" => {
                if let Ok(SectionData::Undefined(bytes)) = section.get_data(elf) {
                    for record in bytes.split(|&b| b == 0) {
                        let Ok(record) = core::str::from_utf8(record) else {
                            continue;
                        };
                        if let Some((key, value)) = record.split_once('=') {
                            metadata.set_field(key.trim(), value);
                        }
                    }
                }
            }
            _ => {
                let Some(mut key) = section_name
                    .strip_prefix(".mod.")
                    .or_else(|| section_name.strip_prefix(".modinfo."))
                else {
                    continue;
                };
                // Legacy alias used by the old `.mod.desc` convention.
                if key == "desc" {
                    key = "description";
                }
                if !matches!(key, "name" | "version" | "description" | "author") {
                    continue;
                }
                if !metadata.field_is_empty(key) {
                    continue;
                }
                match section.get_data(elf) {
                    Ok(SectionData::Undefined(bytes)) | Ok(SectionData::StrArray(bytes)) => {
                        metadata.set_field_raw(key, bytes)
                    }
                    _ => {}
                }
            }
        }
    }
    metadata
}

#[cfg(ktest)]
mod tests {
    use super::*;
    use crate::api::errno::Errno;
    use crate::modules::registry::{MODULE_IMAGES, register_loaded_module};
    use crate::modules::{ModuleState, list_modules, register_symbol, unload_module};
    use ostd::prelude::ktest;

    // Relocation type constants, mirrored locally to build fixture ELFs.
    const R_X86_64_64: u32 = 1;
    const R_X86_64_RELATIVE: u32 = 8;

    /// Writes a minimal 64-bit little-endian x86_64 ELF header.
    fn make_ehdr(
        buf: &mut [u8],
        etype: u16,
        entry: u64,
        phoff: u64,
        phnum: u16,
        shoff: u64,
        shnum: u16,
        shstrndx: u16,
    ) {
        buf[0..4].copy_from_slice(b"\x7fELF");
        buf[4] = 2; // ELFCLASS64
        buf[5] = 1; // ELFDATA2LSB
        buf[6] = 1; // EV_CURRENT
        buf[16..18].copy_from_slice(&etype.to_le_bytes());
        buf[18..20].copy_from_slice(&62u16.to_le_bytes()); // EM_X86_64
        buf[20..24].copy_from_slice(&1u32.to_le_bytes());
        buf[24..32].copy_from_slice(&entry.to_le_bytes());
        buf[32..40].copy_from_slice(&phoff.to_le_bytes());
        buf[40..48].copy_from_slice(&shoff.to_le_bytes());
        buf[52..54].copy_from_slice(&64u16.to_le_bytes()); // e_ehsize
        buf[54..56].copy_from_slice(&56u16.to_le_bytes()); // e_phentsize
        buf[56..58].copy_from_slice(&phnum.to_le_bytes());
        buf[58..60].copy_from_slice(&64u16.to_le_bytes()); // e_shentsize
        buf[60..62].copy_from_slice(&shnum.to_le_bytes());
        buf[62..64].copy_from_slice(&shstrndx.to_le_bytes());
    }

    fn make_phdr_load(buf: &mut [u8], offset: u64, vaddr: u64, filesz: u64, memsz: u64) {
        buf[0..4].copy_from_slice(&1u32.to_le_bytes()); // PT_LOAD
        buf[4..8].copy_from_slice(&5u32.to_le_bytes()); // R|X
        buf[8..16].copy_from_slice(&offset.to_le_bytes());
        buf[16..24].copy_from_slice(&vaddr.to_le_bytes());
        buf[32..40].copy_from_slice(&filesz.to_le_bytes());
        buf[40..48].copy_from_slice(&memsz.to_le_bytes());
        buf[48..56].copy_from_slice(&4096u64.to_le_bytes());
    }

    #[allow(clippy::too_many_arguments)]
    fn make_shdr(
        buf: &mut [u8],
        name: u32,
        sh_type: u32,
        flags: u64,
        offset: u64,
        size: u64,
        link: u32,
        info: u32,
        align: u64,
        entsize: u64,
    ) {
        buf[0..4].copy_from_slice(&name.to_le_bytes());
        buf[4..8].copy_from_slice(&sh_type.to_le_bytes());
        buf[8..16].copy_from_slice(&flags.to_le_bytes());
        buf[24..32].copy_from_slice(&offset.to_le_bytes());
        buf[32..40].copy_from_slice(&size.to_le_bytes());
        buf[40..44].copy_from_slice(&link.to_le_bytes());
        buf[44..48].copy_from_slice(&info.to_le_bytes());
        buf[48..56].copy_from_slice(&align.to_le_bytes());
        buf[56..64].copy_from_slice(&entsize.to_le_bytes());
    }

    fn make_sym(buf: &mut [u8], name: u32, bind_type: u8, shndx: u16, value: u64, size: u64) {
        buf[0..4].copy_from_slice(&name.to_le_bytes());
        buf[4] = bind_type;
        buf[6..8].copy_from_slice(&shndx.to_le_bytes());
        buf[8..16].copy_from_slice(&value.to_le_bytes());
        buf[16..24].copy_from_slice(&size.to_le_bytes());
    }

    fn make_rela(buf: &mut [u8], offset: u64, sym_idx: u32, rtype: u32, addend: u64) {
        buf[0..8].copy_from_slice(&offset.to_le_bytes());
        buf[8..16].copy_from_slice(&(((sym_idx as u64) << 32) | rtype as u64).to_le_bytes());
        buf[16..24].copy_from_slice(&addend.to_le_bytes());
    }

    fn find_module(name: &str) -> ModuleInfo {
        list_modules()
            .into_iter()
            .find(|m| m.name == name)
            .expect("loaded module registered")
    }

    fn test_module_info(name: &str) -> ModuleInfo {
        ModuleInfo {
            name: name.to_owned(),
            version: String::new(),
            description: String::new(),
            author: String::new(),
            load_base: 0,
            load_size: 0,
            entry_point: 0,
            state: ModuleState::Live,
            dependencies: Vec::new(),
            params: Vec::new(),
        }
    }

    #[ktest]
    fn invalid_elf_module_rejected() {
        let bad = [0u8; 32];
        assert!(load_module(&bad, "").is_err());
    }

    #[ktest]
    fn dyn_module_loads_with_relative_relocation() {
        // 64B header | 56B PHDR @64 | text @128 | RELA @136 | SHDRs @160
        let mut hdr = [0u8; 288];
        make_ehdr(&mut hdr, 3 /*ET_DYN*/, 0x1000, 64, 1, 160, 2, 0);
        make_phdr_load(&mut hdr[64..120], 0, 0x1000, 288, 288);
        // RELATIVE relocation at module vaddr 0x1000, addend 0x1010
        make_rela(&mut hdr[136..160], 0x1000, 0, R_X86_64_RELATIVE, 0x1010);
        make_shdr(&mut hdr[160..224], 0, 0, 0, 0, 0, 0, 0, 0, 0); // NULL
        make_shdr(
            &mut hdr[224..288],
            0,
            4, /*RELA*/
            0,
            136,
            24,
            0,
            0,
            8,
            24,
        );

        let mod_name = load_module(&hdr, "debug=1").expect("load ET_DYN module");
        let info = find_module(&mod_name);
        let base = info.load_base;
        assert_ne!(base, 0);
        assert_eq!(info.entry_point, base);
        assert_eq!(info.state, ModuleState::Live);
        assert_eq!(
            info.params,
            alloc::vec![("debug".to_owned(), "1".to_owned())]
        );

        // value = load bias + addend = base - 0x1000 + 0x1010 = base + 0x10
        let image = MODULE_IMAGES
            .lock()
            .get(&mod_name)
            .cloned()
            .expect("image retained");
        assert_eq!(image[0..8], (base + 0x10).to_le_bytes());

        unload_module(&mod_name).expect("unload ET_DYN module");
        assert!(list_modules().iter().all(|m| m.name != mod_name));
    }

    #[ktest]
    fn rel_module_links_kernel_symbols() {
        // 64B EHDR | .text @64 | .symtab @72 | .strtab @168 | .rela.text @200
        // | .shstrtab @232 | 6 SHDRs @280
        const SHOFF: u64 = 280;
        let mut hdr = [0u8; 664];
        make_ehdr(&mut hdr, 1 /*ET_REL*/, 0, 0, 0, SHOFF, 6, 5);
        hdr[64..72].fill(0x90);
        // .symtab: [0]=NULL, [1]=local .text fn, [2]=undef global, [3]=init
        make_sym(&mut hdr[72..96], 0, 0, 0, 0, 0);
        make_sym(&mut hdr[96..120], 1, 0x12, 1, 0, 8);
        make_sym(&mut hdr[120..144], 1, 0x12, 0, 0, 0); // "petra_test_helper"
        make_sym(&mut hdr[144..168], 19, 0x12, 1, 0, 0); // "init_module"
        hdr[168..168 + 31].copy_from_slice(b"\0petra_test_helper\0init_module\0");
        // R_X86_64_64 against symbol #2, placed at offset 0 of .text
        make_rela(&mut hdr[200..224], 0, 2, R_X86_64_64, 0);
        hdr[232..232 + 44].copy_from_slice(b"\0.text\0.symtab\0.strtab\0.rela.text\0.shstrtab\0");

        let shdrs = &mut hdr[SHOFF as usize..];
        for entry in shdrs.chunks_mut(64) {
            entry.fill(0);
        }
        make_shdr(&mut shdrs[64..128], 1, 1, 6, 64, 8, 0, 0, 4, 0); // .text
        make_shdr(&mut shdrs[128..192], 7, 2, 0, 72, 96, 3, 2, 8, 24); // .symtab
        make_shdr(&mut shdrs[192..256], 15, 3, 0, 168, 31, 0, 0, 1, 0); // .strtab
        make_shdr(&mut shdrs[256..320], 23, 4, 0x40, 200, 24, 2, 1, 8, 24); // .rela.text
        make_shdr(&mut shdrs[320..384], 34, 3, 0, 232, 44, 0, 0, 1, 0); // .shstrtab

        const HELPER_ADDR: u64 = 0x1234_5678_9abc;
        register_symbol("petra_test_helper", HELPER_ADDR as usize);

        let mod_name = load_module(&hdr, "verbose=2,flag").expect("load ET_REL module");
        let info = find_module(&mod_name);
        let base = info.load_base;
        assert_eq!(info.entry_point, base); // init_module at .text offset 0
        assert_eq!(
            info.params,
            alloc::vec![
                ("verbose".to_owned(), "2".to_owned()),
                ("flag".to_owned(), String::new()),
            ]
        );

        let image = MODULE_IMAGES
            .lock()
            .get(&mod_name)
            .cloned()
            .expect("image retained");
        assert_eq!(image[0..8], HELPER_ADDR.to_le_bytes());

        // Unresolved external symbol must fail the load. Keep the fixture's
        // layout intact and point the undefined reference at a symbol that
        // is not exported by the kernel ("init_module" stays a valid init
        // symbol because symbol #3 is defined locally).
        let mut bad = hdr;
        make_sym(&mut bad[120..144], 19, 0x12, 0, 0, 0);
        assert!(load_module(&bad, "").is_err());

        unload_module(&mod_name).expect("unload ET_REL module");
    }

    #[ktest]
    fn duplicate_module_name_rejected() {
        register_loaded_module(test_module_info("dup_test"), alloc::boxed::Box::new([]))
            .expect("first registration");
        let second =
            register_loaded_module(test_module_info("dup_test"), alloc::boxed::Box::new([]));
        assert!(matches!(second, Err(Errno::EEXIST)));
        unload_module("dup_test").expect("cleanup");
    }
}
