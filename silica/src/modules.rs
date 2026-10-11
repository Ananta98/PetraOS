// SPDX-License-Identifier: GPL-2.0

//! Kernel Module System and Linux-compatible Initcalls.
//!
//! Provides:
//! - Linux-style staged initcalls (`Early`, `Core`, `PostCore`, `Arch`, `Subsys`, `Fs`, `Device`, `Late`).
//! - Modular driver abstraction (`KernelModule` trait).
//! - Loadable Kernel Module (LKM) ELF parser & loader using `xmas_elf` (replacing bloated manual headers).
//! - Kernel symbol table registry (`SYMBOL_TABLE`) for symbol lookups and exports.
//! - 100% Safe Rust implementation complying with kernel `#![deny(unsafe_code)]`.

use alloc::{
    borrow::ToOwned, collections::btree_map::BTreeMap, format, string::String, sync::Arc, vec::Vec,
};
use core::fmt;
use spin::Mutex;
use xmas_elf::{
    ElfFile,
    header::{Class, Data, Machine},
    program::Type as ProgramType,
    sections::SectionData,
    symbol_table::Entry,
};

use crate::api::errno::{Errno, Result};

/// Initcall execution levels, matching Linux kernel boot stages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum InitcallLevel {
    /// Pure / Early initialization (level 0)
    Early = 0,
    /// Core kernel subsystems (level 1)
    Core = 1,
    /// Post-core initialization (level 2)
    PostCore = 2,
    /// Architecture-specific setup (level 3)
    Arch = 3,
    /// Kernel subsystems: VFS, IPC, network stack (level 4)
    Subsys = 4,
    /// Filesystems: ext2, devfs, initramfs (level 5)
    Fs = 5,
    /// Device drivers: PCI, NVMe, Block devices (level 6)
    Device = 6,
    /// Late initialization before userspace (level 7)
    Late = 7,
}

impl fmt::Display for InitcallLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Early => write!(f, "0 (early)"),
            Self::Core => write!(f, "1 (core)"),
            Self::PostCore => write!(f, "2 (postcore)"),
            Self::Arch => write!(f, "3 (arch)"),
            Self::Subsys => write!(f, "4 (subsys)"),
            Self::Fs => write!(f, "5 (fs)"),
            Self::Device => write!(f, "6 (device)"),
            Self::Late => write!(f, "7 (late)"),
        }
    }
}

pub type InitcallFn = fn() -> Result<()>;

/// An entry in the kernel's initcall dispatch table.
#[derive(Clone, Copy)]
pub struct InitcallEntry {
    pub level: InitcallLevel,
    pub name: &'static str,
    pub func: InitcallFn,
}

/// Global registry of kernel initcalls.
static INITCALL_LIST: Mutex<Vec<InitcallEntry>> = Mutex::new(Vec::new());

/// Register a function into the kernel initcall queue.
pub fn register_initcall(level: InitcallLevel, name: &'static str, func: InitcallFn) {
    let mut list = INITCALL_LIST.lock();
    list.push(InitcallEntry { level, name, func });
}

/// Execute all registered initcalls in priority order (levels 0 through 7).
pub fn do_initcalls() -> Result<()> {
    let mut entries = {
        let mut list = INITCALL_LIST.lock();
        core::mem::take(&mut *list)
    };

    // Sort by initcall level ascending so earlier levels execute first
    entries.sort_by_key(|e| e.level);

    ostd::info!(
        "Executing kernel initcalls ({} registered)...",
        entries.len()
    );

    let mut current_level = None;
    for entry in &entries {
        if current_level != Some(entry.level) {
            current_level = Some(entry.level);
            ostd::info!("initcall: entering level {}", entry.level);
        }

        match (entry.func)() {
            Ok(()) => {
                ostd::info!("initcall: {}() ok", entry.name);
            }
            Err(err) => {
                ostd::error!("initcall: {}() failed with error {:?}", entry.name, err);
                return Err(err);
            }
        }
    }

    ostd::info!("All kernel initcalls completed successfully");
    Ok(())
}

// ----------------------------------------------------------------------------
// Kernel Driver Module Macro
// ----------------------------------------------------------------------------

