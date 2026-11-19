// SPDX-License-Identifier: MPL-2.0
//
// Ported from Asterinas `aster-device` (`kernel/core/comps/device/src/test`):
// https://github.com/asterinas/asterinas/tree/e36b5b3b759e616febf2c8f0e88d8cb2c103f266/kernel/core/comps/device/src/test
// Copyright (c) Asterinas contributors. Adapted for PetraOS.

//! Device-model unit tests (`cargo osdk test`).

use alloc::{
    boxed::Box,
    format,
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};
use ::core::sync::atomic::{AtomicUsize, Ordering};

use ostd::prelude::ktest;
use ostd::sync::Mutex;

use crate::drivers::core::{
    self,
    bus::{self, Bus, BusDevice, BusHandle, Driver},
    class::{self, Class, ClassDevice, ClassHandle, ClassObserver},
    common::{AnyDevice, Attr, BareDevice, DevNode, DevNum, DeviceId, DeviceInternals, DeviceType, Error, Result, SysStr, registry},
    hooks::{DevNodeRequest, HookError, HookSlot, KernelHooks},
};
use crate::fs::sysfs::kobject::SysNode;

// ---------------------------------------------------------------------------
// Test helpers: sysfs inspection & toy bus/class
// ---------------------------------------------------------------------------

mod utils {
    use super::*;

    pub(super) fn lookup(path: &str) -> Option<Arc<dyn SysNode>> {
        let mut node: Arc<dyn SysNode> = registry::get().root().clone();
        for part in path.split('/').filter(|p| !p.is_empty()) {
            node = node.get_child(part)?;
        }
        Some(node)
    }

    pub(super) fn link_target(path: &str) -> Option<String> {
        let node = lookup(path)?;
        if !node.is_symlink() {
            return None;
        }
        node.readlink().ok()
    }

    pub(super) fn read_attr(path: &str, name: &str) -> String {
        let dir = lookup(path).expect("node path missing");
        let attr = dir.get_child(name).expect("attribute missing");
        let bytes = attr.read_attr().expect("read_attr failed");
        String::from_utf8(bytes).expect("utf8 attr")
    }

    pub(super) fn attr_names(path: &str) -> Vec<String> {
        let dir = lookup(path).expect("node path missing");
        dir.list_children()
            .into_iter()
            .filter_map(|(name, is_dir)| {
                if is_dir {
                    return None;
                }
                let child = dir.get_child(&name)?;
                if child.is_symlink() { None } else { Some(name) }
            })
            .collect()
    }

    pub(super) fn attr_ids<D: AnyDevice>(dev: &Arc<D>) -> Vec<(String, u8)> {
        dev.base()
            .attr_set()
            .iter()
            .map(|meta| (meta.name().to_string(), meta.id()))
            .collect()
    }
}

mod toy {
    use super::*;

    pub(super) fn register_toy_subsystems() -> (Arc<ClassHandle<ToyBlock>>, Arc<BusHandle<ToyBus>>) {
        static TOY_SUBSYSTEMS: spin::Once<(Arc<ClassHandle<ToyBlock>>, Arc<BusHandle<ToyBus>>)> =
            spin::Once::new();

        core::init();
        TOY_SUBSYSTEMS
            .call_once(|| {
                let bus = bus::register(ToyBus).unwrap();
                let class = class::register(ToyBlock).unwrap();
                (class, bus)
            })
            .clone()
    }

    pub(super) struct ToyBus;

    pub(super) struct ToyDev {
        pub(super) vendor: u32,
        pub(super) model: u32,
    }

    pub(super) struct ToyMatch {
        pub(super) vendor: u32,
    }

    impl Bus for ToyBus {
        const NAME: &'static str = "toy";
        type Device = ToyDev;
        type MatchData = ToyMatch;

        fn matches(&self, dev: &ToyDev, data: &ToyMatch) -> bool {
            dev.vendor == data.vendor
        }

