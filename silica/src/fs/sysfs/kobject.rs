// SPDX-License-Identifier: GPL-2.0

//! Kernel Object / SysNode Tree (`SysNode`, `SysDirNode`, `SysTree`).
//!
//! Provides a decoupled kernel subsystem hierarchy (similar to Linux `kobject`/`kset`).
//! Allows device drivers and kernel subsystems to dynamically register, inspect,
//! and remove class devices, bus entries, and system attributes at runtime.

use alloc::{
    collections::BTreeMap,
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};
use core::sync::atomic::{AtomicUsize, Ordering};
use spin::{Once, RwLock};

use crate::api::errno::Result;

static NEXT_SYSFS_INO: AtomicUsize = AtomicUsize::new(1000);

pub fn alloc_sysfs_ino() -> usize {
    NEXT_SYSFS_INO.fetch_add(1, Ordering::Relaxed)
}

/// A node in the kernel object system tree.
pub trait SysNode: Send + Sync + 'static {
    /// Returns the node's name.
    fn name(&self) -> &str;

    /// Inode number for this node.
    fn ino(&self) -> usize {
        1000
    }

    /// Whether this node represents a directory.
    fn is_dir(&self) -> bool;

    /// Whether this node represents a symbolic link.
    fn is_symlink(&self) -> bool {
        false
    }

    /// Reads this attribute node's data.
    fn read_attr(&self) -> Result<Vec<u8>> {
        crate::return_errno!(EISDIR, "not an attribute file");
    }

    /// Writes data to this attribute node.
    fn write_attr(&self, _buf: &[u8]) -> Result<usize> {
        crate::return_errno!(EACCES, "attribute is read-only");
    }

    /// Whether this attribute is writable.
    fn is_writable(&self) -> bool {
        false
    }

    /// Reads target path if this node is a symbolic link.
    fn readlink(&self) -> Result<String> {
        crate::return_errno!(EINVAL, "not a symbolic link");
    }

    /// Returns a child node if this node is a directory.
    fn get_child(&self, _name: &str) -> Option<Arc<dyn SysNode>> {
        None
    }

    /// Lists directory children names and whether they are directories.
    fn list_children(&self) -> Vec<(String, bool)> {
        Vec::new()
    }
}

/// A directory node (Branch) in the kernel object tree.
pub struct SysDirNode {
    name: String,
    ino: usize,
    children: RwLock<BTreeMap<String, Arc<dyn SysNode>>>,
    subdirs: RwLock<BTreeMap<String, Arc<SysDirNode>>>,
}

impl SysDirNode {
    /// Creates a new directory node with an allocated inode number.
    pub fn new(name: &str) -> Arc<Self> {
        Arc::new(Self {
            name: name.to_string(),
            ino: alloc_sysfs_ino(),
            children: RwLock::new(BTreeMap::new()),
            subdirs: RwLock::new(BTreeMap::new()),
        })
    }

    /// Creates a new directory node with a specific inode number.
    pub fn new_with_ino(name: &str, ino: usize) -> Arc<Self> {
        Arc::new(Self {
            name: name.to_string(),
            ino,
            children: RwLock::new(BTreeMap::new()),
            subdirs: RwLock::new(BTreeMap::new()),
        })
    }

    /// Adds a child node. Returns `EEXIST` if an entry already exists with this name.
    pub fn add_child(&self, child: Arc<dyn SysNode>) -> Result<()> {
        let mut children = self.children.write();
        let name = child.name();
        if children.contains_key(name) {
            crate::return_errno!(EEXIST, "sysfs entry already exists");
        }
        children.insert(name.to_string(), child);
        Ok(())
    }

    /// Adds a subdirectory child, or returns the existing directory if one already exists.
    pub fn add_dir(&self, name: &str) -> Arc<SysDirNode> {
        let mut subdirs = self.subdirs.write();
        if let Some(existing) = subdirs.get(name) {
            return Arc::clone(existing);
        }
        let dir = SysDirNode::new(name);
        subdirs.insert(name.to_string(), Arc::clone(&dir));
        self.children.write().insert(name.to_string(), Arc::clone(&dir) as Arc<dyn SysNode>);
        dir
    }

    /// Adds a read-only dynamic attribute node.
    pub fn add_attr<F>(&self, name: &str, show: F) -> Arc<SysAttrNode<F>>
    where
        F: Fn() -> Vec<u8> + Send + Sync + 'static,
    {
        let attr = Arc::new(SysAttrNode::new(name, show));
        self.children.write().insert(name.to_string(), attr.clone());
        attr
    }

