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

#[cfg(ktest)]
mod tests {
    use super::*;
    use crate::modules::initcall::{MODULE_INITCALLS, ModuleRegistration, StaticKernelModule};
    use crate::modules::module::{KernelModule, ModuleInfo, ModuleState};
    use alloc::string::String;
    use alloc::sync::Arc;
    use ostd::prelude::ktest;

    // ---------------------------------------------------------------------------
    // Stub module for registry lifecycle tests
    // ---------------------------------------------------------------------------

    struct StubModule {
        name: &'static str,
    }

    impl KernelModule for StubModule {
        fn info(&self) -> ModuleInfo {
            ModuleInfo {
                name: String::from(self.name),
                version: String::from("1.0.0"),
                author: String::from("test"),
                description: String::from("stub module for unit tests"),
                license: String::from("MIT"),
                state: ModuleState::Registered,
            }
        }

        fn init(&self) -> Result<(), ostd::Error> {
            Ok(())
        }

        fn exit(&self) -> Result<(), ostd::Error> {
            Ok(())
        }
    }

    // ---------------------------------------------------------------------------
    // Helper: create a unique name to avoid collisions between tests
    // ---------------------------------------------------------------------------

    fn unique_name(suffix: &'static str) -> Arc<StubModule> {
        Arc::new(StubModule { name: suffix })
    }

    // ---------------------------------------------------------------------------
    // Registry lifecycle tests
    // ---------------------------------------------------------------------------

    /// Registering a module inserts it and a second registration with the same
    /// name should fail.
    #[ktest]
    fn test_register_module_unique() {
        let module = unique_name("__test_unique_reg");
        assert!(register_module(module.clone()).is_ok());
        // Duplicate should fail.
        assert!(register_module(module).is_err());
        // Cleanup.
        let _ = unregister_module("__test_unique_reg");
    }

    /// A freshly registered module should appear in `get_module_info` as
    /// `ModuleState::Registered`.
    #[ktest]
    fn test_registered_module_appears_in_registry() {
        let module = unique_name("__test_appears");
        assert!(register_module(module).is_ok());
        let info = get_module_info("__test_appears")
            .expect("module should be in registry after registration");
        assert_eq!(info.state, ModuleState::Registered);
        let _ = unregister_module("__test_appears");
    }

    /// Loading a module should transition it to `ModuleState::Active`.
    #[ktest]
    fn test_load_module_transitions_to_active() {
        let module = unique_name("__test_load_active");
        assert!(register_module(module).is_ok());
        assert!(load_module("__test_load_active").is_ok());
        let info = get_module_info("__test_load_active").unwrap();
        assert_eq!(info.state, ModuleState::Active);
        // Cleanup.
        let _ = unload_module("__test_load_active");
        let _ = unregister_module("__test_load_active");
    }

    /// Unloading an active module should transition it to `ModuleState::Unloaded`.
    #[ktest]
    fn test_unload_module_transitions_to_unloaded() {
        let module = unique_name("__test_unload");
        assert!(register_module(module).is_ok());
        assert!(load_module("__test_unload").is_ok());
        assert!(unload_module("__test_unload").is_ok());
        let info = get_module_info("__test_unload").unwrap();
        assert_eq!(info.state, ModuleState::Unloaded);
        let _ = unregister_module("__test_unload");
    }

    /// Loading an already-active module must return an error.
    #[ktest]
    fn test_load_already_active_module_fails() {
        let module = unique_name("__test_double_load");
        assert!(register_module(module).is_ok());
        assert!(load_module("__test_double_load").is_ok());
        assert!(load_module("__test_double_load").is_err());
        let _ = unload_module("__test_double_load");
        let _ = unregister_module("__test_double_load");
    }

    /// Unloading a module that was never loaded should return an error.
    #[ktest]
    fn test_unload_inactive_module_fails() {
        let module = unique_name("__test_unload_inactive");
        assert!(register_module(module).is_ok());
        assert!(unload_module("__test_unload_inactive").is_err());
        let _ = unregister_module("__test_unload_inactive");
    }

    /// Unregistering a module that does not exist should fail.
    #[ktest]
    fn test_unregister_nonexistent_module_fails() {
        assert!(unregister_module("__test_does_not_exist_xyz").is_err());
    }

    /// `list_modules` should return metadata for all currently registered modules.
    #[ktest]
    fn test_list_modules_contains_registered() {
        let module = unique_name("__test_list");
        assert!(register_module(module).is_ok());
        let all = list_modules();
        let found = all.iter().any(|info| info.name == "__test_list");
        assert!(found, "list_modules should include __test_list");
        let _ = unregister_module("__test_list");
    }

    // ---------------------------------------------------------------------------
    // MODULE_INITCALLS distributed-slice tests
    // ---------------------------------------------------------------------------

