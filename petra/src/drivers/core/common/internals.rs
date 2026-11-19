// SPDX-License-Identifier: MPL-2.0
//
// Ported from Asterinas `aster-device` (`kernel/core/comps/device/src`):
// https://github.com/asterinas/asterinas/tree/e36b5b3b759e616febf2c8f0e88d8cb2c103f266/kernel/core/comps/device/src
// Copyright (c) Asterinas contributors. Adapted for PetraOS.

//! Internal device interfaces and shared state.

use alloc::{
    collections::BTreeMap,
    string::{String, ToString},
    sync::{Arc, Weak},
    vec::Vec,
};

use ostd::sync::{Mutex, RwMutex};
use spin::Once;

use super::{AnyDevice, DevNode, Links, State, TreeParent};
use crate::drivers::core::{
    common::{
        Error, Result, SysStr,
        attr::{AttrSet, AttrTable, TyErasedAttr},
        devnum::DevNum,
        node::{DeviceAttrSysNode, GlueDirs, SysTreeEdit, is_valid_name},
    },
    hooks::DevNodeRequest,
};
use crate::fs::sysfs::kobject::{SysNode, alloc_sysfs_ino};

/// What the registration sequence asks a device for.
///
/// This trait seals [`AnyDevice`] and has no public import path outside the
/// device model.
pub trait DeviceInternals {
    /// Returns the shared device state.
    fn base(&self) -> &DeviceBase;

    /// Returns the device type name.
    fn type_name(&self) -> Option<&'static str>;

    /// Returns type-erased attributes contributed by the subsystem, the device type, and the device.
    fn attr_groups(&self) -> Vec<TyErasedAttr>;

    /// Returns the `/dev` node overrides from the device type and the subsystem.
    fn devnode_override(&self) -> Option<DevNode>;

    /// Returns whether the device gets a `device` symlink to its parent
    /// (class devices only; a device type may opt out).
    fn wants_device_link(&self) -> bool {
        true
    }
}

/// The properties and registration status shared by all devices.
pub struct DeviceBase {
    name: SysStr,
    ino: usize,
    weak_self: Weak<dyn AnyDevice>,
    /// The parent in the sysfs tree, as the registration sequence edits it.
    tree_parent: Once<TreeParent>,
    /// The entries of the device's directory: symlinks, glue directories,
    /// and child devices of any kind.
    children: RwMutex<BTreeMap<SysStr, Arc<dyn SysNode>>>,
    attrs: AttrTable,
    /// The device this one is reached through, if any.
    parent: Option<Arc<dyn AnyDevice>>,
    devnum: Option<DevNum>,
    state: Mutex<State>,
    /// Glue directories created under this device, one per class of child.
    glue_dirs: GlueDirs,
    /// Which of the symlinks that `add` may create exist,
    /// so that `teardown` removes only links this device made.
    links: Mutex<Links>,
    /// The `/dev` node created for this device, kept so it can be deleted.
    devnode: Mutex<Option<DevNodeRequest>>,
}

impl DeviceBase {
    /// Creates the shared state of a device.
    ///
    /// # Panics
    ///
    /// Panics if `name` is not a valid sysfs node name.
    pub(crate) fn new(
        name: SysStr,
        parent: Option<Arc<dyn AnyDevice>>,
        devnum: Option<DevNum>,
        weak_self: Weak<dyn AnyDevice>,
    ) -> Self {
        assert!(is_valid_name(&name), "invalid SysTree node name");
        Self {
            name,
            ino: alloc_sysfs_ino(),
            weak_self,
            tree_parent: Once::new(),
            children: RwMutex::new(BTreeMap::new()),
            attrs: AttrTable::new(),
            parent,
            devnum,
            state: Mutex::new(State::Initialized),
            glue_dirs: GlueDirs::new(),
            links: Mutex::new(Links::default()),
            devnode: Mutex::new(None),
        }
    }

    /// Returns the device name, which is also its directory name.
    pub fn name(&self) -> &SysStr {
        &self.name
    }

    /// Returns the sysfs inode number allocated for this device directory.
    pub(crate) fn ino(&self) -> usize {
        self.ino
    }

    /// Returns a weak reference to the owning device trait object.
    pub(crate) fn weak_self(&self) -> &Weak<dyn AnyDevice> {
        &self.weak_self
    }

    /// Returns the parent device, if any.
    pub(crate) fn parent(&self) -> Option<&Arc<dyn AnyDevice>> {
        self.parent.as_ref()
    }

