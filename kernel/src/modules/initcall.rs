//! Kernel Initcall Infrastructure

pub type InitCallFn = fn() -> Result<(), &'static str>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum InitLevel {
    Early = 0,
    Core = 1,
    Arch = 2,
    Fs = 3,
    Device = 4,
    Late = 5,
    Module = 6,
}

#[repr(C)]
pub struct InitCall {
    pub level: InitLevel,
    pub name: &'static str,
    pub func: InitCallFn,
}

#[allow(improper_ctypes)]
unsafe extern "C" {
    static __initcall_start: InitCall;
    static __initcall_end: InitCall;
}

#[macro_export]
macro_rules! early_initcall {
    ($fn:path) => {
        const _: () = {
            #[used]
            #[unsafe(link_section = ".initcall.0.early")]
            static __ENTRY: $crate::modules::initcall::InitCall =
                $crate::modules::initcall::InitCall {
                    level: $crate::modules::initcall::InitLevel::Early,
                    name: stringify!($fn),
                    func: $fn,
                };
        };
    };
}

#[macro_export]
macro_rules! core_initcall {
    ($fn:path) => {
        const _: () = {
            #[used]
            #[unsafe(link_section = ".initcall.1.core")]
            static __ENTRY: $crate::modules::initcall::InitCall =
                $crate::modules::initcall::InitCall {
                    level: $crate::modules::initcall::InitLevel::Core,
                    name: stringify!($fn),
                    func: $fn,
                };
        };
    };
}

#[macro_export]
macro_rules! arch_initcall {
    ($fn:path) => {
        const _: () = {
            #[used]
            #[unsafe(link_section = ".initcall.2.arch")]
            static __ENTRY: $crate::modules::initcall::InitCall =
                $crate::modules::initcall::InitCall {
                    level: $crate::modules::initcall::InitLevel::Arch,
                    name: stringify!($fn),
                    func: $fn,
                };
        };
    };
}

#[macro_export]
macro_rules! fs_initcall {
    ($fn:path) => {
        const _: () = {
            #[used]
            #[unsafe(link_section = ".initcall.3.fs")]
            static __ENTRY: $crate::modules::initcall::InitCall =
                $crate::modules::initcall::InitCall {
                    level: $crate::modules::initcall::InitLevel::Fs,
                    name: stringify!($fn),
                    func: $fn,
                };
        };
    };
}

#[macro_export]
macro_rules! device_initcall {
    ($fn:path) => {
        const _: () = {
            #[used]
            #[unsafe(link_section = ".initcall.4.device")]
            static __ENTRY: $crate::modules::initcall::InitCall =
                $crate::modules::initcall::InitCall {
                    level: $crate::modules::initcall::InitLevel::Device,
                    name: stringify!($fn),
                    func: $fn,
                };
        };
    };
}

#[macro_export]
macro_rules! late_initcall {
    ($fn:path) => {
        const _: () = {
            #[used]
            #[unsafe(link_section = ".initcall.5.late")]
            static __ENTRY: $crate::modules::initcall::InitCall =
                $crate::modules::initcall::InitCall {
                    level: $crate::modules::initcall::InitLevel::Late,
                    name: stringify!($fn),
                    func: $fn,
                };
        };
    };
}

#[macro_export]
macro_rules! module_init {
    ($fn:path) => {
        const _: () = {
            #[used]
            #[unsafe(link_section = ".initcall.6.module")]
            static __ENTRY: $crate::modules::initcall::InitCall =
                $crate::modules::initcall::InitCall {
                    level: $crate::modules::initcall::InitLevel::Module,
                    name: stringify!($fn),
                    func: $fn,
                };
        };
    };
}

#[macro_export]
macro_rules! module_driver {
    ($initcall_ident:ident, $init_fn:ident, $name:expr, $driver_ty:ty) => {
        pub fn $init_fn() -> Result<(), &'static str> {
            use $crate::device::Driver;
            use $crate::modules::module::{get_attr_for_module, ModAttrKind};

            let driver = <$driver_ty>::default();
            let path = module_path!();
            let author = get_attr_for_module(path, ModAttrKind::Author, "PetraOS Development Team");
            let desc = get_attr_for_module(path, ModAttrKind::Description, driver.description());
            let license = get_attr_for_module(path, ModAttrKind::License, "BSD-2-Clause");
            let version = get_attr_for_module(path, ModAttrKind::Version, "1.0.0");

            match driver.probe() {
                Ok(()) => {
                    let module = $crate::modules::KernelModule::new(
                        $name,
                        author,
                        desc,
                        license,
                        version,
                        $init_fn,
                        None,
                    );
                    let _ = $crate::modules::MODULE_MANAGER.lock().register(module);
                    Ok(())
                }
                Err(_e) => {
                    log::info!(
                        "[Module Driver] Driver '{}' probe finished (no device or skipped).",
                        driver.name()
                    );
                    Ok(())
                }
            }
        }

        $crate::device_initcall!($init_fn);
    };
}

/// Execute all registered initcalls in priority order (from Early to Module).
pub fn do_initcalls() {
    let (start_ptr, count) = unsafe {
        let start = &__initcall_start as *const InitCall;
        let end = &__initcall_end as *const InitCall;
        let num_calls = if (end as usize) >= (start as usize) {
            (end as usize - start as usize) / core::mem::size_of::<InitCall>()
        } else {
            0
        };
        (start, num_calls)
    };

    let initcalls = if count > 0 {
        unsafe { core::slice::from_raw_parts(start_ptr, count) }
    } else {
        &[]
    };

    log::info!("[Initcall] Discovered {} initcall(s)", initcalls.len());

    for call in initcalls {
        let level_name = match call.level {
            InitLevel::Early => "Early",
            InitLevel::Core => "Core",
            InitLevel::Arch => "Arch",
            InitLevel::Fs => "Filesystem",
            InitLevel::Device => "Device",
            InitLevel::Late => "Late",
            InitLevel::Module => "Module",
        };

        log::info!(
            "[Initcall:{}] Calling init function '{}'",
            level_name,
            call.name
        );
        if let Err(e) = (call.func)() {
            log::error!(
                "[Initcall:{}] Function '{}' failed: {}",
                level_name,
                call.name,
                e
            );
        }
    }
}
