// SPDX-License-Identifier: GPL-2.0

//! Kernel object tree for sysfs (`kobject` / `kset` equivalent).
//!
//! Subsystem and driver code registers nodes in this in-memory hierarchy;
//! the VFS projection ([`super::inode`]) turns them into `/sys/*` files. The
//! split mirrors Linux: `kobject` state lives independently of sysfs, while
//! sysfs is just one interface over the same tree.
//!
//! The tree stores three shapes:
//!
//! - [`SysDirNode`] — a branch that own children,
//! - attribute nodes (`SysAttrNode`, `SysAttrRwNode`, `SysStaticAttrNode`)
//!   — leaf "files" whose content is produced by the owning subsystem,
//! - [`SysSymlinkNode`] — a cross-tree pointer shown as a symlink.

use alloc::{collections::BTreeMap, string::String, sync::Arc, vec::Vec};
use core::sync::atomic::{AtomicUsize, Ordering};
use spin::{Once, RwLock};

use crate::api::errno::Result;

static NEXT_SYSFS_INO: AtomicUsize = AtomicUsize::new(1000);

/// Allocates the next unique sysfs inode number; 1.. is reserved for the
/// root (see `SysTree::new`).
pub fn alloc_sysfs_ino() -> usize {
    NEXT_SYSFS_INO.fetch_add(1, Ordering::Relaxed)
}

/// A node in the kernel object system tree.
pub trait SysNode: Send + Sync + 'static {
    /// Node name as it appears in its parent directory.
    fn name(&self) -> &str;

    /// Stable inode number for this node.
    fn ino(&self) -> usize {
        1000
    }

    /// Whether this node is a directory branch.
    fn is_dir(&self) -> bool;

    /// Whether this node is a symbolic link.
    fn is_symlink(&self) -> bool {
        false
    }

    /// Attribute content, only meaningful for attribute nodes.
    fn read_attr(&self) -> Result<Vec<u8>> {
        crate::return_errno!(EISDIR, "not an attribute file");
    }

    /// Stores into an attribute node; read-only by default.
    fn write_attr(&self, _buf: &[u8]) -> Result<usize> {
        crate::return_errno!(EACCES, "attribute is read-only");
    }

    /// Whether this attribute accepts writes.
    fn is_writable(&self) -> bool {
        false
    }

    /// Link target, only meaningful for symlink nodes.
    fn readlink(&self) -> Result<String> {
        crate::return_errno!(EINVAL, "not a symbolic link");
    }

    /// Child lookup of a directory node.
    fn get_child(&self, _name: &str) -> Option<Arc<dyn SysNode>> {
        None
    }

    /// Child names of a directory node with their type flags.
    fn list_children(&self) -> Vec<(String, bool)> {
        Vec::new()
    }
}

/// Child container tracking the concrete type for safe downcasting.
enum Child {
    Dir(Arc<SysDirNode>),
    Other(Arc<dyn SysNode>),
}

impl Child {
    /// Returns the parent-tree projection of the node.
    fn node(&self) -> Arc<dyn SysNode> {
        match self {
            Child::Dir(dir) => dir.clone(),
            Child::Other(node) => Arc::clone(node),
        }
    }
}

/// Directory branch of the kernel object tree.
///
/// Attributes, symlinks and further directories go through this one map;
/// the typed helpers below decide the shape registered, so callers never
/// touch the map directly (keeping validation — e.g. duplicate names — in
/// one place).
pub struct SysDirNode {
    name: String,
    ino: usize,
    children: RwLock<BTreeMap<String, Child>>,
}

impl SysDirNode {
    /// Inode number fixed at construction time.
    pub const fn ino(&self) -> usize {
        self.ino
    }

    /// Creates a new directory node with an allocated inode number.
    pub fn new(name: &str) -> Arc<Self> {
        Self::new_with_ino(name, alloc_sysfs_ino())
    }

    /// Creates a new directory node with a specific inode number (used for
    /// reserved numbers such as the sysfs root, inode 1).
    pub fn new_with_ino(name: &str, ino: usize) -> Arc<Self> {
        Arc::new(Self {
            name: String::from(name),
            ino,
            children: RwLock::new(BTreeMap::new()),
        })
    }