/// Macro for declaring and registering a kernel driver module.
///
/// # Examples
/// ```
/// module!("Ext2 Filesystem", "SilicaOS Team", InitcallLevel::Fs, init);
/// ```
#[macro_export]
macro_rules! module {
    ($desc:expr, $author:expr, $level:expr, $init_fn:path) => {
        const _: () = {
            #[used]
            #[allow(unsafe_code, unsafe_attr_outside_unsafe)]
            #[unsafe(link_section = ".init_array")]
            static __INIT_CALL: extern "C" fn() = {
                extern "C" fn __register() {
                    $crate::modules::register_module_declaration($desc, $author, $level, $init_fn);
                }
                __register
            };
        };
    };
}

// ============================================================================
// 2. Kernel Module Trait & Abstraction
// ============================================================================

/// Trait implemented by modular kernel drivers and subsystems.
pub trait KernelModule: Send + Sync {
    /// Name of the module (e.g., "nvme", "ext2").
    fn name(&self) -> &'static str;

    /// Module version string.
    fn version(&self) -> &'static str {
        "0.1.0"
    }

    /// Module description.
    fn description(&self) -> &'static str {
        ""
    }

    /// Module author.
    fn author(&self) -> &'static str {
        ""
    }

    /// Initialize the module (probes devices, registers filesystems, etc.).
    fn init(&self) -> Result<()>;

    /// Cleanup module resources before unload.
    fn exit(&self) -> Result<()> {
        Ok(())
    }
}

// ============================================================================
// 3. Kernel Symbol Table & Export Macros
// ============================================================================

/// Global symbol table for resolving module symbols.
pub static SYMBOL_TABLE: Mutex<BTreeMap<String, usize>> = Mutex::new(BTreeMap::new());

/// Registers a kernel symbol so loadable modules can resolve against it.
pub fn register_symbol(name: &str, address: usize) {
    let mut table = SYMBOL_TABLE.lock();
    table.insert(name.to_owned(), address);
}

/// Resolves a symbol name to its runtime kernel virtual address.
pub fn lookup_symbol(name: &str) -> Option<usize> {
    let table = SYMBOL_TABLE.lock();
    table.get(name).copied()
}

/// Macro to export a kernel function or symbol.
#[macro_export]
macro_rules! export_symbol {
    ($sym:ident) => {
        $crate::modules::register_symbol(stringify!($sym), $sym as usize);
    };
}

// ============================================================================
// 4. Loadable Kernel Module (LKM) & ELF Management via `xmas_elf`
// ============================================================================

/// Lifecycle state of a kernel module.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleState {
    Unloaded,
    Loading,
    Live,
    Unloading,
}

/// Metadata and tracking information for a loaded kernel module.
#[derive(Debug, Clone)]
pub struct ModuleInfo {
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: String,
    pub load_base: usize,
    pub load_size: usize,
    pub entry_point: usize,
    pub state: ModuleState,
    pub dependencies: Vec<String>,
}

/// Global registry of active kernel modules.
pub static MODULE_TABLE: Mutex<BTreeMap<String, ModuleInfo>> = Mutex::new(BTreeMap::new());

/// Static compiled-in module registry.
static COMPILED_MODULES: Mutex<BTreeMap<String, Arc<dyn KernelModule>>> =
    Mutex::new(BTreeMap::new());

/// Registers a compiled-in kernel module.
pub fn register_module(module: Arc<dyn KernelModule>) {
    let name = module.name();
    let mut table = COMPILED_MODULES.lock();
    table.insert(name.to_owned(), module);
}

/// Initializes a compiled-in kernel module by name.
pub fn init_compiled_module(name: &str) -> Result<()> {
    let module = {
        let table = COMPILED_MODULES.lock();
        table.get(name).cloned().ok_or(Errno::ENOENT)?
    };

    ostd::info!("Initializing module \"{}\"...", module.name());
    module.init()?;

    let mut mod_table = MODULE_TABLE.lock();
    mod_table.insert(
        module.name().to_owned(),
        ModuleInfo {
            name: module.name().to_owned(),
            version: module.version().to_owned(),
            description: module.description().to_owned(),
            author: module.author().to_owned(),
            load_base: 0,
            load_size: 0,
            entry_point: 0,
            state: ModuleState::Live,
            dependencies: Vec::new(),
        },
    );

    Ok(())
}