        fn dev_attrs(&self) -> &'static [Attr<BusDevice<Self>>] {
            TOY_DEV_ATTRS
        }
    }

    pub(super) static DISK_TYPE: DeviceType<BusDevice<ToyBus>> = DeviceType::named("disk");

    pub(super) struct ToyDiskDriver {
        pub(super) match_data: ToyMatch,
        pub(super) class: Arc<ClassHandle<ToyBlock>>,
        pub(super) suppress_bind_attrs: bool,
    }

    impl Driver<ToyBus> for ToyDiskDriver {
        fn name(&self) -> &str {
            "toy_disk"
        }

        fn match_data(&self) -> &ToyMatch {
            &self.match_data
        }

        fn on_probe(&self, dev: &Arc<BusDevice<ToyBus>>) -> Result<()> {
            let name = format!("td{}", dev.model);
            let disk = ClassDevice::builder(&self.class, name, ToyDisk { sectors: 8 })
                .parent(dev.clone())
                .devnum(DevNum::block(DeviceId::new(200, dev.model)))
                .build();
            core::add_device(&disk)?;
            Ok(())
        }

        fn on_release(&self, dev: &Arc<BusDevice<ToyBus>>) {
            let path = dev.path();
            assert!(utils::lookup(&format!("{path}/driver")).is_none());
            assert!(
                !utils::attr_names(&path)
                    .iter()
                    .any(|name| name == "bound_by")
            );

            let name = format!("td{}", dev.model);
            let disk = self.class.find_device(&name).unwrap();
            core::remove_device(&disk).unwrap();
        }

        fn dev_attrs(&self) -> &'static [Attr<BusDevice<ToyBus>>] {
            BOUND_BY
        }

        fn suppress_bind_attrs(&self) -> bool {
            self.suppress_bind_attrs
        }
    }

    pub(super) struct ToyBlock;

    pub(super) struct ToyDisk {
        pub(super) sectors: u64,
    }

    impl Class for ToyBlock {
        const NAME: &'static str = "toyblk";
        type Device = ToyDisk;

        fn dev_attrs(&self) -> &'static [Attr<ClassDevice<Self>>] {
            TOY_BLOCK_ATTRS
        }

        fn devnode(&self, _dev: &ClassDevice<Self>) -> Option<DevNode> {
            Some(DevNode {
                path: None,
                mode: Some(0o660),
            })
        }
    }

    pub(super) struct CountingObserver {
        pub(super) added: AtomicUsize,
        pub(super) removed: AtomicUsize,
    }

    impl ClassObserver<ToyBlock> for CountingObserver {
        fn on_device_added(&self, _dev: &Arc<ClassDevice<ToyBlock>>) {
            self.added.fetch_add(1, Ordering::Relaxed);
        }

        fn on_device_removed(&self, _dev: &Arc<ClassDevice<ToyBlock>>) {
            self.removed.fetch_add(1, Ordering::Relaxed);
        }
    }

    const TOY_DEV_ATTRS: &[Attr<BusDevice<ToyBus>>] = &[
        Attr::ro("vendor", |dev, w| {
            writeln!(w, "{:#06x}", dev.vendor)?;
            Ok(())
        }),
        Attr::ro("model", |dev, w| {
            writeln!(w, "{:#06x}", dev.model)?;
            Ok(())
        }),
    ];

    const BOUND_BY: &[Attr<BusDevice<ToyBus>>] = &[Attr::ro("bound_by", |_dev, w| {
        writeln!(w, "toy_disk")?;
        Ok(())
    })];

    const TOY_BLOCK_ATTRS: &[Attr<ClassDevice<ToyBlock>>] = &[Attr::ro("size", |dev, w| {
        writeln!(w, "{}", dev.sectors)?;
        Ok(())
    })];
}

use toy::{CountingObserver, ToyBlock, ToyBus, ToyDev, ToyDisk, ToyDiskDriver, ToyMatch};

