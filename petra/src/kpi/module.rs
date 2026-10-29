// SPDX-License-Identifier: GPL-2.0

//! Linux kernel module lifecycle macros and declarations.
//!
//! Provides Linux-compatible `module_init!`, `module_exit!`, and staged initcall macros
//! (`core_initcall!`, `postcore_initcall!`, `subsys_initcall!`, `device_initcall!`, `late_initcall!`).

pub use crate::modules::InitcallLevel;

/// Linux `core_initcall`: registers an init function at the Core stage (level 1).
#[macro_export]
macro_rules! core_initcall {
    ($init_fn:path) => {
        $crate::module!(
            concat!("core:", stringify!($init_fn)),
            "Linux KPI Core Driver",
            $crate::modules::InitcallLevel::Core,
            $init_fn
        );
    };
}

/// Linux `postcore_initcall`: registers an init function at the PostCore stage (level 2).
#[macro_export]
macro_rules! postcore_initcall {
    ($init_fn:path) => {
        $crate::module!(
            concat!("postcore:", stringify!($init_fn)),
            "Linux KPI Driver",
            $crate::modules::InitcallLevel::PostCore,
            $init_fn
        );
    };
}

/// Linux `subsys_initcall`: registers an init function at the Subsystem stage (level 4).
#[macro_export]
macro_rules! subsys_initcall {
    ($init_fn:path) => {
        $crate::module!(
            concat!("subsys:", stringify!($init_fn)),
            "Linux KPI Driver",
            $crate::modules::InitcallLevel::Subsys,
            $init_fn
        );
    };
}

/// Linux `device_initcall` / `module_init`: registers a driver init function at the Device stage (level 6).
#[macro_export]
macro_rules! device_initcall {
    ($init_fn:path) => {
        $crate::module!(
            concat!("device:", stringify!($init_fn)),
            "Linux KPI Driver",
            $crate::modules::InitcallLevel::Device,
            $init_fn
        );
    };
}

/// Linux `late_initcall`: registers an init function at the Late stage (level 7).
#[macro_export]
macro_rules! late_initcall {
    ($init_fn:path) => {
        $crate::module!(
            concat!("late:", stringify!($init_fn)),
            "Linux KPI Driver",
            $crate::modules::InitcallLevel::Late,
            $init_fn
        );
    };
}

/// Linux `module_init`: standard entrypoint macro for kernel modules and drivers.
#[macro_export]
macro_rules! module_init {
    ($init_fn:path) => {
        $crate::device_initcall!($init_fn);
    };
}

/// Linux `module_exit`: exit handler placeholder for dynamic unload.
#[macro_export]
macro_rules! module_exit {
    ($exit_fn:path) => {
        // Tracked for dynamic unload; compiled-in drivers do not execute exit hooks.
    };
}
