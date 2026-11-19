// SPDX-License-Identifier: MPL-2.0
//
// Ported from Asterinas `aster-device` (`kernel/core/comps/device/src`) and
// `aster-systree` (`kernel/core/comps/systree/src`):
// https://github.com/asterinas/asterinas/tree/e36b5b3b759e616febf2c8f0e88d8cb2c103f266/kernel/core/comps/device/src
// Copyright (c) Asterinas contributors. Adapted for PetraOS.

//! The device-model sysfs node types besides devices themselves:
//!
//! - plain directories (`Dir`: roots, index directories, glue directories,
//!   bus directories, driver directories),
//! - symbolic links (`SymlinkNode`),
//! - dynamic attribute projections (`DirAttrSysNode`, `DeviceAttrSysNode`),
//! - path validation and relative-symlink computation (`is_valid_name`,
//!   `relative_path`),
//! - and the crate-private [`SysTreeEdit`] interface used during registration.

use alloc::{
    boxed::Box,
    collections::BTreeMap,
    string::{String, ToString},
    sync::{Arc, Weak},
    vec::Vec,
};
use core::fmt::Write;

use ostd::sync::{Mutex, RwMutex};
use spin::Once;

use crate::drivers::core::common::{
    AnyDevice, Error, Result, SysStr, TreeParent, attr::SysPerms,
};
use crate::fs::sysfs::kobject::{SysNode, alloc_sysfs_ino};
use crate::uapi::errno::{Errno, Result as KernelResult};

/// Returns whether `name` is valid for a non-root sysfs node or attribute.
///
/// A valid name is nonempty, is neither `.` nor `..`, and contains no `/` or `NUL`.
pub fn is_valid_name(name: &str) -> bool {
    !name.is_empty() && !matches!(name, "." | "..") && !name.contains(['/', '\0'])
}

/// Computes the relative path from the directory `from_dir` to the target `to`.
///
/// Both arguments are absolute paths within the tree (the root is `/`).
/// The result always ends with the last component of `to`, matching Linux sysfs
/// symlink conventions (e.g. from `/devices/a/b/c` to `/devices/a` is `../../../a`).
pub fn relative_path(from_dir: &str, to: &str) -> String {
    let from: Vec<&str> = from_dir.split('/').filter(|s| !s.is_empty()).collect();
    let to: Vec<&str> = to.split('/').filter(|s| !s.is_empty()).collect();
    let Some((last, to_parent)) = to.split_last() else {
        return String::from("/");
    };
    let common = from
        .iter()
        .zip(to_parent.iter())
        .take_while(|(a, b)| a == b)
        .count();

    let mut result = String::new();
    for _ in common..from.len() {
        result.push_str("../");
    }
    for component in &to_parent[common..] {
        result.push_str(component);
        result.push('/');
    }
    result.push_str(last);
    result
}

/// The crate-private editing view of a container node.
///
/// Implemented by [`Dir`] and [`DeviceBase`], never exposed outside the
/// device model.
///
/// [`DeviceBase`]: crate::drivers::core::common::DeviceBase
pub(crate) trait SysTreeEdit: Send + Sync {
    /// Adds a child node, failing with [`Error::NameConflict`] if taken.
    fn attach_child(&self, child: Arc<dyn SysNode>) -> Result<()>;

    /// Removes and returns the child with the given name.
    fn detach_child(&self, name: &str) -> Result<Arc<dyn SysNode>>;

    /// Returns whether the container currently has any non-attribute children.
    fn has_children(&self) -> bool;

    /// Returns the container's absolute path within the device tree.
    fn tree_path(&self) -> String;

    /// Returns a [`TreeParent`] handle pointing to this container.
    fn as_tree_parent(&self) -> TreeParent;
}

/// Adds to `dir` a symlink named `name` pointing at `target_path`, expressed
/// relative to `dir`.
pub(crate) fn add_link(dir: &dyn SysTreeEdit, name: &str, target_path: &str) -> Result<()> {
    assert!(is_valid_name(name), "invalid SysTree symlink name");
    let target = relative_path(&dir.tree_path(), target_path);
    let link = SymlinkNode::new(SysStr::from(name.to_string()), target);
    dir.attach_child(link)
}