// ---------------------------------------------------------------------------
// Registry & Registration tests
// ---------------------------------------------------------------------------

#[ktest]
fn registry_creates_top_level_directories() {
    core::init();
    for path in [
        "/devices",
        "/devices/virtual",
        "/bus",
        "/class",
        "/dev",
        "/dev/char",
        "/dev/block",
    ] {
        assert!(utils::lookup(path).is_some(), "{path} missing");
    }
}

#[ktest]
fn parentless_class_device_lives_under_virtual() {
    let (class, _) = toy::register_toy_subsystems();
    let dev = ClassDevice::builder(&class, "loop0", ToyDisk { sectors: 1 })
        .devnum(DevNum::block(DeviceId::new(7, 0)))
        .build();
    core::add_device(&dev).unwrap();

    assert_eq!(dev.path(), "/devices/virtual/toyblk/loop0");
    assert_eq!(
        utils::link_target("/class/toyblk/loop0").unwrap(),
        "../../devices/virtual/toyblk/loop0"
    );
    assert_eq!(
        utils::link_target("/devices/virtual/toyblk/loop0/subsystem").unwrap(),
        "../../../../class/toyblk"
    );
    assert!(utils::lookup("/devices/virtual/toyblk/loop0/device").is_none());

    core::remove_device(&dev).unwrap();
    assert!(utils::lookup("/devices/virtual/toyblk").is_none());
}

#[ktest]
fn class_device_under_class_device_has_no_glue_dir() {
    let (class, _) = toy::register_toy_subsystems();
    let disk = ClassDevice::builder(&class, "sd0", ToyDisk { sectors: 16 }).build();
    core::add_device(&disk).unwrap();

    let part = ClassDevice::builder(&class, "sd0p1", ToyDisk { sectors: 8 })
        .parent(disk.clone())
        .build();
    core::add_device(&part).unwrap();
    assert_eq!(part.path(), "/devices/virtual/toyblk/sd0/sd0p1");

    core::remove_device(&part).unwrap();
    core::remove_device(&disk).unwrap();
}

#[ktest]
fn unregistered_parent_is_rejected() {
    let (class, _) = toy::register_toy_subsystems();
    let parent = ClassDevice::builder(&class, "orphan-parent", ToyDisk { sectors: 0 }).build();
    let child = ClassDevice::builder(&class, "orphan", ToyDisk { sectors: 0 })
        .parent(parent)
        .build();

    assert!(matches!(core::add_device(&child), Err(Error::ParentNotAdded)));
}

#[ktest]
fn duplicate_device_name_is_rejected() {
    let (class, _) = toy::register_toy_subsystems();
    let a = ClassDevice::builder(&class, "twin", ToyDisk { sectors: 0 }).build();
    let b = ClassDevice::builder(&class, "twin", ToyDisk { sectors: 0 }).build();
    core::add_device(&a).unwrap();
    assert!(matches!(core::add_device(&b), Err(Error::NameConflict)));
    assert!(utils::lookup("/devices/virtual/toyblk/twin").is_some());
    assert_eq!(
        utils::link_target("/class/toyblk/twin").unwrap(),
        "../../devices/virtual/toyblk/twin"
    );

    core::remove_device(&a).unwrap();
    assert!(utils::lookup("/class/toyblk/twin").is_none());
}

