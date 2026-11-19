// SPDX-License-Identifier: MPL-2.0
//
// Ported from Asterinas `aster-device` (`kernel/core/comps/device/src`):
// https://github.com/asterinas/asterinas/tree/e36b5b3b759e616febf2c8f0e88d8cb2c103f266/kernel/core/comps/device/src
// Copyright (c) Asterinas contributors. Adapted for PetraOS.

//! The device model's shared registry.
//!
//! [`init`] creates the device-model roots and mirrors registered buses,
//! classes, and root devices into PetraOS's `/sys` tree ([`sys_tree`]).

use alloc::{string::String, sync::Arc, vec::Vec};

use ostd::sync::Mutex;
use spin::Once;

use crate::drivers::core::common::{
    DevKind, Result, SysStr,
    node::{Dir, GlueDirs},
    subsystem::SubsystemOps,
};
use crate::fs::sysfs::kobject::{SysNode, sys_tree};

/// Returns the initialized device registry.
pub(crate) fn get() -> &'static Registry {
    REGISTRY.get().expect("the device model is not initialized")
}

/// Initializes the device registry once (idempotent).
pub(crate) fn init() {
    REGISTRY.call_once(|| Registry::new().expect("cannot create the device model roots"));
}

/// Looks up a `<major>:<minor>` symlink node in `/sys/dev/{char,block}` if the
/// device registry is initialized.
pub fn dev_index_node(kind: DevKind, name: &str) -> Option<Arc<dyn SysNode>> {
    let reg = REGISTRY.get()?;
    reg.dev_index(kind).get_child(name)
}

/// Lists all `<major>:<minor>` entry names currently recorded in the device
/// registry for `kind`.
pub fn dev_index_names(kind: DevKind) -> Vec<String> {
    let Some(reg) = REGISTRY.get() else {
        return Vec::new();
    };
    reg.dev_index(kind)
        .list_children()
        .into_iter()
        .map(|(name, _)| name)
        .collect()
}

/// The top-level directories the device model owns.
pub(crate) struct Registry {
    root: Arc<Dir>,
    devices: Arc<Dir>,
    virtual_dir: Arc<Dir>,
    bus: Arc<Dir>,
    class: Arc<Dir>,
    dev_char: Arc<Dir>,
    dev_block: Arc<Dir>,
    virtual_glue_dirs: GlueDirs,
    /// The registered buses and classes, kept alive for the life of the kernel.
    subsystems: Mutex<Vec<Arc<dyn SubsystemOps>>>,
}

impl Registry {
    #[cfg_attr(not(ktest), allow(dead_code))]
    pub(crate) fn root(&self) -> &Arc<Dir> {
        &self.root
    }

    pub(crate) fn devices_root(&self) -> &Arc<Dir> {
        &self.devices
    }

    pub(crate) fn bus_root(&self) -> &Arc<Dir> {
        &self.bus
    }

    pub(crate) fn class_root(&self) -> &Arc<Dir> {
        &self.class
    }

    pub(crate) fn dev_index(&self, kind: DevKind) -> &Arc<Dir> {
        match kind {
            DevKind::Char => &self.dev_char,
            DevKind::Block => &self.dev_block,
        }
    }

    /// Keeps a registered bus or class alive for the life of the kernel.
    pub(crate) fn keep_subsystem(&self, subsystem: Arc<dyn SubsystemOps>) {
        self.subsystems.lock().push(subsystem);
    }

    /// Mirrors a newly registered bus directory into `/sys/bus/<name>`.
    pub(crate) fn mirror_bus(&self, name: &str, dir: Arc<dyn SysNode>) {
        let bus_dir = sys_tree().bus_dir();
        if bus_dir.get_child(name).is_none() {
            let _ = bus_dir.insert_child(name, dir);
        }
    }

    /// Mirrors a newly registered class directory into `/sys/class/<name>`
    /// when a legacy specialized view does not already claim that name.
    pub(crate) fn mirror_class(&self, name: &str, dir: Arc<dyn SysNode>) {
        let class_dir = sys_tree().class_dir();
        if class_dir.get_child(name).is_none() {
            let _ = class_dir.insert_child(name, dir);
        }
    }

    /// Mirrors a top-level device directory into `/sys/devices/<name>`.
    pub(crate) fn mirror_root_device(&self, name: &str, node: Arc<dyn SysNode>) {
        let devices_dir = sys_tree().devices_dir();
        if devices_dir.get_child(name).is_none() {
            let _ = devices_dir.insert_child(name, node);
        }
    }

    /// Removes a mirrored top-level device directory from `/sys/devices/<name>`.
    pub(crate) fn unmirror_root_device(&self, name: &str) {
        let _ = sys_tree().devices_dir().remove_child(name);
    }

    /// Attaches `child` into `/devices/virtual/<class>`, creating the directory
    /// if needed, and mirrors it into `/sys/devices/virtual/<class>`.
    pub(crate) fn attach_into_virtual_glue_dir(
        &self,
        class: &str,
        child: Arc<dyn SysNode>,
    ) -> Result<Arc<Dir>> {
        let dir = self
            .virtual_glue_dirs
            .attach_into(class, self.virtual_dir.as_ref(), child.clone())?;
        let vclass = sys_tree().virtual_dir().add_dir(class);
        let child_name = String::from(child.name());
        if vclass.get_child(&child_name).is_none() {
            let _ = vclass.insert_child(&child_name, child);
        }
        Ok(dir)
    }

    pub(crate) fn drop_virtual_glue_dir_if_empty(&self, class: &str, child_name: &str) {
        self.virtual_glue_dirs
            .drop_if_empty(class, self.virtual_dir.as_ref());
        if sys_tree().virtual_dir().get_child(class).is_some() {
            let vclass = sys_tree().virtual_dir().add_dir(class);
            let _ = vclass.remove_child(child_name);
            if vclass.list_children().is_empty() && !matches!(class, "drm" | "input") {
                let _ = sys_tree().virtual_dir().remove_child(class);
            }
        }
    }

    fn new() -> Result<Self> {
        let root = Dir::new_root();
        let devices = Dir::new(SysStr::from("devices"));
        let virtual_dir = Dir::new(SysStr::from("virtual"));
        let bus = Dir::new(SysStr::from("bus"));
        let class = Dir::new(SysStr::from("class"));
        let dev = Dir::new(SysStr::from("dev"));
        let dev_char = Dir::new(SysStr::from("char"));
        let dev_block = Dir::new(SysStr::from("block"));

        root.attach_dir(&devices)?;
        devices.attach_dir(&virtual_dir)?;
        root.attach_dir(&bus)?;
        root.attach_dir(&class)?;
        root.attach_dir(&dev)?;
        dev.attach_dir(&dev_char)?;
        dev.attach_dir(&dev_block)?;

        Ok(Self {
            root,
            devices,
            virtual_dir,
            bus,
            class,
            dev_char,
            dev_block,
            virtual_glue_dirs: GlueDirs::new(),
            subsystems: Mutex::new(Vec::new()),
        })
    }
}

static REGISTRY: Once<Registry> = Once::new();
