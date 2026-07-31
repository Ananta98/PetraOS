/// Linux C-style kernel module & driver auto-registration subsystem (`module_init`).
///
/// Uses distributed linker sections (`linkme`) to gather module and driver initcalls
/// at compile-time without requiring central hardcoded initialization lists.
use crate::modules::module::{KernelModule, ModuleInfo, ModuleState};
pub use linkme;

/// Holds metadata and function pointers for auto-registered kernel modules.
pub struct ModuleRegistration {
    /// Name of the kernel module or driver.
    pub name: &'static str,
    /// Author / maintainer of the module.
    pub author: &'static str,
    /// Human-readable description of the module.
    pub description: &'static str,
    /// Module version string.
    pub version: &'static str,
    /// Open source / proprietary license.
    pub license: &'static str,
    /// Initialization callback function.
    pub init_fn: fn() -> Result<(), ostd::Error>,
    /// Optional exit/cleanup callback function.
    pub exit_fn: Option<fn() -> Result<(), ostd::Error>>,
}

/// Distributed slice containing all module initcalls registered via `module_init!`.
#[allow(unsafe_code)]
#[linkme::distributed_slice]
pub static MODULE_INITCALLS: [ModuleRegistration];

/// Adapter struct exposing a [`ModuleRegistration`] static entry as a [`KernelModule`].
pub struct StaticKernelModule {
    reg: &'static ModuleRegistration,
}

impl StaticKernelModule {
    /// Create a new adapter for the given static registration entry.
    pub fn new(reg: &'static ModuleRegistration) -> Self {
        Self { reg }
    }
}

impl KernelModule for StaticKernelModule {
    fn info(&self) -> ModuleInfo {
        ModuleInfo {
            name: alloc::string::String::from(self.reg.name),
            version: alloc::string::String::from(self.reg.version),
            author: alloc::string::String::from(self.reg.author),
            description: alloc::string::String::from(self.reg.description),
            license: alloc::string::String::from(self.reg.license),
            state: ModuleState::Registered,
        }
    }

    fn init(&self) -> Result<(), ostd::Error> {
        (self.reg.init_fn)()
    }

    fn exit(&self) -> Result<(), ostd::Error> {
        if let Some(exit_fn) = self.reg.exit_fn {
            exit_fn()
        } else {
            Ok(())
        }
    }
}

/// Registers a kernel module initialization routine.
#[macro_export]
macro_rules! module_init {
    ($static_ident:ident, $init_fn:ident) => {
        #[allow(unsafe_code)]
        #[$crate::modules::initcall::linkme::distributed_slice(
            $crate::modules::initcall::MODULE_INITCALLS
        )]
        #[linkme(crate = $crate::modules::initcall::linkme)]
        static $static_ident: $crate::modules::initcall::ModuleRegistration =
            $crate::modules::initcall::ModuleRegistration {
                name: stringify!($init_fn),
                author: "PetraOS Core",
                description: "Built-in Kernel Module",
                version: "1.0.0",
                license: "BSD-2.0",
                init_fn: $init_fn,
                exit_fn: None,
            };
    };
    ($static_ident:ident, $name:expr, $init_fn:ident) => {
        #[allow(unsafe_code)]
        #[$crate::modules::initcall::linkme::distributed_slice(
            $crate::modules::initcall::MODULE_INITCALLS
        )]
        #[linkme(crate = $crate::modules::initcall::linkme)]
        static $static_ident: $crate::modules::initcall::ModuleRegistration =
            $crate::modules::initcall::ModuleRegistration {
                name: $name,
                author: "PetraOS Core",
                description: "Built-in Kernel Module",
                version: "1.0.0",
                license: "BSD-2.0",
                init_fn: $init_fn,
                exit_fn: None,
            };
    };
    ($static_ident:ident, $name:expr, $init_fn:ident, $exit_fn:ident) => {
        #[allow(unsafe_code)]
        #[$crate::modules::initcall::linkme::distributed_slice(
            $crate::modules::initcall::MODULE_INITCALLS
        )]
        #[linkme(crate = $crate::modules::initcall::linkme)]
        static $static_ident: $crate::modules::initcall::ModuleRegistration =
            $crate::modules::initcall::ModuleRegistration {
                name: $name,
                author: "PetraOS Core",
                description: "Built-in Kernel Module",
                version: "1.0.0",
                license: "BSD-2.0",
                init_fn: $init_fn,
                exit_fn: Some($exit_fn),
            };
    };
}

/// kernel helper alias for device initcall registration (`device_initcall`).
#[macro_export]
macro_rules! device_initcall {
    ($static_ident:ident, $init_fn:ident) => {
        $crate::module_init!($static_ident, $init_fn);
    };
    ($static_ident:ident, $name:expr, $init_fn:ident) => {
        $crate::module_init!($static_ident, $name, $init_fn);
    };
}

/// kernel macro to auto-register a driver (`module_driver`).
#[macro_export]
macro_rules! module_driver {
    ($static_ident:ident, $init_fn_ident:ident, $driver_expr:expr) => {
        pub(crate) fn $init_fn_ident() -> Result<(), ostd::Error> {
            $crate::device::register_driver(alloc::sync::Arc::new($driver_expr))
        }
        $crate::module_init!($static_ident, $init_fn_ident);
    };
    ($static_ident:ident, $init_fn_ident:ident, $name:expr, $driver_expr:expr) => {
        pub(crate) fn $init_fn_ident() -> Result<(), ostd::Error> {
            $crate::device::register_driver(alloc::sync::Arc::new($driver_expr))
        }
        $crate::module_init!($static_ident, $name, $init_fn_ident);
    };
}

/// Kernel macro for PCI driver auto-registration (`module_pci_driver`).
#[macro_export]
macro_rules! module_pci_driver {
    ($static_ident:ident, $init_fn_ident:ident, $driver_expr:expr) => {
        $crate::module_driver!($static_ident, $init_fn_ident, $driver_expr);
    };
    ($static_ident:ident, $init_fn_ident:ident, $name:expr, $driver_expr:expr) => {
        $crate::module_driver!($static_ident, $init_fn_ident, $name, $driver_expr);
    };
}
