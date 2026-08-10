//! Kernel Modules & Initcall Subsystem

pub mod initcall;
pub mod manager;
pub mod module;

pub use initcall::{do_initcalls, InitCall, InitCallFn, InitLevel};
pub use manager::{ModuleManager, MODULE_MANAGER};
pub use module::{KernelModule, ModuleInfo, ModuleState};

pub fn init() {
    log::info!("Initializing Kernel Module Subsystem...");
    do_initcalls();
    MODULE_MANAGER.lock().list_modules();
}
