/// Kernel module definitions and trait abstractions.
///
/// All kernel modules must implement the [`KernelModule`] trait.

use alloc::string::String;

/// Represents the lifecycle state of a kernel module.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleState {
    /// Module is registered in the kernel but not initialized.
    Registered,
    /// Module is currently executing its initialization sequence.
    Initializing,
    /// Module is active, initialized, and operational.
    Active,
    /// Module is executing its cleanup/exit sequence.
    Exiting,
    /// Module has been unloaded and deactivated.
    Unloaded,
    /// Module encountered an error during initialization or execution.
    Failed,
}

impl ModuleState {
    /// Returns `true` if the module is currently active.
    pub fn is_active(&self) -> bool {
        matches!(self, ModuleState::Active)
    }

    /// Returns `true` if the module is registered but uninitialized.
    pub fn is_registered(&self) -> bool {
        matches!(self, ModuleState::Registered)
    }

    /// Returns `true` if the module failed.
    pub fn is_failed(&self) -> bool {
        matches!(self, ModuleState::Failed)
    }
}

/// Snapshot of kernel module metadata and state.
#[derive(Debug, Clone)]
pub struct ModuleInfo {
    /// Unique name identifying the module.
    pub name: String,
    /// Version string of the module (e.g. "1.0.0").
    pub version: String,
    /// Author or organization responsible for the module.
    pub author: String,
    /// Short description of the module's functionality.
    pub description: String,
    /// Open source or proprietary license tag (e.g. "GPL-2.0", "MIT").
    pub license: String,
    /// Current operational state of the module.
    pub state: ModuleState,
}

/// Core abstraction for safe kernel modules.
///
/// Implementations of this trait must be safe (`#![deny(unsafe_code)]`),
/// thread-safe (`Send + Sync`), and interact with hardware or kernel services
/// strictly through `ostd` safe abstractions.
pub trait KernelModule: Send + Sync {
    /// Returns metadata describing this module.
    fn info(&self) -> ModuleInfo;

    /// Called when the kernel module is loaded or activated.
    ///
    /// # Errors
    /// Returns [`ostd::Error`] if initialization fails.
    fn init(&self) -> Result<(), ostd::Error>;

    /// Called when the kernel module is unloaded or deactivated.
    ///
    /// # Errors
    /// Returns [`ostd::Error`] if resource cleanup fails.
    fn exit(&self) -> Result<(), ostd::Error>;
}