    /// Adds an already-backed child node under its own name, rejecting
    /// duplicates with `EEXIST`.
    pub fn add_child(&self, child: Arc<dyn SysNode>) -> Result<()> {
        let name = String::from(child.name());
        self.insert_child(&name, child)
    }

    /// Adds an already-backed child node under an explicit map name.
    ///
    /// Device class/bus registration uses this to key children by their
    /// user-visible device name (node names may differ).
    pub fn insert_child(&self, name: &str, child: Arc<dyn SysNode>) -> Result<()> {
        let mut children = self.children.write();
        if children.contains_key(name) {
            crate::return_errno!(EEXIST, "sysfs entry already exists");
        }
        children.insert(String::from(name), Child::Other(child));
        Ok(())
    }

    /// Adds (or returns the existing) subdirectory `name`.
    pub fn add_dir(&self, name: &str) -> Arc<SysDirNode> {
        if let Some(Child::Dir(existing)) = self.children.read().get(name) {
            return Arc::clone(existing);
        }
        let dir = SysDirNode::new(name);
        self.children
            .write()
            .insert(String::from(name), Child::Dir(Arc::clone(&dir)));
        dir
    }

    /// Adds a read-only dynamic attribute.
    pub fn add_attr<F>(&self, name: &str, show: F) -> Arc<SysAttrNode<F>>
    where
        F: Fn() -> Vec<u8> + Send + Sync + 'static,
    {
        let attr = Arc::new(SysAttrNode::new(name, show));
        self.children
            .write()
            .insert(String::from(name), Child::Other(attr.clone()));
        attr
    }

    /// Adds a read-write attribute backed by user callbacks.
    pub fn add_attr_rw<R, W>(&self, name: &str, show: R, store: W) -> Arc<SysAttrRwNode<R, W>>
    where
        R: Fn() -> Vec<u8> + Send + Sync + 'static,
        W: Fn(&[u8]) -> Result<usize> + Send + Sync + 'static,
    {
        let attr = Arc::new(SysAttrRwNode::new(name, show, store));
        self.children
            .write()
            .insert(String::from(name), Child::Other(attr.clone()));
        attr
    }

    /// Adds an attribute with fixed content.
    pub fn add_static_attr(&self, name: &str, content: &'static [u8]) -> Arc<SysStaticAttrNode> {
        let attr = Arc::new(SysStaticAttrNode::new(name, content));
        self.children
            .write()
            .insert(String::from(name), Child::Other(attr.clone()));
        attr
    }

    /// Adds a symlink pointing (link-form-relative) at `target`.
    pub fn add_symlink(&self, name: &str, target: &str) -> Arc<SysSymlinkNode> {
        let sym = Arc::new(SysSymlinkNode::new(name, target));
        self.children
            .write()
            .insert(String::from(name), Child::Other(sym.clone()));
        sym
    }

    /// Removes a child by name, returning the removed node.
    pub fn remove_child(&self, name: &str) -> Option<Arc<dyn SysNode>> {
        self.children.write().remove(name).map(|child| child.node())
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
        self.children.read().get(name).map(|child| child.node())
    }

    fn list_children(&self) -> Vec<(String, bool)> {
        self.children
            .read()
            .iter()
            .map(|(k, v)| (k.clone(), matches!(v, Child::Dir(_))))
            .collect()
    }
}

/// Read-only attribute with a dynamic generator closure.
pub struct SysAttrNode<F: Fn() -> Vec<u8> + Send + Sync + 'static> {
    name: String,
    ino: usize,
    show: F,
}

