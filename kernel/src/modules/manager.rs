//! Kernel Module Manager

use alloc::vec::Vec;
use crate::sync::spinlock::Spinlock;
use super::module::KernelModule;

pub static MODULE_MANAGER: Spinlock<ModuleManager> = Spinlock::new(ModuleManager::new());

pub struct ModuleManager {
    modules: Vec<KernelModule>,
}

impl ModuleManager {
    pub const fn new() -> Self {
        Self { modules: Vec::new() }
    }

    pub fn register(&mut self, module: KernelModule) -> Result<(), &'static str> {
        for m in &self.modules {
            if m.info.name == module.info.name {
                return Err("Module with this name already registered");
            }
        }
        self.modules.push(module);
        Ok(())
    }

    pub fn list_modules(&self) {
        log::info!("── Registered Kernel Modules ({}) ──", self.modules.len());
        for m in &self.modules {
            log::info!(
                "  Module: {} v{} [{}] (Author: {}) - {}",
                m.info.name,
                m.info.version,
                m.info.license,
                m.info.author,
                m.info.description
            );
        }
    }
}
