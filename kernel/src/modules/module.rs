//! Kernel Module Definitions and Metadata

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleState {
    Unloaded,
    Loading,
    Live,
    Unloading,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModAttrKind {
    License,
    Author,
    Description,
    Version,
}

#[repr(C)]
pub struct ModAttr {
    pub module_path: &'static str,
    pub kind: ModAttrKind,
    pub value: &'static str,
}

#[allow(improper_ctypes)]
unsafe extern "C" {
    static __modinfo_start: ModAttr;
    static __modinfo_end: ModAttr;
}

pub struct ModuleInfo {
    pub name: &'static str,
    pub author: &'static str,
    pub description: &'static str,
    pub license: &'static str,
    pub version: &'static str,
}

pub struct KernelModule {
    pub info: ModuleInfo,
    pub state: ModuleState,
    pub init: fn() -> Result<(), &'static str>,
    pub exit: Option<fn()>,
}

impl KernelModule {
    pub const fn new(
        name: &'static str,
        author: &'static str,
        description: &'static str,
        license: &'static str,
        version: &'static str,
        init: fn() -> Result<(), &'static str>,
        exit: Option<fn()>,
    ) -> Self {
        Self {
            info: ModuleInfo {
                name,
                author,
                description,
                license,
                version,
            },
            state: ModuleState::Unloaded,
            init,
            exit,
        }
    }
}

pub fn get_attr_for_module(
    module_path: &str,
    kind: ModAttrKind,
    default: &'static str,
) -> &'static str {
    let (start_ptr, count) = unsafe {
        let start = &__modinfo_start as *const ModAttr;
        let end = &__modinfo_end as *const ModAttr;
        let num_attrs = if (end as usize) >= (start as usize) {
            (end as usize - start as usize) / core::mem::size_of::<ModAttr>()
        } else {
            0
        };
        (start, num_attrs)
    };

    if count > 0 {
        let attrs = unsafe { core::slice::from_raw_parts(start_ptr, count) };
        for attr in attrs {
            if attr.module_path == module_path && attr.kind == kind {
                return attr.value;
            }
        }
    }
    default
}

/// Linux-style MODULE_LICENSE macro
#[macro_export]
macro_rules! MODULE_LICENSE {
    ($val:expr) => {
        const _: () = {
            #[used]
            #[unsafe(link_section = ".modinfo")]
            static __ATTR_LICENSE: $crate::modules::module::ModAttr =
                $crate::modules::module::ModAttr {
                    module_path: module_path!(),
                    kind: $crate::modules::module::ModAttrKind::License,
                    value: $val,
                };
        };
    };
}

#[macro_export]
macro_rules! MODULE_AUTHOR {
    ($val:expr) => {
        const _: () = {
            #[used]
            #[unsafe(link_section = ".modinfo")]
            static __ATTR_AUTHOR: $crate::modules::module::ModAttr =
                $crate::modules::module::ModAttr {
                    module_path: module_path!(),
                    kind: $crate::modules::module::ModAttrKind::Author,
                    value: $val,
                };
        };
    };
}

#[macro_export]
macro_rules! MODULE_DESCRIPTION {
    ($val:expr) => {
        const _: () = {
            #[used]
            #[unsafe(link_section = ".modinfo")]
            static __ATTR_DESC: $crate::modules::module::ModAttr =
                $crate::modules::module::ModAttr {
                    module_path: module_path!(),
                    kind: $crate::modules::module::ModAttrKind::Description,
                    value: $val,
                };
        };
    };
}

/// Linux-style MODULE_VERSION macro
#[macro_export]
macro_rules! MODULE_VERSION {
    ($val:expr) => {
        const _: () = {
            #[used]
            #[unsafe(link_section = ".modinfo")]
            static __ATTR_VER: $crate::modules::module::ModAttr =
                $crate::modules::module::ModAttr {
                    module_path: module_path!(),
                    kind: $crate::modules::module::ModAttrKind::Version,
                    value: $val,
                };
        };
    };
}
