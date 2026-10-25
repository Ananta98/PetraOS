// SPDX-License-Identifier: GPL-2.0

//! SysFS Input Class Subsystem (`/sys/class/input`).
//!
//! Exposes evdev event nodes and mice multiplexers to user-space input
//! handlers (`libinput`, `systemd-udevd`, Wayland compositors).

use alloc::{format, string::String, sync::Arc, vec, vec::Vec};

use crate::{
    drivers::input::{self, InputDevice},
    fs::sysfs::kobject::{
        SysAttrNode, SysNode, SysStaticAttrNode, SysSymlinkNode, alloc_sysfs_ino,
    },
};

const INPUT_MAJOR: u32 = 13;

/// Directory `/sys/class/input`.
pub struct SysClassInputDirNode {
    ino: usize,
}

impl SysClassInputDirNode {
    /// Creates a new `/sys/class/input` directory node.
    pub fn new() -> Self {
        Self {
            ino: alloc_sysfs_ino(),
        }
    }
}

impl Default for SysClassInputDirNode {
    fn default() -> Self {
        Self::new()
    }
}

impl SysNode for SysClassInputDirNode {
    fn name(&self) -> &str {
        "input"
    }

    fn ino(&self) -> usize {
        self.ino
    }

    fn is_dir(&self) -> bool {
        true
    }

    fn get_child(&self, name: &str) -> Option<Arc<dyn SysNode>> {
        if name == "mice" {
            return Some(Arc::new(SysInputMiceDirNode::new()));
        }

        if let Some(event_idx) = name.strip_prefix("event") {
            let index: usize = event_idx.parse().ok()?;
            let dev = input::get_device(index)?;
            return Some(Arc::new(SysInputEventDirNode::new(dev)));
        }

        None
    }

    fn list_children(&self) -> Vec<(String, bool)> {
        let mut children = Vec::new();

        for dev in input::devices() {
            children.push((format!("event{}", dev.index()), true));
        }
        children.push((String::from("mice"), true));

        children
    }
}

/// Directory `/sys/class/input/eventN`.
pub struct SysInputEventDirNode {
    dev: Arc<InputDevice>,
    ino: usize,
}

impl SysInputEventDirNode {
    /// Creates a new evdev sysfs directory node.
    pub fn new(dev: Arc<InputDevice>) -> Self {
        Self {
            dev,
            ino: alloc_sysfs_ino(),
        }
    }
}

impl SysNode for SysInputEventDirNode {
    fn name(&self) -> &str {
        "event"
    }

    fn ino(&self) -> usize {
        self.ino
    }

    fn is_dir(&self) -> bool {
        true
    }

    fn get_child(&self, name: &str) -> Option<Arc<dyn SysNode>> {
        let idx = self.dev.index();
        let dev_name = String::from(self.dev.name());

        match name {
            "dev" => Some(Arc::new(SysAttrNode::new("dev", move || {
                format!("{INPUT_MAJOR}:{}\n", 64 + idx).into_bytes()
            }))),
            "uevent" => Some(Arc::new(SysAttrNode::new("uevent", move || {
                format!("MAJOR={INPUT_MAJOR}\nMINOR={}\nDEVNAME=input/event{idx}\n", 64 + idx)
                    .into_bytes()
            }))),
            "name" => Some(Arc::new(SysAttrNode::new("name", move || {
                format!("{dev_name}\n").into_bytes()
            }))),
            "device" => Some(Arc::new(SysSymlinkNode::new(
                "device",
                &format!("../../devices/virtual/input/input{idx}"),
            ))),
            "subsystem" => Some(Arc::new(SysSymlinkNode::new(
                "subsystem",
                "../../class/input",
            ))),
            _ => None,
        }
    }

    fn list_children(&self) -> Vec<(String, bool)> {
        vec![
            (String::from("dev"), false),
            (String::from("uevent"), false),
            (String::from("name"), false),
            (String::from("device"), false),
            (String::from("subsystem"), false),
        ]
    }
}

/// Directory `/sys/class/input/mice`.
pub struct SysInputMiceDirNode {
    ino: usize,
}

impl SysInputMiceDirNode {
    /// Creates a new mice sysfs directory node.
    pub fn new() -> Self {
        Self {
            ino: alloc_sysfs_ino(),
        }
    }
}

impl Default for SysInputMiceDirNode {
    fn default() -> Self {
        Self::new()
    }
}

impl SysNode for SysInputMiceDirNode {
    fn name(&self) -> &str {
        "mice"
    }

    fn ino(&self) -> usize {
        self.ino
    }

    fn is_dir(&self) -> bool {
        true
    }

    fn get_child(&self, name: &str) -> Option<Arc<dyn SysNode>> {
        match name {
            "dev" => Some(Arc::new(SysStaticAttrNode::new("dev", b"13:63\n"))),
            "uevent" => Some(Arc::new(SysStaticAttrNode::new(
                "uevent",
                b"MAJOR=13\nMINOR=63\nDEVNAME=input/mice\n",
            ))),
            "subsystem" => Some(Arc::new(SysSymlinkNode::new(
                "subsystem",
                "../../class/input",
            ))),
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
