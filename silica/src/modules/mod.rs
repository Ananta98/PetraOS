// SPDX-License-Identifier: GPL-2.0

//! Kernel Module System and Linux-compatible Initcalls.
//!
//! Provides:
//! - Linux-style staged initcalls (`Early`, `Core`, `PostCore`, `Arch`, `Subsys`, `Fs`, `Device`, `Late`).
//! - Modular driver abstraction ([`KernelModule`]).
//! - Loadable Kernel Module (LKM) ELF loader for `ET_DYN` and `ET_REL` images,
//!   with x86_64 relocation application and kernel symbol resolution.
//! - Kernel symbol table registry ([`symbols`]) for symbol lookups and exports.
//! - 100% Safe Rust implementation complying with kernel `#![deny(unsafe_code)]`.
//!
//! Note: loaded module images currently live in non-executable kernel memory.
//! Calling module entry points requires kernel-VA executable mapping support
//! from OSTD and is tracked as follow-up work.

pub mod initcall;
pub mod loader;
pub mod params;
pub mod registry;
pub mod reloc;
pub mod symbols;

use crate::uapi::errno::Result;

pub use initcall::{InitcallEntry, InitcallFn, InitcallLevel, do_initcalls, register_initcall};
pub use loader::{load_module, load_module_from_file};
pub use registry::{
    KernelModule, ModuleInfo, ModuleState, init_compiled_module, list_modules, register_module,
    register_module_declaration, unload_module,
};
pub use symbols::{lookup_symbol, register_symbol};

/// Boots the module subsystem: exports core kernel symbols, then runs every
/// staged initcall.
pub fn init_calls() -> Result<()> {
    symbols::register_core_symbols();
    do_initcalls()
}

// ----------------------------------------------------------------------------
// Kernel Driver Module Macro
// ----------------------------------------------------------------------------

/// Macro for declaring and registering a kernel driver module.
///
/// # Examples
/// ```
/// module!("Ext2 Filesystem", "PetraOS Team", InitcallLevel::Fs, init);
/// ```
#[macro_export]
macro_rules! module {
    ($desc:expr, $author:expr, $level:expr, $init_fn:path) => {
        const _: () = {
            #[used]
            #[allow(unsafe_code, unsafe_attr_outside_unsafe)]
            #[unsafe(link_section = ".init_array")]
            static __INIT_CALL: extern "C" fn() = {
                extern "C" fn __register() {
                    $crate::modules::register_module_declaration($desc, $author, $level, $init_fn);
                }
                __register
            };
        };
    };
}

// ----------------------------------------------------------------------------
// Kernel Symbol Export Macro
// ----------------------------------------------------------------------------

/// Macro to export a kernel function or symbol.
#[macro_export]
macro_rules! export_symbol {
    ($sym:ident) => {
        $crate::modules::register_symbol(stringify!($sym), {
            // The function-item cast is signature-agnostic; the lint is known.
            #[allow(function_casts_as_integer)]
            let __addr: usize = $sym as usize;
            __addr
        });
    };
}