    /// Returns whether the device is registered.
    pub(crate) fn is_added(&self) -> bool {
        matches!(*self.state.lock(), State::Adding(_) | State::Added(_))
    }

    pub(crate) fn attrs(&self) -> &AttrTable {
        &self.attrs
    }

    /// Returns the immutable attribute metadata snapshot (used for sysfs/test inspection).
    #[cfg_attr(not(ktest), allow(dead_code))]
    pub(crate) fn attr_set(&self) -> Arc<AttrSet> {
        self.attrs.set()
    }

    pub(super) fn tree_parent(&self) -> &Once<TreeParent> {
        &self.tree_parent
    }

    pub(super) fn devnum(&self) -> Option<DevNum> {
        self.devnum
    }

    pub(super) fn state(&self) -> &Mutex<State> {
        &self.state
    }

    pub(super) fn glue_dirs(&self) -> &GlueDirs {
        &self.glue_dirs
    }

    pub(super) fn links(&self) -> &Mutex<Links> {
        &self.links
    }

    pub(super) fn devnode(&self) -> &Mutex<Option<DevNodeRequest>> {
        &self.devnode
    }

    /// Looks up a child node or dynamic attribute file on this device.
    pub(crate) fn get_child(&self, name: &str) -> Option<Arc<dyn SysNode>> {
        if let Some(child) = self.children.read().get(name) {
            return Some(Arc::clone(child));
        }
        let meta = self.attrs.meta(name)?;
        // Derive a stable per-attribute inode number from the device's base inode
        // and the preserved attribute slot ID so inode numbers stay stable across binds.
        let attr_ino = self.ino.saturating_mul(256).saturating_add(meta.id() as usize + 1);
        Some(Arc::new(DeviceAttrSysNode {
            dev: self.weak_self.clone(),
            name: meta.name().clone(),
            perms: meta.perms(),
            ino: attr_ino,
        }))
    }

    /// Lists all child nodes and attribute files under this device.
    pub(crate) fn list_children(&self) -> Vec<(String, bool)> {
        let mut entries: Vec<(String, bool)> = self
            .children
            .read()
            .iter()
            .map(|(k, v)| (k.to_string(), v.is_dir()))
            .collect();
        let set = self.attrs.set();
        for meta in set.iter() {
            entries.push((meta.name().to_string(), false));
        }
        entries
    }

    /// Reads a named attribute of this device.
    #[cfg_attr(not(ktest), allow(dead_code))]
    pub(crate) fn show_attr(&self, name: &str) -> Result<String> {
        let dev = self.weak_self.upgrade().ok_or(Error::NotAdded)?;
        if !self.is_added() {
            return Err(Error::NotAdded);
        }
        self.attrs.show_string(dev.as_ref(), name)
    }

    /// Writes a named attribute of this device.
    #[cfg_attr(not(ktest), allow(dead_code))]
    pub(crate) fn store_attr(&self, name: &str, value: &str) -> Result<()> {
        let dev = self.weak_self.upgrade().ok_or(Error::NotAdded)?;
        if !self.is_added() {
            return Err(Error::NotAdded);
        }
        self.attrs.store_str(dev.as_ref(), name, value)
    }
}

impl SysTreeEdit for DeviceBase {
    fn attach_child(&self, child: Arc<dyn SysNode>) -> Result<()> {
        let name = SysStr::from(child.name().to_string());
        if self.attrs.meta(name.as_ref()).is_some() {
            return Err(Error::NameConflict);
        }
        let mut children = self.children.write();
        if children.contains_key(&name) {
            return Err(Error::NameConflict);
        }
        children.insert(name, child);
        Ok(())
    }

    fn detach_child(&self, name: &str) -> Result<Arc<dyn SysNode>> {
        self.children.write().remove(name).ok_or(Error::NotFound)
    }

    fn has_children(&self) -> bool {
        !self.children.read().is_empty()
    }

    /// Returns the device's absolute path in the sysfs tree.
    fn tree_path(&self) -> String {
        let Some(parent_path) = self.tree_parent.get().and_then(TreeParent::tree_path) else {
            return alloc::format!("/{}", self.name);
        };
        if parent_path == "/" {
            alloc::format!("/{}", self.name)
        } else {
            alloc::format!("{}/{}", parent_path, self.name)
        }
    }

    fn as_tree_parent(&self) -> TreeParent {
        TreeParent::Device(self.weak_self.clone())
    }
}

impl core::fmt::Debug for DeviceBase {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("DeviceBase")
            .field("name", self.name())
            .field("devnum", &self.devnum)
            .field("state", &*self.state.lock())
            .finish_non_exhaustive()
    }
}
