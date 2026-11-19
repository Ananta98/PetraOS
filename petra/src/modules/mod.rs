// SPDX-License-Identifier: GPL-2.0

//! Kernel Module System and Safe Component Initcalls.
//!
//! Provides:
//! - Linux-style staged initcalls (`Early`, `Core`, `PostCore`, `Arch`, `Subsys`, `Fs`, `Device`, `Late`),
//!   sorted deterministically by `(level, name)`.
//! - Modular driver abstraction ([`KernelModule`]).
//! - Safe distributed registration of compiled-in modules via [`inventory`]
//!   (inspired by Asterinas `component`: static descriptor submission with zero
//!   `unsafe` code and zero heap allocation before `kernel_main`).
//! - Loadable Kernel Module (LKM) ELF loader for `ET_DYN` and `ET_REL` images,
//!   with x86_64 relocation application and kernel symbol resolution.
//! - Kernel symbol table registry ([`symbols`]) for symbol lookups and exports.
//! - 100% Safe Rust implementation complying with `#![forbid(unsafe_code)]`.
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

use spin::Once;

use crate::uapi::errno::Result;

pub use initcall::{InitcallEntry, InitcallFn, InitcallLevel, do_initcalls, register_initcall};
pub use loader::{load_module, load_module_from_file};
pub use registry::{
    KernelModule, ModuleInfo, ModuleState, init_compiled_module, list_modules, register_module,
    register_module_declaration, unload_module,
};
pub use symbols::{lookup_symbol, register_symbol};

/// Static descriptor submitted at link time by [`crate::module!`], collected
/// safely via `inventory` during [`init_calls`].
pub struct ModuleDescriptor {
    pub description: &'static str,
    pub author: &'static str,
    pub level: InitcallLevel,
    pub init_fn: InitcallFn,
}

impl ModuleDescriptor {
    /// Creates a new static module descriptor.
    pub const fn new(
        description: &'static str,
        author: &'static str,
        level: InitcallLevel,
        init_fn: InitcallFn,
    ) -> Self {
        Self {
            description,
            author,
            level,
            init_fn,
        }
    }
}

inventory::collect!(ModuleDescriptor);

#[doc(hidden)]
pub use inventory::submit as __submit_module;

static MODULES_COLLECTED: Once = Once::new();

/// Boots the module subsystem: exports core kernel symbols, collects all
/// compiled-in module descriptors, and runs every staged initcall.
pub fn init_calls() -> Result<()> {
    symbols::register_core_symbols();
    MODULES_COLLECTED.call_once(|| {
        for desc in inventory::iter::<ModuleDescriptor> {
            register_module_declaration(desc.description, desc.author, desc.level, desc.init_fn);
        }
    });
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
        $crate::modules::__submit_module! {
            $crate::modules::ModuleDescriptor::new($desc, $author, $level, $init_fn)
        }
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

#[cfg(ktest)]
mod tests {
    use super::*;
    use alloc::vec::Vec;
    use ostd::prelude::ktest;

    #[ktest]
    fn inventory_collects_compiled_in_modules() {
        let names: Vec<&'static str> = inventory::iter::<ModuleDescriptor>
            .into_iter()
            .map(|desc| desc.description)
            .collect();

        for expected in [
            "AHCI SATA Storage Driver",
            "Ext4 Filesystem Driver",
            "Virtio PCI Transport",
            "DRM Framebuffer Subsystem",
        ] {
            assert!(
                names.contains(&expected),
                "compiled-in module descriptor missing: {expected}"
            );
        }
    }
}
