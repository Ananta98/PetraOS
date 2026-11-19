// SPDX-License-Identifier: MPL-2.0
//
// Ported from Asterinas `aster-device` (`kernel/core/comps/device/src`):
// https://github.com/asterinas/asterinas/tree/e36b5b3b759e616febf2c8f0e88d8cb2c103f266/kernel/core/comps/device/src
// Copyright (c) Asterinas contributors. Adapted for PetraOS.

//! Shared device types, attributes, device numbers, and registration machinery.
//!
//! Build a device through [`BusDevice::builder`] or [`ClassDevice::builder`],
//! then call [`add_device`] to register it.
//! Use [`BareDevice`] for a parent that belongs to neither a bus nor a class.
//! Remove child devices before calling [`remove_device`] on their parent.
//!
//! [`BusDevice::builder`]: crate::drivers::core::bus::BusDevice::builder
//! [`ClassDevice::builder`]: crate::drivers::core::class::ClassDevice::builder
//! [`add_device`]: crate::drivers::core::add_device
//! [`remove_device`]: crate::drivers::core::remove_device

pub(crate) mod attr;
mod bare_device;
mod devnum;
mod error;
mod internals;
pub(crate) mod node;
pub(crate) mod registration;
pub(crate) mod registry;
pub(crate) mod subsystem;

use alloc::{
    borrow::Cow,
    string::String,
    sync::{Arc, Weak},
    vec::Vec,
};

pub(crate) use self::internals::{DeviceBase, DeviceInternals};
use self::{
    attr::TyErasedAttr,
    node::{Dir, SysTreeEdit},
};
pub use self::{
    attr::{Attr, AttrMeta, AttrSet, ShowFn, StoreFn, SysPerms},
    bare_device::BareDevice,
    devnum::{DEFAULT_DEVNODE_MODE, DevKind, DevNum, DeviceId},
    error::{Error, Result},
    registration::DeviceBuilder,
    subsystem::{Subsystem, SubsystemKind},
};
use crate::fs::sysfs::kobject::SysNode;

/// An owned string or a static reference to a string.
pub type SysStr = Cow<'static, str>;

/// An interface shared by all devices.
///
/// Use `dyn AnyDevice` to inspect devices without knowing their concrete bus or class type.
/// Implemented by [`BusDevice`], [`ClassDevice`], and [`BareDevice`].
///
/// [`BusDevice`]: crate::drivers::core::bus::BusDevice
/// [`ClassDevice`]: crate::drivers::core::class::ClassDevice
pub trait AnyDevice: SysNode + DeviceInternals {
    /// Returns this device as a dynamic `Any` trait object for safe downcasting.
    fn as_any(&self) -> &dyn core::any::Any;

    /// Returns the device number, if user space can open this device.
    fn devnum(&self) -> Option<DevNum> {
        self.base().devnum()
    }

    /// Returns the subsystem that owns the device.
    fn subsystem(&self) -> Subsystem;

    /// Returns the absolute path of this device within the sysfs tree.
    fn path(&self) -> String {
        self.base().tree_path()
    }

    /// Returns a strong reference to this device as a trait object.
    fn to_arc(&self) -> Arc<dyn AnyDevice> {
        self.base()
            .weak_self()
            .upgrade()
            .expect("a device is only reachable through an `Arc`")
    }
}

/// A finer kind within one bus or class, such as `disk` versus `partition` in the `block` class.
///
/// A device type contributes attributes and a `devnode` policy to every device that carries it.
pub struct DeviceType<D: 'static> {
    /// The device type name.
    pub name: &'static str,
    /// Attributes contributed by this device type.
    pub attrs: &'static [Attr<D>],
    /// Overrides the `/dev` node name or mode.
    pub devnode: Option<fn(&D) -> Option<DevNode>>,
    /// Whether a class device with this device type gets the `device` symlink to its parent.
    pub has_device_link: bool,
}

impl<D: 'static> DeviceType<D> {
    /// Creates a device type with a name and nothing else.
    pub const fn named(name: &'static str) -> Self {
        Self {
            name,
            attrs: &[],
            devnode: None,
            has_device_link: true,
        }
    }
}

