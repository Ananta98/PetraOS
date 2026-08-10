//! Kernel Module Definitions and Metadata

pub use linkme::distributed_slice;

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

pub struct ModAttr {
    pub module_path: &'static str,
    pub kind: ModAttrKind,
    pub value: &'static str,
}

#[distributed_slice]
pub static MOD_ATTRS: [ModAttr];

pub struct ModuleInfo {
    pub name: &'static str,
    pub author: &'static str,
    pub description: &'static str,
    pub license: &'static str,
    pub version: &'static str,
}

#[distributed_slice]
pub static MODULE_METADATA: [ModuleInfo];

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
    for attr in MOD_ATTRS.iter() {
        if attr.module_path == module_path && attr.kind == kind {
            return attr.value;
        }
    }
    default
}

/// Linux-style MODULE_LICENSE macro
#[macro_export]
macro_rules! MODULE_LICENSE {
    ($val:expr) => {
        const _: () = {
            #[$crate::modules::module::distributed_slice($crate::modules::module::MOD_ATTRS)]
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
            #[$crate::modules::module::distributed_slice($crate::modules::module::MOD_ATTRS)]
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
            #[$crate::modules::module::distributed_slice($crate::modules::module::MOD_ATTRS)]
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
            #[$crate::modules::module::distributed_slice($crate::modules::module::MOD_ATTRS)]
            static __ATTR_VER: $crate::modules::module::ModAttr =
                $crate::modules::module::ModAttr {
                    module_path: module_path!(),
                    kind: $crate::modules::module::ModAttrKind::Version,
                    value: $val,
                };
        };
    };
}

/// Unified module_info! macro
#[macro_export]
macro_rules! module_info {
    (
        name: $name:expr,
        author: $author:expr,
        description: $desc:expr,
        license: $license:expr,
        version: $version:expr $(,)?
    ) => {
        const _: () = {
            #[$crate::modules::module::distributed_slice($crate::modules::module::MODULE_METADATA)]
            static __META: $crate::modules::module::ModuleInfo =
                $crate::modules::module::ModuleInfo {
                    name: $name,
                    author: $author,
                    description: $desc,
                    license: $license,
                    version: $version,
                };
        };
    };
}