/// Removes the symlink named `name` from `dir`, ignoring its absence.
pub(crate) fn remove_link(dir: &dyn SysTreeEdit, name: &str) {
    let _ = dir.detach_child(name);
}

/// Glue directories owned by one container, one per class of child.
///
/// A glue directory is created when the first device of a class is placed under
/// the container and dropped when the last one leaves, serialized under one lock.
pub(crate) struct GlueDirs {
    dirs: Mutex<BTreeMap<SysStr, Arc<Dir>>>,
}

impl GlueDirs {
    pub(crate) const fn new() -> Self {
        Self {
            dirs: Mutex::new(BTreeMap::new()),
        }
    }

    /// Attaches `child` into the glue directory `name` under `owner`, creating
    /// the directory if needed, and returns that directory.
    pub(crate) fn attach_into(
        &self,
        name: &str,
        owner: &dyn SysTreeEdit,
        child: Arc<dyn SysNode>,
    ) -> Result<Arc<Dir>> {
        let mut dirs = self.dirs.lock();
        if let Some(dir) = dirs.get(name) {
            dir.attach_child(child)?;
            return Ok(dir.clone());
        }
        let dir = Dir::new(SysStr::from(name.to_string()));
        dir.init_parent(owner.as_tree_parent());
        owner.attach_child(dir.clone())?;
        if let Err(e) = dir.attach_child(child) {
            let _ = owner.detach_child(name);
            return Err(e);
        }
        dirs.insert(SysStr::from(name.to_string()), dir.clone());
        Ok(dir)
    }

    /// Drops the glue directory `name` under `owner` if it is now empty.
    pub(crate) fn drop_if_empty(&self, name: &str, owner: &dyn SysTreeEdit) {
        let mut dirs = self.dirs.lock();
        if let Some(dir) = dirs.get(name)
            && !dir.has_children()
        {
            let _ = owner.detach_child(name);
            dirs.remove(name);
        }
    }
}

/// Callbacks behind the fixed control attributes of a [`Dir`].
pub(crate) trait DirAttrOps: Send + Sync + 'static {
    /// Produces the text of an attribute.
    fn show(&self, _name: &str, _w: &mut dyn Write) -> Result<()> {
        Err(Error::NotFound)
    }

    /// Consumes the text written to an attribute.
    fn store(&self, _name: &str, _value: &str) -> Result<()> {
        Err(Error::NotFound)
    }
}

#[derive(Clone, Copy)]
struct DirAttrEntry {
    name: &'static str,
    perms: SysPerms,
    ino: usize,
}

/// A plain directory in the device sysfs tree.
pub(crate) struct Dir {
    name: SysStr,
    is_root: bool,
    ino: usize,
    parent: Once<TreeParent>,
    children: RwMutex<BTreeMap<SysStr, Arc<dyn SysNode>>>,
    attrs: Vec<DirAttrEntry>,
    ops: Once<Box<dyn DirAttrOps>>,
    weak: Weak<Self>,
}

impl Dir {
    /// Creates the root directory `/` of the device model tree.
    pub(crate) fn new_root() -> Arc<Self> {
        Arc::new_cyclic(|weak| Self {
            name: SysStr::from(""),
            is_root: true,
            ino: alloc_sysfs_ino(),
            parent: Once::new(),
            children: RwMutex::new(BTreeMap::new()),
            attrs: Vec::new(),
            ops: Once::new(),
            weak: weak.clone(),
        })
    }

    /// Creates a directory with no attributes, not yet attached anywhere.
    ///
    /// # Panics
    ///
    /// Panics if `name` is not a valid sysfs node name.
    pub(crate) fn new(name: SysStr) -> Arc<Self> {
        assert!(is_valid_name(&name), "invalid SysTree node name");
        Arc::new_cyclic(|weak| Self {
            name,
            is_root: false,
            ino: alloc_sysfs_ino(),
            parent: Once::new(),
            children: RwMutex::new(BTreeMap::new()),
            attrs: Vec::new(),
            ops: Once::new(),
            weak: weak.clone(),
        })
    }