#[ktest]
fn class_index_conflict_rolls_back_registration() {
    let (class, _) = toy::register_toy_subsystems();
    let p1 = ClassDevice::builder(&class, "p1", ToyDisk { sectors: 0 }).build();
    let p2 = ClassDevice::builder(&class, "p2", ToyDisk { sectors: 0 }).build();
    core::add_device(&p1).unwrap();
    core::add_device(&p2).unwrap();

    let c1 = ClassDevice::builder(&class, "same", ToyDisk { sectors: 0 })
        .parent(p1.clone())
        .build();
    let c2 = ClassDevice::builder(&class, "same", ToyDisk { sectors: 0 })
        .parent(p2.clone())
        .build();
    core::add_device(&c1).unwrap();
    assert!(matches!(core::add_device(&c2), Err(Error::NameConflict)));
    assert_eq!(
        utils::link_target("/class/toyblk/same").unwrap(),
        "../../devices/virtual/toyblk/p1/same"
    );
    assert!(utils::lookup("/devices/virtual/toyblk/p2/same").is_none());

    core::remove_device(&c1).unwrap();
    core::remove_device(&p2).unwrap();
    core::remove_device(&p1).unwrap();
}

#[ktest]
fn bus_device_registration_and_probe() {
    let (class, bus) = toy::register_toy_subsystems();
    for path in ["/bus/toy", "/bus/toy/devices", "/bus/toy/drivers", "/class/toyblk"] {
        assert!(utils::lookup(path).is_some(), "{path} missing");
    }
    assert_eq!(utils::read_attr("/bus/toy", "drivers_autoprobe"), "1\n");

    let driver = bus
        .register_driver(Box::new(ToyDiskDriver {
            match_data: ToyMatch { vendor: 1 },
            class: class.clone(),
            suppress_bind_attrs: false,
        }))
        .unwrap();
    let observer = Arc::new(CountingObserver {
        added: AtomicUsize::new(0),
        removed: AtomicUsize::new(0),
    });
    class
        .register_observer(observer.clone() as Arc<dyn ClassObserver<ToyBlock>>)
        .unwrap();

    let root = BareDevice::new_root("toy0000:00");
    core::add_device(&root).unwrap();
    let bus_dev = BusDevice::builder(&bus, "0000:00:01.0", ToyDev { vendor: 1, model: 3 })
        .parent(root.clone())
        .dev_type(&toy::DISK_TYPE)
        .build();
    core::add_device(&bus_dev).unwrap();

    assert_eq!(bus_dev.path(), "/devices/toy0000:00/0000:00:01.0");
    assert_eq!(
        utils::link_target("/devices/toy0000:00/0000:00:01.0/subsystem").unwrap(),
        "../../../bus/toy"
    );
    assert_eq!(
        utils::link_target("/bus/toy/devices/0000:00:01.0").unwrap(),
        "../../../devices/toy0000:00/0000:00:01.0"
    );

    assert!(bus_dev.driver().is_some_and(|d| Arc::ptr_eq(&d, &driver)));
    assert_eq!(
        utils::link_target("/devices/toy0000:00/0000:00:01.0/driver").unwrap(),
        "../../../bus/toy/drivers/toy_disk"
    );
    assert_eq!(
        utils::link_target("/bus/toy/drivers/toy_disk/0000:00:01.0").unwrap(),
        "../../../../devices/toy0000:00/0000:00:01.0"
    );

    let names = utils::attr_names("/devices/toy0000:00/0000:00:01.0");
    for expected in ["vendor", "model", "bound_by"] {
        assert!(names.iter().any(|n| n == expected), "{expected} missing");
    }
    assert_eq!(
        utils::read_attr("/devices/toy0000:00/0000:00:01.0", "vendor"),
        "0x0001\n"
    );
    assert_eq!(names.len(), 3);

    let disk_path = "/devices/toy0000:00/0000:00:01.0/toyblk/td3";
    assert!(utils::lookup(disk_path).is_some(), "class device missing");
    assert_eq!(
        utils::link_target(&format!("{disk_path}/device")).unwrap(),
        "../../../0000:00:01.0"
    );
    assert_eq!(
        utils::link_target(&format!("{disk_path}/subsystem")).unwrap(),
        "../../../../../class/toyblk"
    );
    assert_eq!(
        utils::link_target("/class/toyblk/td3").unwrap(),
        "../../devices/toy0000:00/0000:00:01.0/toyblk/td3"
    );
    assert_eq!(
        utils::link_target("/dev/block/200:3").unwrap(),
        "../../devices/toy0000:00/0000:00:01.0/toyblk/td3"
    );
    assert_eq!(utils::read_attr(disk_path, "dev"), "200:3\n");
    assert_eq!(utils::read_attr(disk_path, "size"), "8\n");
    assert_eq!(utils::attr_names(disk_path).len(), 2);
    assert_eq!(observer.added.load(Ordering::Relaxed), 1);

    bus.unbind(&bus_dev).unwrap();
    assert!(bus_dev.driver().is_none());
    assert!(utils::lookup("/devices/toy0000:00/0000:00:01.0/driver").is_none());
    assert!(utils::lookup("/bus/toy/drivers/toy_disk/0000:00:01.0").is_none());
    assert!(
        !utils::attr_names("/devices/toy0000:00/0000:00:01.0")
            .iter()
            .any(|n| n == "bound_by")
    );
    assert!(utils::lookup(disk_path).is_none());
    assert!(utils::lookup("/devices/toy0000:00/0000:00:01.0/toyblk").is_none());
    assert!(utils::lookup("/class/toyblk/td3").is_none());
    assert!(utils::lookup("/dev/block/200:3").is_none());
    assert_eq!(observer.removed.load(Ordering::Relaxed), 1);

    let bind_attr = utils::lookup("/bus/toy/drivers/toy_disk/bind").unwrap();
    bind_attr.write_attr(b"0000:00:01.0\n").unwrap();
    assert!(bus_dev.driver().is_some());
    assert!(utils::lookup(disk_path).is_some());

    assert!(matches!(
        core::remove_device(&bus_dev),
        Err(Error::HasChildren)
    ));
    let unbind_attr = utils::lookup("/bus/toy/drivers/toy_disk/unbind").unwrap();
    unbind_attr.write_attr(b"0000:00:01.0\n").unwrap();
    core::remove_device(&bus_dev).unwrap();
    assert!(utils::lookup("/devices/toy0000:00/0000:00:01.0").is_none());
    assert!(utils::lookup("/bus/toy/devices/0000:00:01.0").is_none());
    assert!(matches!(
        core::remove_device(&bus_dev),
        Err(Error::NotAdded)
    ));
    assert!(matches!(
        core::add_device(&bus_dev),
        Err(Error::AlreadyAdded)
    ));

    core::remove_device(&root).unwrap();
    bus.unregister_driver(&driver).unwrap();
    class
        .unregister_observer(&(observer as Arc<dyn ClassObserver<ToyBlock>>))
        .unwrap();
}

