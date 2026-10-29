// SPDX-License-Identifier: GPL-2.0

//! SysFS Device Directory Subsystem (`/sys/dev/char` and `/sys/dev/block`).
//!
//! Provides device number lookup symlinks for `libudev`, `mesa`, and `libinput`.
//! Resolves `<major>:<minor>` entries to backing devices under `/sys/class/` or
//! `/sys/block/`.

use alloc::{format, string::String, sync::Arc, vec, vec::Vec};

use crate::fs::sysfs::kobject::{SysNode, SysSymlinkNode, alloc_sysfs_ino};

/// Linux device major numbers.
const MEM_MAJOR: u32 = 1;
const TTY_MAJOR: u32 = 5;
const MISC_MAJOR: u32 = 10;
const INPUT_MAJOR: u32 = 13;
const DRM_MAJOR: u32 = 226;

/// Root directory `/sys/dev`.
pub struct SysDevRootDirNode {
    ino: usize,
}

impl SysDevRootDirNode {
    /// Creates a new `/sys/dev` root node.
    pub fn new() -> Self {
        Self {
            ino: alloc_sysfs_ino(),
        }
    }
}

impl Default for SysDevRootDirNode {
    fn default() -> Self {
        Self::new()
    }
}

impl SysNode for SysDevRootDirNode {
    fn name(&self) -> &str {
        "dev"
    }

    fn ino(&self) -> usize {
        self.ino
    }

    fn is_dir(&self) -> bool {
        true
    }

    fn get_child(&self, name: &str) -> Option<Arc<dyn SysNode>> {
        match name {
            "char" => Some(Arc::new(SysDevCharDirNode::new())),
            "block" => Some(Arc::new(SysDevBlockDirNode::new())),
            _ => None,
        }
    }

    fn list_children(&self) -> Vec<(String, bool)> {
        vec![(String::from("char"), true), (String::from("block"), true)]
    }
}

/// Directory `/sys/dev/char` exposing `<major>:<minor>` symlinks.
pub struct SysDevCharDirNode {
    ino: usize,
}

impl SysDevCharDirNode {
    /// Creates a new `/sys/dev/char` directory node.
    pub fn new() -> Self {
        Self {
            ino: alloc_sysfs_ino(),
        }
    }

    fn resolve_drm(&self, name: &str, minor: u32) -> Option<Arc<dyn SysNode>> {
        if minor < 128 {
            Some(Arc::new(SysSymlinkNode::new(
                name,
                &format!("../../class/drm/card{minor}"),
            )))
        } else {
            Some(Arc::new(SysSymlinkNode::new(
                name,
                &format!("../../class/drm/renderD{minor}"),
            )))
        }
    }

    fn resolve_input(&self, name: &str, minor: u32) -> Option<Arc<dyn SysNode>> {
        if minor == 63 {
            Some(Arc::new(SysSymlinkNode::new(
                name,
                "../../class/input/mice",
            )))
        } else if minor >= 64 {
            let event_idx = minor - 64;
            Some(Arc::new(SysSymlinkNode::new(
                name,
                &format!("../../class/input/event{event_idx}"),
            )))
        } else {
            None
        }
    }

    fn resolve_tty(&self, name: &str, minor: u32) -> Option<Arc<dyn SysNode>> {
        match minor {
            0 => Some(Arc::new(SysSymlinkNode::new(name, "../../class/tty/tty"))),
            1 => Some(Arc::new(SysSymlinkNode::new(
                name,
                "../../class/tty/console",
            ))),
            2 => Some(Arc::new(SysSymlinkNode::new(name, "../../class/tty/ptmx"))),
            _ => None,
        }
    }

    fn resolve_mem(&self, name: &str, minor: u32) -> Option<Arc<dyn SysNode>> {
        match minor {
            3 => Some(Arc::new(SysSymlinkNode::new(name, "../../class/mem/null"))),
            5 => Some(Arc::new(SysSymlinkNode::new(name, "../../class/mem/zero"))),
            8 => Some(Arc::new(SysSymlinkNode::new(
                name,
                "../../class/mem/random",
            ))),
            9 => Some(Arc::new(SysSymlinkNode::new(
                name,
                "../../class/mem/urandom",
            ))),
            _ => None,
        }
    }
}

impl Default for SysDevCharDirNode {
    fn default() -> Self {
        Self::new()
    }
}

impl SysNode for SysDevCharDirNode {
    fn name(&self) -> &str {
        "char"
    }

    fn ino(&self) -> usize {
        self.ino
    }

    fn is_dir(&self) -> bool {
        true
    }

    fn get_child(&self, name: &str) -> Option<Arc<dyn SysNode>> {
        let (maj_s, min_s) = name.split_once(':')?;
        let maj: u32 = maj_s.parse().ok()?;
        let min: u32 = min_s.parse().ok()?;

        match maj {
            DRM_MAJOR => self.resolve_drm(name, min),
            INPUT_MAJOR => self.resolve_input(name, min),
            TTY_MAJOR => self.resolve_tty(name, min),
            MEM_MAJOR => self.resolve_mem(name, min),
            MISC_MAJOR if min == 223 => {
                Some(Arc::new(SysSymlinkNode::new(name, "../../class/misc/uinput")))
            }
            _ => None,
        }
    }

    fn list_children(&self) -> Vec<(String, bool)> {
        let mut children = Vec::new();

        // DRM cards and render nodes
        let drm_count = crate::drivers::drm::core::DRM_DEVICES.lock().len();
        for minor in 0..drm_count {
            children.push((format!("{DRM_MAJOR}:{minor}"), false));
            children.push((format!("{DRM_MAJOR}:{}", 128 + minor), false));
        }

        // Input devices
        for dev in crate::drivers::input::devices() {
            children.push((format!("{INPUT_MAJOR}:{}", 64 + dev.index()), false));
        }
        children.push((format!("{INPUT_MAJOR}:63"), false));

        // TTY devices
        children.push((format!("{TTY_MAJOR}:0"), false));
        children.push((format!("{TTY_MAJOR}:1"), false));
        children.push((format!("{TTY_MAJOR}:2"), false));

        // Mem devices
        children.push((format!("{MEM_MAJOR}:3"), false));
        children.push((format!("{MEM_MAJOR}:5"), false));
        children.push((format!("{MEM_MAJOR}:8"), false));
        children.push((format!("{MEM_MAJOR}:9"), false));

        // Misc devices
        children.push((format!("{MISC_MAJOR}:223"), false));

        children
    }
}

/// Directory `/sys/dev/block` exposing `<major>:<minor>` symlinks for block devices.
pub struct SysDevBlockDirNode {
    ino: usize,
}

impl SysDevBlockDirNode {
    /// Creates a new `/sys/dev/block` directory node.
    pub fn new() -> Self {
        Self {
            ino: alloc_sysfs_ino(),
        }
    }
}

impl Default for SysDevBlockDirNode {
    fn default() -> Self {
        Self::new()
    }
}

impl SysNode for SysDevBlockDirNode {
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
            let (maj, min) = dev.device_number();
            if name == format!("{maj}:{min}") {
                return Some(Arc::new(SysSymlinkNode::new(
                    name,
                    &format!("../../block/{}", dev.name()),
                )));
            }
        }
        None
    }

    fn list_children(&self) -> Vec<(String, bool)> {
        crate::drivers::get_blkdevs()
            .into_iter()
            .map(|dev| {
                let (maj, min) = dev.device_number();
                (format!("{maj}:{min}"), false)
            })
            .collect()
    }
}