    /// Creates a directory carrying fixed control files, not yet attached.
    ///
    /// # Panics
    ///
    /// Panics if `name` is not a valid sysfs node name.
    pub(crate) fn with_attrs(
        name: SysStr,
        attrs: &[(&'static str, SysPerms)],
    ) -> Result<Arc<Self>> {
        assert!(is_valid_name(&name), "invalid SysTree node name");
        let mut entries = Vec::with_capacity(attrs.len());
        for (attr_name, perms) in attrs {
            if !is_valid_name(attr_name) {
                return Err(Error::InvalidName);
            }
            if entries.iter().any(|e: &DirAttrEntry| e.name == *attr_name) {
                return Err(Error::NameConflict);
            }
            entries.push(DirAttrEntry {
                name: *attr_name,
                perms: *perms,
                ino: alloc_sysfs_ino(),
            });
        }
        Ok(Arc::new_cyclic(|weak| Self {
            name,
            is_root: false,
            ino: alloc_sysfs_ino(),
            parent: Once::new(),
            children: RwMutex::new(BTreeMap::new()),
            attrs: entries,
            ops: Once::new(),
            weak: weak.clone(),
        }))
    }

    /// Installs the callbacks behind this directory's control files.
    pub(crate) fn set_ops(&self, ops: Box<dyn DirAttrOps>) {
        self.ops.call_once(|| ops);
    }

    /// Records this directory's parent in the tree.
    pub(crate) fn init_parent(&self, parent: TreeParent) {
        self.parent.call_once(|| parent);
    }

    /// Attaches a subdirectory and initializes its parent pointer.
    pub(crate) fn attach_dir(&self, child: &Arc<Dir>) -> Result<()> {
        child.init_parent(TreeParent::Dir(self.weak.clone()));
        self.attach_child(child.clone())
    }

    /// Reads an attribute's text (used by VFS nodes and tests).
    pub(crate) fn show_attr(&self, name: &str) -> Result<String> {
        let entry = self
            .attrs
            .iter()
            .find(|e| e.name == name)
            .ok_or(Error::NotFound)?;
        if !entry.perms.is_readable() {
            return Err(Error::Attribute);
        }
        let ops = self.ops.get().ok_or(Error::NotFound)?;
        let mut out = String::new();
        ops.show(name, &mut out)?;
        Ok(out)
    }

    /// Writes text to a control attribute (used by VFS nodes and tests).
    pub(crate) fn store_attr(&self, name: &str, value: &str) -> Result<()> {
        let entry = self
            .attrs
            .iter()
            .find(|e| e.name == name)
            .ok_or(Error::NotFound)?;
        if !entry.perms.is_writable() {
            return Err(Error::Attribute);
        }
        let ops = self.ops.get().ok_or(Error::NotFound)?;
        ops.store(name, value)
    }

    /// Lists the names of all fixed control attributes in this directory.
    #[cfg_attr(not(ktest), allow(dead_code))]
    pub(crate) fn attr_names(&self) -> Vec<String> {
        self.attrs.iter().map(|e| e.name.to_string()).collect()
    }
}

impl SysTreeEdit for Dir {
    fn attach_child(&self, child: Arc<dyn SysNode>) -> Result<()> {
        let name = SysStr::from(child.name().to_string());
        if self.attrs.iter().any(|e| e.name == name.as_ref()) {
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

    fn tree_path(&self) -> String {
        if self.is_root {
            return String::from("/");
        }
        let Some(parent_path) = self.parent.get().and_then(TreeParent::tree_path) else {
            return alloc::format!("/{}", self.name);
        };
        if parent_path == "/" {
            alloc::format!("/{}", self.name)
        } else {
            alloc::format!("{}/{}", parent_path, self.name)
        }
    }

    fn as_tree_parent(&self) -> TreeParent {
        TreeParent::Dir(self.weak.clone())
    }
}

impl SysNode for Dir {
    fn name(&self) -> &str {
        self.name.as_ref()
    }

    fn ino(&self) -> usize {
        self.ino
    }

    fn is_dir(&self) -> bool {
        true
    }

    fn get_child(&self, name: &str) -> Option<Arc<dyn SysNode>> {
        if let Some(child) = self.children.read().get(name) {
            return Some(Arc::clone(child));
        }
        let entry = self.attrs.iter().find(|e| e.name == name)?;
        Some(Arc::new(DirAttrSysNode {
            dir: self.weak.clone(),
            entry: *entry,
        }))
    }

    fn list_children(&self) -> Vec<(String, bool)> {
        let mut items: Vec<(String, bool)> = self
            .children
            .read()
            .iter()
            .map(|(k, v)| (k.to_string(), v.is_dir()))
            .collect();
        for entry in &self.attrs {
            items.push((entry.name.to_string(), false));
        }
        items
    }
}

impl core::fmt::Debug for Dir {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Dir")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

struct DirAttrSysNode {
    dir: Weak<Dir>,
    entry: DirAttrEntry,
}

impl SysNode for DirAttrSysNode {
    fn name(&self) -> &str {
        self.entry.name
    }

    fn ino(&self) -> usize {
        self.entry.ino
    }

    fn is_dir(&self) -> bool {
        false
    }

    fn is_readable(&self) -> bool {
        self.entry.perms.is_readable()
    }

    fn is_writable(&self) -> bool {
        self.entry.perms.is_writable()
    }

    fn read_attr(&self) -> KernelResult<Vec<u8>> {
        let dir = self.dir.upgrade().ok_or(Errno::ENOENT)?;
        dir.show_attr(self.entry.name)
            .map(String::into_bytes)
            .map_err(Into::into)
    }

    fn write_attr(&self, buf: &[u8]) -> KernelResult<usize> {
        let dir = self.dir.upgrade().ok_or(Errno::ENOENT)?;
        let text = core::str::from_utf8(buf).map_err(|_| Errno::EINVAL)?;
        dir.store_attr(self.entry.name, text).map_err(Errno::from)?;
        Ok(buf.len())
    }
}

/// Dynamic sysfs leaf node projecting a device's registered attribute.
pub(crate) struct DeviceAttrSysNode {
    pub(crate) dev: Weak<dyn AnyDevice>,
    pub(crate) name: SysStr,
    pub(crate) perms: SysPerms,
    pub(crate) ino: usize,
}

impl SysNode for DeviceAttrSysNode {
    fn name(&self) -> &str {
        self.name.as_ref()
    }

    fn ino(&self) -> usize {
        self.ino
    }

    fn is_dir(&self) -> bool {
        false
    }

    fn is_readable(&self) -> bool {
        self.perms.is_readable()
    }

    fn is_writable(&self) -> bool {
        self.perms.is_writable()
    }

    fn read_attr(&self) -> KernelResult<Vec<u8>> {
        let dev = self.dev.upgrade().ok_or(Errno::ENODEV)?;
        if !dev.base().is_added() {
            return Err(Errno::ENODEV);
        }
        dev.base()
            .attrs()
            .show_string(dev.as_ref(), self.name.as_ref())
            .map(String::into_bytes)
            .map_err(Into::into)
    }

    fn write_attr(&self, buf: &[u8]) -> KernelResult<usize> {
        let dev = self.dev.upgrade().ok_or(Errno::ENODEV)?;
        if !dev.base().is_added() {
            return Err(Errno::ENODEV);
        }
        let text = core::str::from_utf8(buf).map_err(|_| Errno::EINVAL)?;
        dev.base()
            .attrs()
            .store_str(dev.as_ref(), self.name.as_ref(), text)
            .map_err(Errno::from)?;
        Ok(buf.len())
    }
}

/// A symbolic link in the device sysfs tree.
#[derive(Debug)]
pub(crate) struct SymlinkNode {
    name: SysStr,
    ino: usize,
    target: String,
}

impl SymlinkNode {
    /// Creates a symlink with a literal target.
    ///
    /// # Panics
    ///
    /// Panics if `name` is not a valid sysfs node name.
    pub(crate) fn new(name: SysStr, target: String) -> Arc<Self> {
        assert!(is_valid_name(&name), "invalid SysTree symlink name");
        Arc::new(Self {
            name,
            ino: alloc_sysfs_ino(),
            target,
        })
    }
}

impl SysNode for SymlinkNode {
    fn name(&self) -> &str {
        self.name.as_ref()
    }

    fn ino(&self) -> usize {
        self.ino
    }

    fn is_dir(&self) -> bool {
        false
    }

    fn is_symlink(&self) -> bool {
        true
    }

    fn readlink(&self) -> KernelResult<String> {
        Ok(self.target.clone())
    }
}