// ---------------------------------------------------------------------------
// Attribute, observer, name validation, and hook-slot tests
// ---------------------------------------------------------------------------

#[ktest]
fn attribute_ids_survive_bind_and_unbind() {
    let (class, bus) = toy::register_toy_subsystems();
    let dev = BusDevice::builder(&bus, "ids0", ToyDev { vendor: 9, model: 91 }).build();
    core::add_device(&dev).unwrap();
    let before = utils::attr_ids(&dev);
    assert!(!before.is_empty());
    assert!(before.iter().all(|(name, _)| name != "bound_by"));

    let driver = bus
        .register_driver(Box::new(ToyDiskDriver {
            match_data: ToyMatch { vendor: 9 },
            class,
            suppress_bind_attrs: false,
        }))
        .unwrap();
    assert!(dev.driver().is_some());
    let driver_attrs = utils::attr_names("/bus/toy/drivers/toy_disk");
    assert!(driver_attrs.iter().any(|name| name == "bind"));
    assert!(driver_attrs.iter().any(|name| name == "unbind"));

    let bound = utils::attr_ids(&dev);
    for (name, id) in &before {
        assert_eq!(
            bound.iter().find(|(n, _)| n == name).map(|(_, i)| *i),
            Some(*id),
        );
    }
    let bound_by_id = bound
        .iter()
        .find(|(n, _)| n == "bound_by")
        .map(|(_, i)| *i)
        .unwrap();
    assert!(before.iter().all(|(_, id)| *id != bound_by_id));

    bus.unbind(&dev).unwrap();
    assert_eq!(utils::attr_ids(&dev), before);

    bus.bind(&dev, &driver).unwrap();
    assert_eq!(
        utils::attr_ids(&dev)
            .iter()
            .find(|(n, _)| n == "bound_by")
            .map(|(_, i)| *i),
        Some(bound_by_id)
    );

    bus.unbind(&dev).unwrap();
    core::remove_device(&dev).unwrap();
    bus.unregister_driver(&driver).unwrap();
}