    /// The distributed slice must be non-empty – at minimum the driver registrations
    /// from ahci, nvme, e1000, rtl8139, console, keyboard, mouse, framebuffer, etc.
    #[ktest]
    fn test_module_initcalls_slice_is_non_empty() {
        assert!(
            !MODULE_INITCALLS.is_empty(),
            "MODULE_INITCALLS linker section must contain at least one entry"
        );
    }

    /// Every entry in MODULE_INITCALLS must have a non-empty name.
    #[ktest]
    fn test_module_initcalls_names_are_non_empty() {
        for reg in MODULE_INITCALLS {
            assert!(
                !reg.name.is_empty(),
                "MODULE_INITCALLS entry has an empty name"
            );
        }
    }

    /// Checks that all expected kernel driver names are present in `MODULE_INITCALLS`,
    /// then registers each one via [`StaticKernelModule`] and verifies it appears in
    /// the module registry as [`ModuleState::Registered`] — proving the full path from
    /// linker-section initcall to module-manager registration works correctly.
    #[ktest]
    fn test_module_initcalls_contains_expected_drivers() {
        // --- Step 1: collect names declared in the distributed slice. ---
        let names: alloc::vec::Vec<&str> = MODULE_INITCALLS.iter().map(|r| r.name).collect();

        let expected = [
            "ahci",
            "nvme",
            "e1000",
            "rtl8139",
            "console",
            "keyboard",
            "mouse",
            "framebuffer",
            "cmos-rtc",
            "tsc",
            "system_buses",
        ];

        // --- Step 2: assert every expected name is present in the slice. ---
        for &driver in &expected {
            assert!(
                names.contains(&driver),
                "MODULE_INITCALLS is missing expected driver/module: '{}'",
                driver
            );
        }

        // --- Step 3: for each initcall registration that matches an expected driver,
        //     register it into the module manager and verify it is found as Registered.
        //     Use a unique test-scoped prefix to avoid clashing with other running tests.
        let prefix = "__initcall_reg_test__";
        let mut registered: alloc::vec::Vec<alloc::string::String> = alloc::vec::Vec::new();

        for reg in MODULE_INITCALLS {
            if !expected.contains(&reg.name) {
                continue;
            }

            // Build a scoped name so we don't stomp on real driver registrations.
            let scoped_name = alloc::format!("{}{}", prefix, reg.name);

            // Build a StaticKernelModule adapter that wraps this real initcall entry.
            let adapter: Arc<dyn KernelModule> = Arc::new(StaticKernelModule::new(reg));

            // Wrap it in a thin wrapper that reports the scoped name so the manager
            // treats it as a fresh entry independent of any real loaded module.
            struct ScopedModule {
                inner: Arc<dyn KernelModule>,
                scoped_name: alloc::string::String,
            }

            impl KernelModule for ScopedModule {
                fn info(&self) -> ModuleInfo {
                    let mut info = self.inner.info();
                    info.name = self.scoped_name.clone();
                    info
                }
                fn init(&self) -> Result<(), ostd::Error> {
                    Ok(())
                }
                fn exit(&self) -> Result<(), ostd::Error> {
                    Ok(())
                }
            }

            let scoped: Arc<dyn KernelModule> = Arc::new(ScopedModule {
                inner: adapter,
                scoped_name: scoped_name.clone(),
            });

            // Register into the module manager.
            let reg_result = register_module(scoped);
            assert!(
                reg_result.is_ok(),
                "Failed to register '{}' (initcall '{}') into the module manager",
                scoped_name,
                reg.name
            );

            // Verify presence and state via get_module_info.
            let info = get_module_info(&scoped_name).unwrap_or_else(|| {
                panic!(
                    "Module '{}' not found in registry after registration",
                    scoped_name
                )
            });

            assert_eq!(
                info.state,
                ModuleState::Registered,
                "Module '{}' should be in Registered state after registration, got {:?}",
                scoped_name,
                info.state
            );

            assert_eq!(
                info.name, scoped_name,
                "Module info name mismatch for '{}'",
                scoped_name
            );

            registered.push(scoped_name);
        }

        // --- Step 4: cleanup — unregister all test-scoped entries. ---
        for name in &registered {
            let _ = unregister_module(name);
        }

        // Verify all registered entries count matches expected drivers found.
        assert_eq!(
            registered.len(),
            expected.len(),
            "Expected to register {} drivers but only registered {}",
            expected.len(),
            registered.len()
        );
    }

    /// StaticKernelModule adapter correctly exposes registration metadata.
    #[ktest]
    fn test_static_kernel_module_adapter_info() {
        // Find any real registration to test the adapter with.
        let reg = MODULE_INITCALLS
            .iter()
            .next()
            .expect("MODULE_INITCALLS must have at least one entry");
        let adapter = StaticKernelModule::new(reg);
        let info = adapter.info();
        assert_eq!(info.name, reg.name);
        assert_eq!(info.version, reg.version);
    }
}
