/// Kernel module management subsystem.
///
/// # Module layout
///
/// | File | Responsibility |
/// |------|---------------|
/// | [`module`]  | [`KernelModule`] trait, [`ModuleState`], and [`ModuleInfo`] |
/// | [`manager`] | Global registry, registration, load/unload lifecycle management |
/// | [`builtin`] | Pre-packaged built-in kernel modules |
///
/// # Public surface
///
/// ```rust,ignore
/// use crate::modules::{KernelModule, ModuleInfo, ModuleState};
/// use crate::modules::{register_module, load_module, unload_module, list_modules};
/// ```

pub mod builtin;
pub mod manager;
pub mod module;

pub use manager::{
    get_module_info, init, list_modules, load_module, register_module, unload_module,
    unregister_module,
};
pub use module::{KernelModule, ModuleInfo, ModuleState};