#[ktest]
fn an_attribute_may_not_shadow_a_core_one() {
    const SHADOWING: &[Attr<BusDevice<ToyBus>>] = &[Attr::ro("dev", |_dev, w| {
        writeln!(w, "shadow")?;
        Ok(())
    })];

    let (_, bus) = toy::register_toy_subsystems();
    let dev = BusDevice::builder(&bus, "shadow0", ToyDev { vendor: 8, model: 1 })
        .devnum(DevNum::char(DeviceId::new(200, 1)))
        .attrs(SHADOWING)
        .build();

    assert!(matches!(core::add_device(&dev), Err(Error::NameConflict)));
    assert!(utils::lookup("/devices/shadow0").is_none());
}

#[ktest]
fn invalid_attribute_names_are_rejected() {
    const INVALID_ATTRS: &[[Attr<ClassDevice<ToyBlock>>; 1]] = &[
        [Attr::ro("", |_dev, _w| Ok(()))],
        [Attr::ro(".", |_dev, _w| Ok(()))],
        [Attr::ro("..", |_dev, _w| Ok(()))],
        [Attr::ro("bad/name", |_dev, _w| Ok(()))],
        [Attr::ro("bad\0name", |_dev, _w| Ok(()))],
    ];
    let (class, _) = toy::register_toy_subsystems();

    for attrs in INVALID_ATTRS {
        let dev = ClassDevice::builder(&class, "invalid-attr", ToyDisk { sectors: 8 })
            .attrs(attrs)
            .build();
        assert!(matches!(core::add_device(&dev), Err(Error::InvalidName)));
        assert!(utils::lookup("/devices/virtual/toyblk/invalid-attr").is_none());
        assert!(utils::lookup("/class/toyblk/invalid-attr").is_none());
    }
}

#[ktest]
fn suppressed_bind_attrs_are_inaccessible() {
    let (class, bus) = toy::register_toy_subsystems();
    let dev = BusDevice::builder(
        &bus,
        "suppressed0",
        ToyDev {
            vendor: 101,
            model: 101,
        },
    )
    .build();
    core::add_device(&dev).unwrap();
    let driver = bus
        .register_driver(Box::new(ToyDiskDriver {
            match_data: ToyMatch { vendor: 101 },
            class: class.clone(),
            suppress_bind_attrs: true,
        }))
        .unwrap();

    let path = "/bus/toy/drivers/toy_disk";
    let attrs = utils::attr_names(path);
    let directory = utils::lookup(path).unwrap();
    for name in ["bind", "unbind"] {
        assert!(!attrs.iter().any(|attr| attr == name));
        assert!(directory.get_child(name).is_none());
    }
    assert!(Arc::ptr_eq(&dev.driver().unwrap(), &driver));
    assert!(class.find_device("td101").is_some());

    bus.unbind(&dev).unwrap();
    core::remove_device(&dev).unwrap();
    bus.unregister_driver(&driver).unwrap();
}

