// SPDX-License-Identifier: GPL-2.0

//! SysFS TTY & Memory Class Subsystems (`/sys/class/tty` and `/sys/class/mem`).
//!
//! Exposes virtual terminal, console, ptmx, and memory pseudo-devices (null, zero, random)
//! to user-space discovery tools and standard Linux utilities.

use alloc::{format, string::String, sync::Arc, vec, vec::Vec};

use crate::fs::sysfs::kobject::{
    SysAttrNode, SysNode, SysSymlinkNode, alloc_sysfs_ino,
};

const TTY_MAJOR: u32 = 5;
const MEM_MAJOR: u32 = 1;

/// Directory `/sys/class/tty`.
pub struct SysClassTtyDirNode {
    ino: usize,
}

impl SysClassTtyDirNode {
    /// Creates a new `/sys/class/tty` directory node.
    pub fn new() -> Self {
        Self {
            ino: alloc_sysfs_ino(),
        }
    }
}

impl Default for SysClassTtyDirNode {
    fn default() -> Self {
        Self::new()
    }
}

impl SysNode for SysClassTtyDirNode {
    fn name(&self) -> &str {
        "tty"
    }

    fn ino(&self) -> usize {
        self.ino
    }

    fn is_dir(&self) -> bool {
        true
    }

    fn get_child(&self, name: &str) -> Option<Arc<dyn SysNode>> {
        match name {
            "tty" => Some(Arc::new(SysTtyDevDirNode::new("tty", 0))),
            "console" => Some(Arc::new(SysTtyDevDirNode::new("console", 1))),
            "ptmx" => Some(Arc::new(SysTtyDevDirNode::new("ptmx", 2))),
            _ => None,
        }
    }

    fn list_children(&self) -> Vec<(String, bool)> {
        vec![
            (String::from("tty"), true),
            (String::from("console"), true),
            (String::from("ptmx"), true),
        ]
    }
}

/// Directory for individual tty devices (`/sys/class/tty/<name>`).
pub struct SysTtyDevDirNode {
    name: String,
    minor: u32,
    ino: usize,
}

impl SysTtyDevDirNode {
    /// Creates a new TTY sysfs device node.
    pub fn new(name: &str, minor: u32) -> Self {
        Self {
            name: String::from(name),
            minor,
            ino: alloc_sysfs_ino(),
        }
    }
}

impl SysNode for SysTtyDevDirNode {
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
        let minor = self.minor;
        let devname = self.name.clone();

        match name {
            "dev" => Some(Arc::new(SysAttrNode::new("dev", move || {
                format!("{TTY_MAJOR}:{minor}\n").into_bytes()
            }))),
            "uevent" => Some(Arc::new(SysAttrNode::new("uevent", move || {
                format!("MAJOR={TTY_MAJOR}\nMINOR={minor}\nDEVNAME={devname}\n").into_bytes()
            }))),
            "subsystem" => Some(Arc::new(SysSymlinkNode::new("subsystem", "../../class/tty"))),
            _ => None,
        }
    }

    fn list_children(&self) -> Vec<(String, bool)> {
        vec![
            (String::from("dev"), false),
            (String::from("uevent"), false),
            (String::from("subsystem"), false),
        ]
    }
}

/// Directory `/sys/class/mem`.
pub struct SysClassMemDirNode {
    ino: usize,
}

impl SysClassMemDirNode {
    /// Creates a new `/sys/class/mem` directory node.
    pub fn new() -> Self {
        Self {
            ino: alloc_sysfs_ino(),
        }
    }
}

impl Default for SysClassMemDirNode {
    fn default() -> Self {
        Self::new()
    }
}

impl SysNode for SysClassMemDirNode {
    fn name(&self) -> &str {
        "mem"
    }

    fn ino(&self) -> usize {
        self.ino
    }

    fn is_dir(&self) -> bool {
        true
    }

    fn get_child(&self, name: &str) -> Option<Arc<dyn SysNode>> {
        match name {
            "null" => Some(Arc::new(SysMemDevDirNode::new("null", 3))),
            "zero" => Some(Arc::new(SysMemDevDirNode::new("zero", 5))),
            "random" => Some(Arc::new(SysMemDevDirNode::new("random", 8))),
            "urandom" => Some(Arc::new(SysMemDevDirNode::new("urandom", 9))),
            _ => None,
        }
    }

    fn list_children(&self) -> Vec<(String, bool)> {
        vec![
            (String::from("null"), true),
            (String::from("zero"), true),
            (String::from("random"), true),
            (String::from("urandom"), true),
        ]
    }
}

/// Directory for individual memory devices (`/sys/class/mem/<name>`).
pub struct SysMemDevDirNode {
    name: String,
    minor: u32,
    ino: usize,
}

impl SysMemDevDirNode {
    /// Creates a new memory sysfs device node.
    pub fn new(name: &str, minor: u32) -> Self {
        Self {
            name: String::from(name),
            minor,
            ino: alloc_sysfs_ino(),
        }
    }
}

impl SysNode for SysMemDevDirNode {
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
        let minor = self.minor;
        let devname = self.name.clone();

        match name {
            "dev" => Some(Arc::new(SysAttrNode::new("dev", move || {
                format!("{MEM_MAJOR}:{minor}\n").into_bytes()
            }))),
            "uevent" => Some(Arc::new(SysAttrNode::new("uevent", move || {
                format!("MAJOR={MEM_MAJOR}\nMINOR={minor}\nDEVNAME={devname}\n").into_bytes()
            }))),
            "subsystem" => Some(Arc::new(SysSymlinkNode::new("subsystem", "../../class/mem"))),
            _ => None,
        }
    }

    fn list_children(&self) -> Vec<(String, bool)> {
        vec![
            (String::from("dev"), false),
            (String::from("uevent"), false),
            (String::from("subsystem"), false),
        ]
    }
}
