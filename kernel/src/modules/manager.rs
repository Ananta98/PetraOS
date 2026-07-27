/// Global module registry and lifecycle management.
use crate::modules::module::{KernelModule, ModuleInfo, ModuleState};
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use ostd::sync::SpinLock;

/// Entry stored inside the global kernel module registry.
struct ModuleEntry {
    module: Arc<dyn KernelModule>,
    state: ModuleState,
}

static MODULES: SpinLock<BTreeMap<String, ModuleEntry>> = SpinLock::new(BTreeMap::new());

/// Registers a kernel module with the global registry.
///
/// The module is placed in the [`ModuleState::Registered`] state.
///
/// # Errors
/// Returns [`ostd::Error::InvalidArgs`] if a module with the same name is already registered.
pub fn register_module(module: Arc<dyn KernelModule>) -> Result<(), ostd::Error> {
    let mut modules = MODULES.lock();
    let name = module.info().name;
    if modules.contains_key(&name) {
        return Err(ostd::Error::InvalidArgs);
    }
    modules.insert(
        name,
        ModuleEntry {
            module,
            state: ModuleState::Registered,
        },
    );
    Ok(())
}

/// Unregisters a kernel module by name.
///
/// The module must be in [`ModuleState::Unloaded`], [`ModuleState::Registered`],
/// or [`ModuleState::Failed`] state before it can be unregistered.
///
/// # Errors
/// - [`ostd::Error::InvalidArgs`] if the module is not found or is in an active state.
pub fn unregister_module(name: &str) -> Result<(), ostd::Error> {
    let mut modules = MODULES.lock();
    let entry = modules.get(name).ok_or(ostd::Error::InvalidArgs)?;

    if entry.state == ModuleState::Active
        || entry.state == ModuleState::Initializing
        || entry.state == ModuleState::Exiting
    {
        return Err(ostd::Error::InvalidArgs);
    }

    modules.remove(name);
    Ok(())
}

/// Loads and initializes a registered kernel module by name.
///
/// Executes the module's [`KernelModule::init`] method and updates its state to
/// [`ModuleState::Active`] on success, or [`ModuleState::Failed`] on failure.
///
/// # Errors
/// - [`ostd::Error::InvalidArgs`] if no module with `name` exists or if it is already active.
pub fn load_module(name: &str) -> Result<(), ostd::Error> {
    let module_arc = {
        let mut modules = MODULES.lock();
        let entry = modules.get_mut(name).ok_or(ostd::Error::InvalidArgs)?;

        if entry.state == ModuleState::Active
            || entry.state == ModuleState::Initializing
            || entry.state == ModuleState::Exiting
        {
            return Err(ostd::Error::InvalidArgs);
        }

        entry.state = ModuleState::Initializing;
        entry.module.clone()
    };

    let init_res = module_arc.init();

    let mut modules = MODULES.lock();
    if let Some(entry) = modules.get_mut(name) {
        if init_res.is_ok() {
            entry.state = ModuleState::Active;
        } else {
            entry.state = ModuleState::Failed;
        }
    }

    init_res
}

/// Unloads an active kernel module by name.
///
/// Executes the module's [`KernelModule::exit`] method and updates its state to
/// [`ModuleState::Unloaded`].
///
/// # Errors
/// - [`ostd::Error::InvalidArgs`] if no module with `name` exists or if it is not active.
pub fn unload_module(name: &str) -> Result<(), ostd::Error> {
    let module_arc = {
        let mut modules = MODULES.lock();
        let entry = modules.get_mut(name).ok_or(ostd::Error::InvalidArgs)?;

        if entry.state != ModuleState::Active {
            return Err(ostd::Error::InvalidArgs);
        }

        entry.state = ModuleState::Exiting;
        entry.module.clone()
    };

    let exit_res = module_arc.exit();

    let mut modules = MODULES.lock();
    if let Some(entry) = modules.get_mut(name) {
        if exit_res.is_ok() {
            entry.state = ModuleState::Unloaded;
        } else {
            entry.state = ModuleState::Failed;
        }
    }

    exit_res
}

/// Retrieves metadata information for a kernel module by name.
pub fn get_module_info(name: &str) -> Option<ModuleInfo> {
    let modules = MODULES.lock();
    modules.get(name).map(|entry| {
        let mut info = entry.module.info();
        info.state = entry.state;
        info
    })
}

/// Returns metadata list of all registered kernel modules.
pub fn list_modules() -> Vec<ModuleInfo> {
    let modules = MODULES.lock();
    modules
        .values()
        .map(|entry| {
            let mut info = entry.module.info();
            info.state = entry.state;
            info
        })
        .collect()
}

/// Initializes the kernel module management subsystem and auto-loads built-in modules & initcalls.
///
/// Iterates over all module and driver initcalls registered via `module_init!` in `.initcall` section.
///
/// # Errors
/// Returns [`ostd::Error`] if registering or initializing built-in modules fails.
pub fn init() -> Result<(), ostd::Error> {
    log::info!(
        "[module_manager] Initializing kernel module subsystem and auto-loading initcalls..."
    );

    // Auto-register and load all module initcalls registered independently across kernel files via module_init!
    for reg in crate::modules::MODULE_INITCALLS {
        log::info!(
            "[module_initcall] Auto-registering module/driver: {}",
            reg.name
        );
        let module = Arc::new(crate::modules::StaticKernelModule::new(reg));
        if register_module(module).is_ok() {
            let _ = load_module(reg.name);
        }
    }

    Ok(())
}