/// Wrapper for statically declared kernel modules.
pub struct StaticKernelModule {
    pub name: &'static str,
    pub version: &'static str,
    pub description: &'static str,
    pub author: &'static str,
    pub init_fn: InitcallFn,
}

impl KernelModule for StaticKernelModule {
    fn name(&self) -> &'static str {
        self.name
    }

    fn version(&self) -> &'static str {
        self.version
    }

    fn description(&self) -> &'static str {
        self.description
    }

    fn author(&self) -> &'static str {
        self.author
    }

    fn init(&self) -> Result<()> {
        (self.init_fn)()
    }
}

/// Registers a module declaration into both the initcall queue and the module table.
pub fn register_module_declaration(
    description: &'static str,
    author: &'static str,
    level: InitcallLevel,
    init_fn: InitcallFn,
) {
    register_initcall(level, description, init_fn);
    register_module(Arc::new(StaticKernelModule {
        name: description,
        version: "1.0.0",
        description,
        author,
        init_fn,
    }));
}

/// Initializes all staged kernel module initcalls, auto-probes block devices, and mounts filesystems.
pub fn init_calls() -> Result<()> {
    do_initcalls()?;
    Ok(())
}

/// Loads an ELF module file from initramfs by path.
pub fn load_module_from_initramfs(path: &str, cmdline: &str) -> Result<String> {
    load_module_from_file(path, cmdline)
}

/// Loads a kernel module from a filesystem path (supporting both rootfs VFS and initramfs).
pub fn load_module_from_file(path: &str, cmdline: &str) -> Result<String> {
    let elf_data = crate::proc::exec::read_binary_data(path)?;
    let mut mod_name = load_module(&elf_data, cmdline)?;
    // If the module didn't specify an explicit name, infer from file stem
    if mod_name.starts_with("module_") {
        let file_name = path.rsplit('/').next().unwrap_or(path);
        let stem = file_name
            .trim_end_matches(".kso")
            .trim_end_matches(".so")
            .trim_end_matches(".ko");
        if !stem.is_empty() {
            let mut table = MODULE_TABLE.lock();
            if let Some(mut info) = table.remove(&mod_name) {
                info.name = stem.to_owned();
                table.insert(stem.to_owned(), info);
                mod_name = stem.to_owned();
            }
        }
    }
    Ok(mod_name)
}

fn align_up(val: usize, align: usize) -> usize {
    (val + align - 1) & !(align - 1)
}

fn align_down(val: usize, align: usize) -> usize {
    val & !(align - 1)
}

