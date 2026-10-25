// SPDX-License-Identifier: GPL-2.0

//! System Filesystem (`/sys`).
//!
//! Exports device, driver, and kernel-subsystem information to user space
//! in the standard Linux hierarchy. Layout — matching the Asterinas sysfs
//! reference (`kernel/core/src/fs/fs_impls/sysfs`):
//!
//! - [`fs`] — the `FileSystem` instance mounted at `/sys`,
//! - [`kobject`] — the kernel object tree backing registrations,
//! - [`inode`] — the VFS projection of the tree,
//! - [`kernel`] — fixed `/sys/kernel` attributes,
//! - [`block`] — the `/sys/block` and `/sys/class/block` subsystem.
//!
//! Content registration is boot-time work ([`init`]); mounting never
//! mutates the tree.

pub mod block;
pub mod dev;
pub mod drm;
pub mod fs;
pub mod inode;
pub mod input;
pub mod kernel;
pub mod kobject;
pub mod tty;

pub use fs::SysFs;
pub use inode::{SysFsAttrNodeOps, SysFsDirFileOps, SysFsDirNodeOps, SysFsSymlinkNodeOps};
pub use kobject::{
    SysAttrNode, SysAttrRwNode, SysDirNode, SysNode, SysStaticAttrNode, SysSymlinkNode, SysTree,
    sys_tree,
};

/// Registers the fixed sysfs content; called once from `fs::init`.
///
/// Idempotent by design — subsystems may be mounted before or after
/// user-space mounts, and repeated calls only re-seed static attributes.
pub fn init() {
    kernel::init();
}

#[cfg(ktest)]
mod tests {
    use super::*;
    use crate::fs::vfs::fs::FileSystem;
    use crate::fs::vfs::{INode, OpenFlags};
    use alloc::{sync::Arc, vec::Vec};
    use ostd::prelude::ktest;
    use spin::RwLock;

    fn root() -> Arc<INode> {
        init();
        SysFs::new().root().expect("sysfs root")
    }

    #[ktest]
    fn sysfs_hierarchy_and_synthetic_files() {
        let root = root();

        // Verify /sys/class, /sys/devices, /sys/kernel exist
        let class = root.node_ops.lookup("class").expect("lookup class");
        let devices = root.node_ops.lookup("devices").expect("lookup devices");
        let kernel = root.node_ops.lookup("kernel").expect("lookup kernel");

        assert!(class.is_dir());
        assert!(devices.is_dir());
        assert!(kernel.is_dir());

        // Verify /sys/kernel/uevent_seqnum content
        let uevent = kernel
            .node_ops
            .lookup("uevent_seqnum")
            .expect("lookup uevent");
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
        SysTree::register_device_class("net", "eth_test", Arc::clone(&eth) as Arc<dyn SysNode>)
            .expect("register eth_test");
        let _ = &eth;

        // Now lookup through VFS
        let sysfs = SysFs::new();
        let root = sysfs.root().expect("sysfs root");
        let class = root.node_ops.lookup("class").expect("lookup class");
        let net = class.node_ops.lookup("net").expect("lookup net");
        let eth_test = net.node_ops.lookup("eth_test").expect("lookup eth_test");
        assert!(eth_test.is_dir());

        // Read address attribute
        let addr_node = eth_test.node_ops.lookup("address").expect("lookup address");
        let addr_file = addr_node
            .node_ops
            .open(OpenFlags::READ)
            .expect("open address");
        let mut buf = [0u8; 32];
        let n = addr_file.read(0, &mut buf).expect("read address");
        assert_eq!(&buf[..n], b"02:00:00:12:34:56\n");

        // Read initial operstate
        let state_node = eth_test
            .node_ops
            .lookup("operstate")
            .expect("lookup operstate");
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
        let state_file2 = state_node
            .node_ops
            .open(OpenFlags::READ)
            .expect("open operstate 2");
        let sn2 = state_file2.read(0, &mut sbuf).expect("read operstate 2");
        assert_eq!(&sbuf[..sn2], b"up\n");

        // Verify symlink
        let dev_link = eth_test
            .node_ops
            .lookup("device")
            .expect("lookup device symlink");
        assert!(dev_link.is_symlink());
        let target = dev_link.node_ops.readlink().expect("readlink");
        assert_eq!(target, "../../../devices/virtual");
    }

    #[ktest]
    fn sysfs_dev_char_and_class_discovery() {
        let root = root();

        // 1. Verify /sys/dev/char and /sys/dev/block directories exist
        let dev_dir = root.node_ops.lookup("dev").expect("lookup dev");
        assert!(dev_dir.is_dir());

        let char_dir = dev_dir.node_ops.lookup("char").expect("lookup dev/char");
        assert!(char_dir.is_dir());

        let block_dir = dev_dir.node_ops.lookup("block").expect("lookup dev/block");
        assert!(block_dir.is_dir());

        // 2. Verify static char symlinks in /sys/dev/char
        let tty_link = char_dir.node_ops.lookup("5:0").expect("lookup 5:0");
        assert!(tty_link.is_symlink());
        assert_eq!(tty_link.node_ops.readlink().expect("readlink 5:0"), "../../class/tty/tty");

        let null_link = char_dir.node_ops.lookup("1:3").expect("lookup 1:3");
        assert!(null_link.is_symlink());
        assert_eq!(null_link.node_ops.readlink().expect("readlink 1:3"), "../../class/mem/null");

        let drm_link = char_dir.node_ops.lookup("226:0").expect("lookup 226:0");
        assert!(drm_link.is_symlink());
        assert_eq!(drm_link.node_ops.readlink().expect("readlink 226:0"), "../../class/drm/card0");

        let render_link = char_dir.node_ops.lookup("226:128").expect("lookup 226:128");
        assert!(render_link.is_symlink());
        assert_eq!(render_link.node_ops.readlink().expect("readlink 226:128"), "../../class/drm/renderD128");

        let mice_link = char_dir.node_ops.lookup("13:63").expect("lookup 13:63");
        assert!(mice_link.is_symlink());
        assert_eq!(mice_link.node_ops.readlink().expect("readlink 13:63"), "../../class/input/mice");

        // 3. Verify /sys/class/drm and /sys/class/input exist
        let class = root.node_ops.lookup("class").expect("lookup class");
        let drm_class = class.node_ops.lookup("drm").expect("lookup class/drm");
        assert!(drm_class.is_dir());

        let input_class = class.node_ops.lookup("input").expect("lookup class/input");
        assert!(input_class.is_dir());

        let tty_class = class.node_ops.lookup("tty").expect("lookup class/tty");
        assert!(tty_class.is_dir());

        let mem_class = class.node_ops.lookup("mem").expect("lookup class/mem");
        assert!(mem_class.is_dir());

        // Verify /sys/class/mem/null attribute dev
        let mem_null = mem_class.node_ops.lookup("null").expect("lookup null");
        let dev_attr = mem_null.node_ops.lookup("dev").expect("lookup null dev");
        let dev_file = dev_attr.node_ops.open(OpenFlags::READ).expect("open null dev");
        let mut buf = [0u8; 16];
        let n = dev_file.read(0, &mut buf).expect("read null dev");
        assert_eq!(&buf[..n], b"1:3\n");
    }
}
