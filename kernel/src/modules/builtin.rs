/// Built-in kernel modules included directly in the PetraOS image.

use crate::modules::manager::{load_module, register_module};
use crate::modules::module::{KernelModule, ModuleInfo, ModuleState};
use alloc::string::String;
use alloc::sync::Arc;

/// Built-in system telemetry kernel module.
pub struct SystemTelemetryModule;

impl KernelModule for SystemTelemetryModule {
    fn info(&self) -> ModuleInfo {
        ModuleInfo {
            name: String::from("system_telemetry"),
            version: String::from("1.0.0"),
            author: String::from("PetraOS Core Team"),
            description: String::from("Kernel telemetry and diagnostics module"),
            license: String::from("GPL-2.0"),
            state: ModuleState::Registered,
        }
    }

    fn init(&self) -> Result<(), ostd::Error> {
        log::info!("[system_telemetry] Initialized kernel telemetry module.");
        Ok(())
    }

    fn exit(&self) -> Result<(), ostd::Error> {
        log::info!("[system_telemetry] Cleaning up kernel telemetry module.");
        Ok(())
    }
}

/// Built-in null device driver kernel module.
pub struct NullDeviceModule;

impl KernelModule for NullDeviceModule {
    fn info(&self) -> ModuleInfo {
        ModuleInfo {
            name: String::from("null_device"),
            version: String::from("1.0.0"),
            author: String::from("PetraOS Drivers"),
            description: String::from("Null pseudo-device kernel module"),
            license: String::from("MIT"),
            state: ModuleState::Registered,
        }
    }

    fn init(&self) -> Result<(), ostd::Error> {
        log::info!("[null_device] Initialized null device module.");
        Ok(())
    }

    fn exit(&self) -> Result<(), ostd::Error> {
        log::info!("[null_device] Unloaded null device module.");
        Ok(())
    }
}

/// Registers and loads all default built-in kernel modules.
pub fn init_builtin_modules() -> Result<(), ostd::Error> {
    let telemetry = Arc::new(SystemTelemetryModule);
    register_module(telemetry)?;
    load_module("system_telemetry")?;

    let null_dev = Arc::new(NullDeviceModule);
    register_module(null_dev)?;
    load_module("null_device")?;

    Ok(())
}