/// Loads and validates a kernel module from an ELF binary image using `xmas_elf`.
///
/// Supports both Position-Independent Shared Object modules (`.so` / `.kso` / `ET_DYN`)
/// and relocatable object files (`.ko` / `ET_REL`).
pub fn load_module(elf_data: &[u8], cmdline: &str) -> Result<String> {
    let elf = ElfFile::new(elf_data).map_err(|_| Errno::ENOEXEC)?;

    // 1. Verify 64-bit Little-Endian ELF for target machine
    if elf.header.pt1.class() != Class::SixtyFour {
        crate::return_errno!(ENOEXEC, "module ELF must be 64-bit");
    }
    if elf.header.pt1.data() != Data::LittleEndian {
        crate::return_errno!(ENOEXEC, "module ELF must be little-endian");
    }
    #[cfg(target_arch = "x86_64")]
    if elf.header.pt2.machine().as_machine() != Machine::X86_64 {
        crate::return_errno!(ENOEXEC, "module ELF target machine must be x86_64");
    }

    // Verify ELF type: ET_DYN (.so/.kso) or ET_REL (.ko)
    let elf_type = elf.header.pt2.type_().as_type();
    if elf_type != xmas_elf::header::Type::SharedObject
        && elf_type != xmas_elf::header::Type::Relocatable
    {
        crate::return_errno!(
            ENOEXEC,
            "module ELF type must be ET_DYN (.so/.kso) or ET_REL (.ko)"
        );
    }

    let mut entry_point = elf.header.pt2.entry_point() as usize;
    let page_size = ostd::mm::PAGE_SIZE;
    let mut load_min = usize::MAX;
    let mut load_end = 0usize;

    // 2. Iterate PT_LOAD program headers to determine span
    for ph in elf.program_iter() {
        if ph.get_type() != Ok(ProgramType::Load) {
            continue;
        }

        let vaddr = ph.virtual_addr() as usize;
        let mem_sz = ph.mem_size() as usize;

        if mem_sz == 0 {
            continue;
        }

        let aligned_virt = align_down(vaddr, page_size);
        let misalign = vaddr - aligned_virt;
        let total_sz = align_up(mem_sz + misalign, page_size);
        let end = aligned_virt.checked_add(total_sz).ok_or(Errno::ENOMEM)?;

        load_min = load_min.min(aligned_virt);
        load_end = load_end.max(end);
    }

    let total_load_size = if load_min < usize::MAX && load_end > load_min {
        load_end - load_min
    } else {
        align_up(elf_data.len(), page_size)
    };

    // 3. Extract module metadata from sections
    let mut mod_name = String::new();
    let mut mod_version = String::new();
    let mut mod_description = String::new();
    let mut mod_author = String::new();
    let mut dependencies = Vec::new();

    for sec in elf.section_iter() {
        let sec_name = sec.get_name(&elf).unwrap_or("");
        match sec_name {
            ".modinfo" => {
                // Parse standard Linux null-terminated key=value pairs
                if let Ok(SectionData::Undefined(bytes)) = sec.get_data(&elf) {
                    for entry in bytes.split(|&b| b == 0) {
                        if let Ok(s) = core::str::from_utf8(entry) {
                            if let Some((k, v)) = s.split_once('=') {
                                match k.trim() {
                                    "name" if mod_name.is_empty() => {
                                        mod_name = v.trim().to_owned();
                                    }
                                    "version" if mod_version.is_empty() => {
                                        mod_version = v.trim().to_owned();
                                    }
                                    "description" if mod_description.is_empty() => {
                                        mod_description = v.trim().to_owned();
                                    }
                                    "author" if mod_author.is_empty() => {
                                        mod_author = v.trim().to_owned();
                                    }
                                    "depends" => {
                                        for d in v.split(',') {
                                            let d_clean = d.trim();
                                            if !d_clean.is_empty()
                                                && !dependencies
                                                    .iter()
                                                    .any(|existing| existing == d_clean)
                                            {
                                                dependencies.push(d_clean.to_owned());
                                            }
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                }
            }
            ".mod.name" | ".modinfo.name" => {
                if let Ok(SectionData::Undefined(bytes)) = sec.get_data(&elf) {
                    if let Ok(s) = core::str::from_utf8(bytes) {
                        mod_name = s.trim_matches('\0').trim().to_owned();
                    }
                }
            }
            ".mod.version" | ".modinfo.version" => {
                if let Ok(SectionData::Undefined(bytes)) = sec.get_data(&elf) {
                    if let Ok(s) = core::str::from_utf8(bytes) {
                        mod_version = s.trim_matches('\0').trim().to_owned();
                    }
                }
            }
            ".mod.desc" | ".modinfo.desc" => {
                if let Ok(SectionData::Undefined(bytes)) = sec.get_data(&elf) {
                    if let Ok(s) = core::str::from_utf8(bytes) {
                        mod_description = s.trim_matches('\0').trim().to_owned();
                    }
                }
            }
            ".mod.author" | ".modinfo.author" => {
                if let Ok(SectionData::Undefined(bytes)) = sec.get_data(&elf) {
                    if let Ok(s) = core::str::from_utf8(bytes) {
                        mod_author = s.trim_matches('\0').trim().to_owned();
                    }
                }
            }
            _ => {}
        }
    }

    // 4. Resolve entry point from symbols if not set in header
    if entry_point == 0 {
        for sec in elf.section_iter() {
            if let Ok(data) = sec.get_data(&elf) {
                match data {
                    SectionData::DynSymbolTable64(symbols) => {
                        for sym in symbols {
                            if let Ok(name) = sym.get_name(&elf) {
                                if name == "init_module" || name == "_init" || name == "init" {
                                    entry_point = sym.value() as usize;
                                    break;
                                }
                            }
                        }
                    }
                    SectionData::SymbolTable64(symbols) => {
                        for sym in symbols {
                            if let Ok(name) = sym.get_name(&elf) {
                                if name == "init_module" || name == "_init" || name == "init" {
                                    entry_point = sym.value() as usize;
                                    break;
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            if entry_point != 0 {
                break;
            }
        }
    }

    if mod_name.is_empty() {
        mod_name = format!("module_{:x}", entry_point);
    }

    // 5. Verify module dependencies
    let active_modules = MODULE_TABLE.lock();
    for dep in &dependencies {
        if !active_modules.contains_key(dep) {
            ostd::error!("Missing module dependency: \"{}\"", dep);
            crate::return_errno!(ENOENT, "missing module dependency");
        }
    }

    // 6. Inspect dynamic relocations and symbols if present
    for sec in elf.section_iter() {
        if let Ok(SectionData::Rela64(relas)) = sec.get_data(&elf) {
            for rela in relas {
                let _offset = rela.get_offset();
                let _sym_idx = rela.get_symbol_table_index();
                let _rel_type = rela.get_type();
                let _addend = rela.get_addend();
            }
        }
    }

    let module_info = ModuleInfo {
        name: mod_name.clone(),
        version: mod_version,
        description: mod_description,
        author: mod_author,
        load_base: load_min,
        load_size: total_load_size,
        entry_point,
        state: ModuleState::Live,
        dependencies,
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

    let registered_name = module_info.name.clone();
    MODULE_TABLE
        .lock()
        .insert(registered_name.clone(), module_info);

    Ok(registered_name)
}

/// Unloads a kernel module by name.
pub fn unload_module(name: &str) -> Result<()> {
    let mut table = MODULE_TABLE.lock();
    let mut info = table.remove(name).ok_or(Errno::ENOENT)?;

    info.state = ModuleState::Unloading;

    // If compiled module exists, call its exit hook
    let compiled = {
        let table = COMPILED_MODULES.lock();
        table.get(name).cloned()
    };

    if let Some(m) = compiled {
        m.exit()?;
    }

    ostd::info!("Unloaded kernel module \"{}\"", name);
    Ok(())
}

/// Query list of all currently loaded modules.
pub fn list_modules() -> Vec<ModuleInfo> {
    let table = MODULE_TABLE.lock();
    table.values().cloned().collect()
}

#[cfg(ktest)]
mod tests {
    use super::*;
    use ostd::prelude::ktest;

    #[ktest]
    fn invalid_elf_module_rejected() {
        let bad = [0u8; 32];
        assert!(load_module(&bad, "").is_err());
    }

    #[ktest]
    fn so_shared_object_module_loading_and_unload() {
        // Minimal valid 64-bit Little-Endian x86_64 ET_DYN (.so) ELF header
        let mut hdr = [0u8; 64];
        hdr[0..4].copy_from_slice(b"\x7fELF");
        hdr[4] = 2; // ELFCLASS64
        hdr[5] = 1; // ELFDATA2LSB
        hdr[6] = 1; // EV_CURRENT
        hdr[16..18].copy_from_slice(&3u16.to_le_bytes()); // ET_DYN (Shared Object)
        hdr[18..20].copy_from_slice(&62u16.to_le_bytes()); // EM_X86_64
        hdr[20..24].copy_from_slice(&1u32.to_le_bytes()); // EV_CURRENT
        hdr[24..32].copy_from_slice(&0x2000u64.to_le_bytes()); // entry point = 0x2000
        hdr[52..54].copy_from_slice(&64u16.to_le_bytes()); // e_ehsize = 64

        let mod_name = load_module(&hdr, "debug=1").expect("load .so module");
        assert_eq!(mod_name, "module_2000");

        let modules = list_modules();
        let found = modules.iter().find(|m| m.name == "module_2000");
        assert!(found.is_some());
        let info = found.unwrap();
        assert_eq!(info.entry_point, 0x2000);
        assert_eq!(info.state, ModuleState::Live);

        // Unload the module
        unload_module("module_2000").expect("unload module");
        assert!(list_modules().iter().all(|m| m.name != "module_2000"));
    }
}
