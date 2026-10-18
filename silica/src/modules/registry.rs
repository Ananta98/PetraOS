// SPDX-License-Identifier: GPL-2.0

//! Kernel module registry: lifecycle state, metadata and compiled-in modules.

use alloc::{
    borrow::ToOwned,
    boxed::Box,
    collections::btree_map::BTreeMap,
    string::String,
    sync::Arc,
    vec::Vec,
};
use spin::Mutex;

use crate::api::errno::{Errno, Result};

use super::initcall::{register_initcall, InitcallFn, InitcallLevel};

/// Lifecycle state of a kernel module.
///
/// `MODULE_TABLE` only ever contains live modules; transient states never
/// survive a function boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleState {
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
    /// Kernel virtual address of the relocated module image.
    pub load_base: usize,
    /// Size in bytes of the loaded (relocated) module image.
    pub load_size: usize,
    /// Address of the module's init function, or `0` if unknown.
    pub entry_point: usize,
    pub state: ModuleState,
    pub dependencies: Vec<String>,
    /// Linux-style module parameters (`key=value` pairs) given at load time.
    pub params: Vec<(String, String)>,
}

/// Global registry of active kernel modules.
pub(crate) static MODULE_TABLE: Mutex<BTreeMap<String, ModuleInfo>> = Mutex::new(BTreeMap::new());

/// Relocated ELF images backing every live module, keyed by module name.
///
/// Kept separate from [`MODULE_TABLE`] so cheap metadata queries
/// ([`list_modules`]) never clone the actual module bytes.
///
/// Locking convention: whenever both registries are needed, lock
/// `MODULE_TABLE` first, then `MODULE_IMAGES` (the mutexes are non-reentrant).
pub(crate) static MODULE_IMAGES: Mutex<BTreeMap<String, Box<[u8]>>> =
    Mutex::new(BTreeMap::new());

/// Global registry of compiled-in kernel modules.
static COMPILED_MODULES: Mutex<BTreeMap<String, Arc<dyn KernelModule>>> =
    Mutex::new(BTreeMap::new());

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

    register_loaded_module(
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
            params: Vec::new(),
        },
        Box::new([]),
    )
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

/// Registers a freshly loaded module image and its metadata.
///
/// Fails with `EEXIST` if a live module already carries the same name.
pub(crate) fn register_loaded_module(info: ModuleInfo, image: Box<[u8]>) -> Result<()> {
    let mut table = MODULE_TABLE.lock();
    if table.contains_key(&info.name) {
        return Err(Errno::EEXIST);
    }
    let name = info.name.clone();
    MODULE_IMAGES.lock().insert(name.clone(), image);
    table.insert(name, info);
    Ok(())
}

/// Renames a live module (used when inferring the module name from the file
/// stem). No-op if no module matches `old_name`.
pub(crate) fn rename_module(old_name: &str, new_name: &str) -> Result<()> {
    let mut table = MODULE_TABLE.lock();
    let Some(mut info) = table.remove(old_name) else {
        return Ok(());
    };
    info.name = new_name.to_owned();

    let image = MODULE_IMAGES.lock().remove(old_name);
    table.insert(new_name.to_owned(), info);
    if let Some(image) = image {
        MODULE_IMAGES.lock().insert(new_name.to_owned(), image);
    }
    Ok(())
}

/// Unloads a kernel module by name.
///
/// The `exit` hook runs before the module entry is dropped so that cleanup
/// can still observe its own registration; a failed cleanup restores the
/// entry in `MODULE_TABLE`.
pub fn unload_module(name: &str) -> Result<()> {
    let mut info = {
        let mut table = MODULE_TABLE.lock();
        table.remove(name).ok_or(Errno::ENOENT)?
    };

    info.state = ModuleState::Unloading;

    // If compiled module exists, call its exit hook
    let compiled = {
        let table = COMPILED_MODULES.lock();
        table.get(name).cloned()
    };

    if let Some(m) = compiled {
        if let Err(err) = m.exit() {
            info.state = ModuleState::Live;
            MODULE_TABLE.lock().insert(name.to_owned(), info);
            return Err(err);
        }
    }

    MODULE_IMAGES.lock().remove(name);
    ostd::info!("Unloaded kernel module \"{}\"", name);
    Ok(())
}

/// Query list of all currently loaded modules.
pub fn list_modules() -> Vec<ModuleInfo> {
    let table = MODULE_TABLE.lock();
    table.values().cloned().collect()
}
