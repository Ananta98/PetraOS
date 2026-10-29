// SPDX-License-Identifier: GPL-2.0

//! SysFS Block Device Subsystem (`/sys/block` and `/sys/class/block`).
//!
//! Exposes registered block devices, dynamic I/O statistics (`stat`), device geometry (`size`,
//! `dev`), and queue configurations to userspace monitoring tools such as `btop`, `lsblk`, and `iostat`.

use alloc::{format, string::String, sync::Arc, vec, vec::Vec};

use crate::{
    drivers::BlockDevice,
    fs::sysfs::kobject::{
        SysAttrNode, SysNode, SysStaticAttrNode, SysSymlinkNode, alloc_sysfs_ino,
    },
};

/// Root directory for `/sys/block`.
pub struct SysBlockRootDirNode {
    ino: usize,
}

impl SysBlockRootDirNode {
    pub fn new() -> Self {
        Self {
            ino: alloc_sysfs_ino(),
        }
    }
}

impl Default for SysBlockRootDirNode {
    fn default() -> Self {
        Self::new()
    }
}

impl SysNode for SysBlockRootDirNode {
    fn name(&self) -> &str {
        "block"
    }

    fn ino(&self) -> usize {
        self.ino
    }

    fn is_dir(&self) -> bool {
        true
    }

    fn get_child(&self, name: &str) -> Option<Arc<dyn SysNode>> {
        for dev in crate::drivers::get_blkdevs() {
            if dev.name() == name {
                return Some(Arc::new(SysBlockDevDirNode::new(dev)));
            }
        }
        None
    }

    fn list_children(&self) -> Vec<(String, bool)> {
        crate::drivers::get_blkdevs()
            .into_iter()
            .map(|d| (d.name(), true))
            .collect()
    }
}

/// Directory for an individual block device (`/sys/block/<devname>`).
pub struct SysBlockDevDirNode {
    name: String,
    dev: Arc<dyn BlockDevice>,
    ino: usize,
}

impl SysBlockDevDirNode {
    pub fn new(dev: Arc<dyn BlockDevice>) -> Self {
        let name = dev.name();
        Self {
            name,
            dev,
            ino: alloc_sysfs_ino(),
        }
    }
}

impl SysNode for SysBlockDevDirNode {
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
        match name {
            "stat" => {
                let dev = Arc::clone(&self.dev);
                Some(Arc::new(SysAttrNode::new("stat", move || {
                    let s = dev.stats();
                    format!(
                        "{:>8} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8}\n",
                        s.reads_completed,
                        s.reads_merged,
                        s.sectors_read,
                        s.read_ms,
                        s.writes_completed,
                        s.writes_merged,
                        s.sectors_written,
                        s.write_ms,
                        s.io_in_progress,
                        s.io_ms,
                        s.weighted_io_ms,
                    )
                    .into_bytes()
                })))
            }
            "size" => {
                let dev = Arc::clone(&self.dev);
                Some(Arc::new(SysAttrNode::new("size", move || {
                    let sectors = dev.block_count().saturating_mul(dev.block_size() as u64) / 512;
                    format!("{}\n", sectors).into_bytes()
                })))
            }
            "dev" => {
                let dev = Arc::clone(&self.dev);
                Some(Arc::new(SysAttrNode::new("dev", move || {
                    let (maj, min) = dev.device_number();
                    format!("{}:{}\n", maj, min).into_bytes()
                })))
            }
            "removable" => Some(Arc::new(SysStaticAttrNode::new("removable", b"0\n"))),
            "ro" => Some(Arc::new(SysStaticAttrNode::new("ro", b"0\n"))),
            "capability" => Some(Arc::new(SysStaticAttrNode::new("capability", b"0\n"))),
            "queue" => Some(Arc::new(SysBlockQueueDirNode::new(Arc::clone(&self.dev)))),
            _ => None,
        }
    }

    fn list_children(&self) -> Vec<(String, bool)> {
        vec![
            (String::from("stat"), false),
            (String::from("size"), false),
            (String::from("dev"), false),
            (String::from("removable"), false),
            (String::from("ro"), false),
            (String::from("capability"), false),
            (String::from("queue"), true),
        ]
    }
}

/// Queue attributes directory for a block device (`/sys/block/<devname>/queue`).
pub struct SysBlockQueueDirNode {
    dev: Arc<dyn BlockDevice>,
    ino: usize,
}

impl SysBlockQueueDirNode {
    pub fn new(dev: Arc<dyn BlockDevice>) -> Self {
        Self {
            dev,
            ino: alloc_sysfs_ino(),
        }
    }
}

impl SysNode for SysBlockQueueDirNode {
    fn name(&self) -> &str {
        "queue"
    }

    fn ino(&self) -> usize {
        self.ino
    }

    fn is_dir(&self) -> bool {
        true
    }

    fn get_child(&self, name: &str) -> Option<Arc<dyn SysNode>> {
        match name {
            "logical_block_size" => {
                let bs = self.dev.block_size();
                Some(Arc::new(SysAttrNode::new(
                    "logical_block_size",
                    move || format!("{}\n", bs).into_bytes(),
                )))
            }
            "physical_block_size" => {
                let bs = self.dev.block_size();
                Some(Arc::new(SysAttrNode::new(
                    "physical_block_size",
                    move || format!("{}\n", bs).into_bytes(),
                )))
            }
            "hw_sector_size" => {
                let bs = self.dev.block_size();
                Some(Arc::new(SysAttrNode::new("hw_sector_size", move || {
                    format!("{}\n", bs).into_bytes()
                })))
            }
            "rotational" => Some(Arc::new(SysStaticAttrNode::new("rotational", b"1\n"))),
            "read_ahead_kb" => Some(Arc::new(SysStaticAttrNode::new("read_ahead_kb", b"128\n"))),
            "nr_requests" => Some(Arc::new(SysStaticAttrNode::new("nr_requests", b"128\n"))),
            "max_sectors_kb" => Some(Arc::new(SysStaticAttrNode::new("max_sectors_kb", b"512\n"))),
            _ => None,
        }
    }

    fn list_children(&self) -> Vec<(String, bool)> {
        vec![
            (String::from("logical_block_size"), false),
            (String::from("physical_block_size"), false),
            (String::from("hw_sector_size"), false),
            (String::from("rotational"), false),
            (String::from("read_ahead_kb"), false),
            (String::from("nr_requests"), false),
            (String::from("max_sectors_kb"), false),
        ]
    }
}

/// Directory for `/sys/class/block` providing symlinks to `/sys/block/<dev>`.
pub struct SysClassBlockDirNode {
    ino: usize,
}

impl SysClassBlockDirNode {
    pub fn new() -> Self {
        Self {
            ino: alloc_sysfs_ino(),
        }
    }
}

impl Default for SysClassBlockDirNode {
    fn default() -> Self {
        Self::new()
    }
}

impl SysNode for SysClassBlockDirNode {
    fn name(&self) -> &str {
        "block"
    }

    fn ino(&self) -> usize {
        self.ino
    }

    fn is_dir(&self) -> bool {
        true
    }

    fn get_child(&self, name: &str) -> Option<Arc<dyn SysNode>> {
        for dev in crate::drivers::get_blkdevs() {
            if dev.name() == name {
                return Some(Arc::new(SysSymlinkNode::new(
                    name,
                    &format!("../../block/{}", name),
                )));
            }
        }
        None
    }

    fn list_children(&self) -> Vec<(String, bool)> {
        crate::drivers::get_blkdevs()
            .into_iter()
            .map(|d| (d.name(), false))
            .collect()
    }
}