/// What a `devnode` hook may override for a device's `/dev` node.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DevNode {
    /// The node path relative to `/dev`; `None` keeps the device name.
    pub path: Option<SysStr>,
    /// The permission bits; `None` keeps the default.
    pub mode: Option<u16>,
}

/// The device type and device-specific attributes of a bus or class device.
pub(crate) struct DeclaredParts<D: 'static> {
    dev_type: Option<&'static DeviceType<D>>,
    own_attrs: &'static [Attr<D>],
}

impl<D: AnyDevice> DeclaredParts<D> {
    /// Returns the device type name, if the device has a device type.
    pub(crate) fn type_name(&self) -> Option<&'static str> {
        self.dev_type.map(|t| t.name)
    }

    /// Collects type-erased attributes from the subsystem, the device type, and the device, in that
    /// order.
    pub(crate) fn attr_groups(&self, subsystem_attrs: &[Attr<D>]) -> Vec<TyErasedAttr> {
        let mut attrs = TyErasedAttr::from_typed_slice(subsystem_attrs);
        if let Some(t) = self.dev_type {
            attrs.extend(TyErasedAttr::from_typed_slice(t.attrs));
        }
        attrs.extend(TyErasedAttr::from_typed_slice(self.own_attrs));
        attrs
    }

    /// Returns the device type's `/dev` node override, if it has one.
    pub(crate) fn type_devnode(&self, dev: &D) -> Option<DevNode> {
        self.dev_type.and_then(|t| t.devnode).and_then(|f| f(dev))
    }

    /// Returns whether the device type keeps the `device` symlink.
    pub(crate) fn has_device_link(&self) -> bool {
        self.dev_type.is_none_or(|t| t.has_device_link)
    }
}

/// The life cycle of a device.
/// Registration and removal each happen once.
#[derive(Debug)]
enum State {
    /// Built, not yet in the tree.
    Initialized,
    /// Preparing the directory and its resources; registration may still fail.
    Preparing,
    /// Registration can no longer fail; subsystem callbacks may add children.
    Adding(ChildDevices),
    /// Registered. Only devices in this state can be removed.
    Added(ChildDevices),
    /// Removed; the object may still be referenced but is dead.
    Removed,
}

type ChildDevices = Vec<Weak<dyn AnyDevice>>;

/// The symlinks a device's registration has created so far.
#[derive(Default)]
struct Links {
    subsystem: bool,
    device: bool,
    index: bool,
    dev_index: bool,
}

/// Where a device's directory was placed:
/// a plain directory (a root, a glue directory) or another device's directory.
pub(crate) enum TreeParent {
    Dir(Weak<Dir>),
    Device(Weak<dyn AnyDevice>),
}

impl TreeParent {
    /// Runs `f` with the crate-private editing view of the parent, if alive.
    fn with_edit<R>(&self, f: impl FnOnce(&dyn SysTreeEdit) -> R) -> Option<R> {
        match self {
            TreeParent::Dir(dir) => dir.upgrade().map(|dir| f(dir.as_ref())),
            TreeParent::Device(dev) => dev.upgrade().map(|dev| f(dev.base())),
        }
    }

    /// Returns the absolute path of this parent container, if still alive.
    pub(crate) fn tree_path(&self) -> Option<String> {
        self.with_edit(|node| node.tree_path())
    }
}

/// Implements [`SysNode`] for a device struct by delegating to its [`DeviceBase`].
macro_rules! impl_device_node {
    ($ty:ident $(< $p:ident : $bound:path >)?) => {
        impl$(<$p: $bound>)? $crate::fs::sysfs::kobject::SysNode for $ty$(<$p>)? {
            fn name(&self) -> &str {
                self.base().name().as_ref()
            }

            fn ino(&self) -> usize {
                self.base().ino()
            }

            fn is_dir(&self) -> bool {
                true
            }

            fn get_child(
                &self,
                name: &str,
            ) -> Option<::alloc::sync::Arc<dyn $crate::fs::sysfs::kobject::SysNode>> {
                if !self.base().is_added() {
                    return None;
                }
                self.base().get_child(name)
            }

            fn list_children(&self) -> ::alloc::vec::Vec<(::alloc::string::String, bool)> {
                if !self.base().is_added() {
                    return ::alloc::vec::Vec::new();
                }
                self.base().list_children()
            }
        }
    };
}

pub(crate) use impl_device_node;