#[ktest]
fn failed_add_does_not_notify_observers() {
    let (class, _) = toy::register_toy_subsystems();
    let observer = Arc::new(CountingObserver {
        added: AtomicUsize::new(0),
        removed: AtomicUsize::new(0),
    });
    class
        .register_observer(observer.clone() as Arc<dyn ClassObserver<ToyBlock>>)
        .unwrap();
    let before_added = observer.added.load(Ordering::Relaxed);

    let a = ClassDevice::builder(&class, "clash", ToyDisk { sectors: 0 }).build();
    let b = ClassDevice::builder(&class, "clash", ToyDisk { sectors: 0 }).build();
    core::add_device(&a).unwrap();
    assert!(matches!(core::add_device(&b), Err(Error::NameConflict)));
    assert_eq!(observer.added.load(Ordering::Relaxed), before_added + 1);
    assert_eq!(observer.removed.load(Ordering::Relaxed), 0);

    core::remove_device(&a).unwrap();
    assert_eq!(observer.removed.load(Ordering::Relaxed), 1);

    class
        .unregister_observer(&(observer as Arc<dyn ClassObserver<ToyBlock>>))
        .unwrap();
}

#[ktest]
fn valid_device_names_are_accepted() {
    let (class, bus) = toy::register_toy_subsystems();
    for name in ["disk0", "0000:00:01.0", "disk.part-1", "cciss!c0d0"] {
        let bare_dev = BareDevice::new_root(name);
        let bus_dev = BusDevice::builder(&bus, name, ToyDev { vendor: 1, model: 3 }).build();
        let class_dev = ClassDevice::builder(&class, name, ToyDisk { sectors: 8 }).build();
        assert_eq!(bare_dev.base().name().as_ref(), name);
        assert_eq!(bus_dev.base().name().as_ref(), name);
        assert_eq!(class_dev.base().name().as_ref(), name);
    }
}

#[ktest]
#[should_panic]
fn empty_device_name_panics_at_construction() {
    BareDevice::new_root("");
}

#[ktest]
#[should_panic]
fn device_name_with_slash_panics_at_construction() {
    BareDevice::new_root("bad/name");
}

#[ktest]
#[should_panic]
fn device_name_with_nul_panics_at_construction() {
    BareDevice::new_root("bad\0name");
}

#[ktest]
fn hook_slot_queues_cancels_and_replays_devnodes() {
    struct RecordingHooks {
        created: Mutex<Vec<SysStr>>,
        deleted: Mutex<Vec<SysStr>>,
    }

    impl KernelHooks for RecordingHooks {
        fn create_devnode(&self, req: &DevNodeRequest) -> ::core::result::Result<(), HookError> {
            self.created.lock().push(req.path.clone());
            Ok(())
        }

        fn delete_devnode(&self, req: &DevNodeRequest) -> ::core::result::Result<(), HookError> {
            self.deleted.lock().push(req.path.clone());
            Ok(())
        }
    }

    static HOOKS: RecordingHooks = RecordingHooks {
        created: Mutex::new(Vec::new()),
        deleted: Mutex::new(Vec::new()),
    };

    let slot = HookSlot::new();
    let req_keep = DevNodeRequest {
        devnum: DevNum::char(DeviceId::new(10, 1)),
        path: SysStr::from("keep0"),
        mode: 0o600,
    };
    let req_drop = DevNodeRequest {
        devnum: DevNum::char(DeviceId::new(10, 2)),
        path: SysStr::from("drop0"),
        mode: 0o600,
    };
    slot.create_devnode(req_keep.clone()).unwrap();
    slot.create_devnode(req_drop.clone()).unwrap();
    slot.delete_devnode(&req_drop).unwrap();

    slot.install(&HOOKS);
    assert_eq!(*HOOKS.created.lock(), alloc::vec![SysStr::from("keep0")]);
    assert!(HOOKS.deleted.lock().is_empty());

    slot.delete_devnode(&req_keep).unwrap();
    assert_eq!(*HOOKS.deleted.lock(), alloc::vec![SysStr::from("keep0")]);
}