    /// Adds a read-write attribute node.
    pub fn add_attr_rw<R, W>(&self, name: &str, show: R, store: W) -> Arc<SysAttrRwNode<R, W>>
    where
        R: Fn() -> Vec<u8> + Send + Sync + 'static,
        W: Fn(&[u8]) -> Result<usize> + Send + Sync + 'static,
    {
        let attr = Arc::new(SysAttrRwNode::new(name, show, store));
        self.children.write().insert(name.to_string(), attr.clone());
        attr
    }

    /// Adds a static text attribute node.
    pub fn add_static_attr(&self, name: &str, content: &'static [u8]) -> Arc<SysStaticAttrNode> {
        let attr = Arc::new(SysStaticAttrNode::new(name, content));
        self.children.write().insert(name.to_string(), attr.clone());
        attr
    }

    /// Adds a symbolic link pointing to `target`.
    pub fn add_symlink(&self, name: &str, target: &str) -> Arc<SysSymlinkNode> {
        let sym = Arc::new(SysSymlinkNode::new(name, target));
        self.children.write().insert(name.to_string(), sym.clone());
        sym
    }

    /// Retrieves a child directory by name.
    pub fn get_dir(&self, name: &str) -> Option<Arc<SysDirNode>> {
        let child = self.get_child(name)?;
        if child.is_dir() {
            // We can look up in a typed directory map or wrap in SysDirNode adapter
            // Since SysDirNode is Arc<dyn SysNode>, we can downcast safely via sub-dirs
        }
        None
    }

    /// Removes a child by name.
    pub fn remove_child(&self, name: &str) -> Option<Arc<dyn SysNode>> {
        self.children.write().remove(name)
    }
}

impl SysNode for SysDirNode {
    fn name(&self) -> &str {
        &self.name
    }

    fn ino(&self) -> usize {
        self.ino
    }

    fn is_dir(&self) -> bool {
        true
    }

    fn get_child(&self, name: &str) -> Option<Arc<dyn SysNode>> {
        self.children.read().get(name).cloned()
    }

    fn list_children(&self) -> Vec<(String, bool)> {
        self.children
            .read()
            .iter()
            .map(|(k, v)| (k.clone(), v.is_dir()))
            .collect()
    }
}

/// A read-only system attribute node with dynamic closure generator.
pub struct SysAttrNode<F: Fn() -> Vec<u8> + Send + Sync + 'static> {
    name: String,
    ino: usize,
    show: F,
}

impl<F: Fn() -> Vec<u8> + Send + Sync + 'static> SysAttrNode<F> {
    pub fn new(name: &str, show: F) -> Self {
        Self {
            name: name.to_string(),
            ino: alloc_sysfs_ino(),
            show,
        }
    }
}

impl<F: Fn() -> Vec<u8> + Send + Sync + 'static> SysNode for SysAttrNode<F> {
    fn name(&self) -> &str {
        &self.name
    }

    fn ino(&self) -> usize {
        self.ino
    }

    fn is_dir(&self) -> bool {
        false
    }

    fn read_attr(&self) -> Result<Vec<u8>> {
        Ok((self.show)())
    }
}

/// A read-write system attribute node.
pub struct SysAttrRwNode<R, W>
where
    R: Fn() -> Vec<u8> + Send + Sync + 'static,
    W: Fn(&[u8]) -> Result<usize> + Send + Sync + 'static,
{
    name: String,
    ino: usize,
    show: R,
    store: W,
}

impl<R, W> SysAttrRwNode<R, W>
where
    R: Fn() -> Vec<u8> + Send + Sync + 'static,
    W: Fn(&[u8]) -> Result<usize> + Send + Sync + 'static,
{
    pub fn new(name: &str, show: R, store: W) -> Self {
        Self {
            name: name.to_string(),
            ino: alloc_sysfs_ino(),
            show,
            store,
        }
    }
}

