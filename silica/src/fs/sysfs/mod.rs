// SPDX-License-Identifier: GPL-2.0

//! System Filesystem (`/sys`).
//!
//! Provides the sysfs pseudo-filesystem exporting device, driver, and kernel subsystem
//! information to userspace according to standard Linux hierarchy.

pub mod inode;
pub mod kernel;
pub mod kobject;

use alloc::{boxed::Box, sync::Arc};

use crate::{
    api::errno::Result,
    fs::vfs::{FileSystem, INode, INodeAttr, Mode, StatFs},
};

pub use inode::{
    SysFsAttrFileOps, SysFsAttrNodeOps, SysFsDirFileOps, SysFsDirNodeOps, SysFsSymlinkNodeOps,
};
pub use kernel::init_kernel_subsystem;
pub use kobject::{
    sys_tree, SysAttrNode, SysAttrRwNode, SysDirNode, SysNode, SysStaticAttrNode, SysSymlinkNode,
    SysTree,
};

/// The SysFS pseudo-filesystem.
pub struct SysFs {
    root: Arc<INode>,
}

impl SysFs {
    /// Constructs a populated SysFs hierarchy based on the global `SysTree`.
    pub fn new() -> Self {
        init_kernel_subsystem();
        let tree = sys_tree();
        let root_inode = Arc::new(INode::new_with_attr(
            Box::new(SysFsDirNodeOps::new(Arc::clone(&tree.root_dir) as Arc<dyn SysNode>)),
            tree.root_dir.ino(),
            INodeAttr {
                size: 0,
                mode: Mode::DIR
                    | Mode::RUSR
                    | Mode::XUSR
                    | Mode::RGRP
                    | Mode::XGRP
                    | Mode::ROTH
                    | Mode::XOTH,
                nlink: 2,
                uid: 0,
                gid: 0,
                rdev: 0,
            },
        ));

        Self { root: root_inode }
    }
}

impl Default for SysFs {
    fn default() -> Self {
        Self::new()
    }
}

impl FileSystem for SysFs {
    fn name(&self) -> &'static str {
        "sysfs"
    }

    fn root(&self) -> Result<Arc<INode>> {
        Ok(Arc::clone(&self.root))
    }

    fn statfs(&self) -> Result<StatFs> {
        Ok(StatFs {
            bsize: 4096,
            blocks: 0,
            bfree: 0,
            bavail: 0,
            files: 0,
            ffree: 0,
            namelen: 255,
        })
    }
}

#[cfg(ktest)]
mod tests {
    use super::*;
    use crate::fs::vfs::OpenFlags;
    use alloc::vec::Vec;
    use ostd::prelude::ktest;
    use spin::RwLock;

    #[ktest]
    fn sysfs_hierarchy_and_synthetic_files() {
        let sysfs = SysFs::new();
        let root = sysfs.root().expect("sysfs root");

        // Verify /sys/class, /sys/devices, /sys/kernel exist
        let class = root.node_ops.lookup("class").expect("lookup class");
        let devices = root.node_ops.lookup("devices").expect("lookup devices");
        let kernel = root.node_ops.lookup("kernel").expect("lookup kernel");

        assert!(class.is_dir());
        assert!(devices.is_dir());
        assert!(kernel.is_dir());

        // Verify /sys/kernel/uevent_seqnum content
        let uevent = kernel.node_ops.lookup("uevent_seqnum").expect("lookup uevent");
        let file_ops = uevent.node_ops.open(OpenFlags::READ).expect("open uevent");
        let mut buf = [0u8; 16];
        let n = file_ops.read(0, &mut buf).expect("read uevent");
        assert_eq!(&buf[..n], b"1\n");
    }

    #[ktest]
    fn sysfs_dynamic_device_registration_and_attributes() {
        // Create virtual device eth_test
        let eth = SysDirNode::new("eth_test");
        let _ = eth.add_attr("address", || b"02:00:00:12:34:56\n".to_vec());
        let _ = eth.add_attr("mtu", || b"1500\n".to_vec());

        // Add a writable attribute
        let state = Arc::new(RwLock::new(Vec::from(b"down\n" as &[u8])));
        let state_r = state.clone();
        let state_w = state.clone();
        let _ = eth.add_attr_rw(
            "operstate",
            move || state_r.read().clone(),
            move |buf| {
                let mut guard = state_w.write();
                guard.clear();
                guard.extend_from_slice(buf);
                Ok(buf.len())
            },
        );

        // Add a symlink to /sys/devices/virtual
        let _ = eth.add_symlink("device", "../../../devices/virtual");

        // Register via SysTree::register_device_class
        SysTree::register_device_class("net", "eth_test", eth).expect("register eth_test");

        // Now lookup through VFS
        let sysfs = SysFs::new();
        let root = sysfs.root().expect("sysfs root");
        let class = root.node_ops.lookup("class").expect("lookup class");
        let net = class.node_ops.lookup("net").expect("lookup net");
        let eth_test = net.node_ops.lookup("eth_test").expect("lookup eth_test");
        assert!(eth_test.is_dir());

        // Read address attribute
        let addr_node = eth_test.node_ops.lookup("address").expect("lookup address");
        let addr_file = addr_node.node_ops.open(OpenFlags::READ).expect("open address");
        let mut buf = [0u8; 32];
        let n = addr_file.read(0, &mut buf).expect("read address");
        assert_eq!(&buf[..n], b"02:00:00:12:34:56\n");

        // Read initial operstate
        let state_node = eth_test.node_ops.lookup("operstate").expect("lookup operstate");
        let state_file = state_node
            .node_ops
            .open(OpenFlags::READ | OpenFlags::WRITE)
            .expect("open operstate");
        let mut sbuf = [0u8; 16];
        let sn = state_file.read(0, &mut sbuf).expect("read operstate");
        assert_eq!(&sbuf[..sn], b"down\n");

        // Write to operstate
        state_file.write(0, b"up\n").expect("write operstate");

        // Re-read operstate to verify it updated
        let state_file2 = state_node.node_ops.open(OpenFlags::READ).expect("open operstate 2");
        let sn2 = state_file2.read(0, &mut sbuf).expect("read operstate 2");
        assert_eq!(&sbuf[..sn2], b"up\n");

        // Verify symlink
        let dev_link = eth_test.node_ops.lookup("device").expect("lookup device symlink");
        assert!(dev_link.is_symlink());
        let target = dev_link.node_ops.readlink().expect("readlink");
        assert_eq!(target, "../../../devices/virtual");
    }
}