impl<F: Fn() -> Vec<u8> + Send + Sync + 'static> SysAttrNode<F> {
    /// Creates the attribute; the generator is invoked per open.
    pub fn new(name: &str, show: F) -> Self {
        Self {
            name: String::from(name),
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

/// Read-write attribute with generator and store callbacks.
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
    /// Creates the attribute; both callbacks are owned by the node.
    pub fn new(name: &str, show: R, store: W) -> Self {
        Self {
            name: String::from(name),
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

/// Attribute with fixed content.
pub struct SysStaticAttrNode {
    name: String,
    ino: usize,
    content: &'static [u8],
}

impl SysStaticAttrNode {
    /// Creates the attribute.
    pub fn new(name: &str, content: &'static [u8]) -> Self {
        Self {
            name: String::from(name),
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

/// Symbolic link node pointing at a tree-relative `target`.
pub struct SysSymlinkNode {
    name: String,
    ino: usize,
    target: String,
}

impl SysSymlinkNode {
    /// Creates the symlink.
    pub fn new(name: &str, target: &str) -> Self {
        Self {
            name: String::from(name),
            ino: alloc_sysfs_ino(),
            target: String::from(target),
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

/// In-memory tree node of `/sys`.
///
/// Accessing branches happens through typed accessors — registration stays
/// in the [`SysDirNode`] methods so duplicate-name rules cannot be dodged.
pub struct SysTree {
    root_dir: Arc<SysDirNode>,
    class_dir: Arc<SysDirNode>,
    devices_dir: Arc<SysDirNode>,
    bus_dir: Arc<SysDirNode>,
    fs_dir: Arc<SysDirNode>,
    kernel_dir: Arc<SysDirNode>,
}

impl SysTree {
    fn new() -> Self {
        let root_dir = SysDirNode::new_with_ino("", 1);

        let block_dir: Arc<dyn SysNode> = Arc::new(super::block::SysBlockRootDirNode::new());
        let _ = root_dir.add_child(block_dir);

        let devices_dir = root_dir.add_dir("devices");
        let _ = devices_dir.add_dir("system");
        let _ = devices_dir.add_dir("virtual");

        let bus_dir = root_dir.add_dir("bus");
        let _ = bus_dir.add_dir("pci");
        let _ = bus_dir.add_dir("usb");

        let fs_dir = root_dir.add_dir("fs");
        let kernel_dir = root_dir.add_dir("kernel");

        let class_dir = root_dir.add_dir("class");
        let class_block_dir: Arc<dyn SysNode> = Arc::new(super::block::SysClassBlockDirNode::new());
        let _ = class_dir.add_child(class_block_dir);
        let _ = class_dir.add_dir("net");
        let _ = class_dir.add_dir("tty");

        Self {
            root_dir,
            class_dir,
            devices_dir,
            bus_dir,
            fs_dir,
            kernel_dir,
        }
    }

    /// Accesses the tree root.
    pub fn root_dir(&self) -> &Arc<SysDirNode> {
        &self.root_dir
    }

    /// Accesses the `/sys/class` branch.
    pub fn class_dir(&self) -> &Arc<SysDirNode> {
        &self.class_dir
    }

    /// Accesses the `/sys/devices` branch.
    pub fn devices_dir(&self) -> &Arc<SysDirNode> {
        &self.devices_dir
    }

    /// Accesses the `/sys/bus` branch.
    pub fn bus_dir(&self) -> &Arc<SysDirNode> {
        &self.bus_dir
    }

    /// Accesses the `/sys/fs` branch.
    pub fn fs_dir(&self) -> &Arc<SysDirNode> {
        &self.fs_dir
    }

    /// Accesses the `/sys/kernel` branch.
    pub fn kernel_dir(&self) -> &Arc<SysDirNode> {
        &self.kernel_dir
    }

    /// Registers a device under `/sys/class/<class_name>/<dev_name>`.
    pub fn register_device_class(
        class_name: &str,
        dev_name: &str,
        node: Arc<dyn SysNode>,
    ) -> Result<()> {
        let tree = sys_tree();
        let class = tree.class_dir.add_dir(class_name);
        class.insert_child(dev_name, node)
    }

    /// Registers a bus device under `/sys/bus/<bus_name>/devices/<dev_name>`.
    pub fn register_bus_device(
        bus_name: &str,
        dev_name: &str,
        node: Arc<dyn SysNode>,
    ) -> Result<()> {
        let tree = sys_tree();
        let bus = tree.bus_dir.add_dir(bus_name);
        let devices = bus.add_dir("devices");
        devices.insert_child(dev_name, node)
    }
}

static SYS_TREE: Once<SysTree> = Once::new();

/// Returns the singleton kernel object system tree.
pub fn sys_tree() -> &'static SysTree {
    SYS_TREE.call_once(SysTree::new)
}