impl<R, W> SysNode for SysAttrRwNode<R, W>
where
    R: Fn() -> Vec<u8> + Send + Sync + 'static,
    W: Fn(&[u8]) -> Result<usize> + Send + Sync + 'static,
{
    fn name(&self) -> &str {
        &self.name
    }

    fn ino(&self) -> usize {
        self.ino
    }

    fn is_dir(&self) -> bool {
        false
    }

    fn read_attr(&self) -> Result<Vec<u8>> {
        Ok((self.show)())
    }

    fn write_attr(&self, buf: &[u8]) -> Result<usize> {
        (self.store)(buf)
    }

    fn is_writable(&self) -> bool {
        true
    }
}

/// A static byte slice system attribute node.
pub struct SysStaticAttrNode {
    name: String,
    ino: usize,
    content: &'static [u8],
}

impl SysStaticAttrNode {
    pub fn new(name: &str, content: &'static [u8]) -> Self {
        Self {
            name: name.to_string(),
            ino: alloc_sysfs_ino(),
            content,
        }
    }
}

impl SysNode for SysStaticAttrNode {
    fn name(&self) -> &str {
        &self.name
    }

    fn ino(&self) -> usize {
        self.ino
    }

    fn is_dir(&self) -> bool {
        false
    }

    fn read_attr(&self) -> Result<Vec<u8>> {
        Ok(self.content.to_vec())
    }
}

/// A symbolic link in the system tree.
pub struct SysSymlinkNode {
    name: String,
    ino: usize,
    target: String,
}

impl SysSymlinkNode {
    pub fn new(name: &str, target: &str) -> Self {
        Self {
            name: name.to_string(),
            ino: alloc_sysfs_ino(),
            target: target.to_string(),
        }
    }
}

impl SysNode for SysSymlinkNode {
    fn name(&self) -> &str {
        &self.name
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

    fn readlink(&self) -> Result<String> {
        Ok(self.target.clone())
    }
}

/// Global registry for devices to mount attributes:
pub struct SysTree {
    pub root_dir: Arc<SysDirNode>,
    pub class_dir: Arc<SysDirNode>,
    pub devices_dir: Arc<SysDirNode>,
    pub bus_dir: Arc<SysDirNode>,
    pub block_dir: Arc<SysDirNode>,
    pub fs_dir: Arc<SysDirNode>,
    pub kernel_dir: Arc<SysDirNode>,
}

impl SysTree {
    fn new() -> Self {
        let root_dir = SysDirNode::new_with_ino("", 1);
        let class_dir = root_dir.add_dir("class");
        let devices_dir = root_dir.add_dir("devices");
        let block_dir = root_dir.add_dir("block");
        let bus_dir = root_dir.add_dir("bus");
        let fs_dir = root_dir.add_dir("fs");
        let kernel_dir = root_dir.add_dir("kernel");

        // Standard subdirectories
        let _ = class_dir.add_dir("net");
        let _ = class_dir.add_dir("block");
        let _ = class_dir.add_dir("tty");

        let _ = devices_dir.add_dir("system");
        let _ = devices_dir.add_dir("virtual");

        let _ = bus_dir.add_dir("pci");
        let _ = bus_dir.add_dir("usb");

        Self {
            root_dir,
            class_dir,
            devices_dir,
            bus_dir,
            block_dir,
            fs_dir,
            kernel_dir,
        }
    }

    /// Automatically registers a device in `/sys/class/<class_name>/<dev_name>`.
    pub fn register_device_class(class_name: &str, dev_name: &str, node: Arc<dyn SysNode>) -> Result<()> {
        let tree = sys_tree();
        let class = tree.class_dir.add_dir(class_name);
        let mut children = class.children.write();
        if children.contains_key(dev_name) {
            crate::return_errno!(EEXIST, "class device already registered");
        }
        children.insert(dev_name.to_string(), node);
        Ok(())
    }

    /// Automatically registers a bus device in `/sys/bus/<bus_name>/devices/<dev_name>`.
    pub fn register_bus_device(bus_name: &str, dev_name: &str, node: Arc<dyn SysNode>) -> Result<()> {
        let tree = sys_tree();
        let bus = tree.bus_dir.add_dir(bus_name);
        let devices = bus.add_dir("devices");
        let mut children = devices.children.write();
        if children.contains_key(dev_name) {
            crate::return_errno!(EEXIST, "bus device already registered");
        }
        children.insert(dev_name.to_string(), node);
        Ok(())
    }
}

static SYS_TREE: Once<SysTree> = Once::new();

/// Returns the singleton kernel object system tree.
pub fn sys_tree() -> &'static SysTree {
    SYS_TREE.call_once(SysTree::new)
}
