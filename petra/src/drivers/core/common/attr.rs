// SPDX-License-Identifier: MPL-2.0
//
// Ported from Asterinas `aster-device` (`kernel/core/comps/device/src`):
// https://github.com/asterinas/asterinas/tree/e36b5b3b759e616febf2c8f0e88d8cb2c103f266/kernel/core/comps/device/src
// Copyright (c) Asterinas contributors. Adapted for PetraOS.

//! Device attributes exposed as files in sysfs.
//!
//! [`Attr<D>`] declares an attribute's name, permissions, and read or write callbacks.
//! The callbacks receive a reference to the device as `&D`.
//!
//! [`AttrTable`] collects the attributes of one device
//! and dispatches file reads and writes to their callbacks.
//! It stores [`TyErasedAttr`] entries whose callbacks accept `&dyn AnyDevice`,
//! allowing the table to serve devices of different Rust types.
//! Driver attributes are added when the device is bound and removed when it is unbound.

use alloc::{
    collections::{BTreeMap, BTreeSet},
    string::String,
    sync::{Arc, Weak},
    vec::Vec,
};
use core::fmt::{self, Write};

use ostd::sync::RwMutex;

use crate::drivers::core::{
    bus::{Bus, BusDevice, DriverHandle},
    common::{AnyDevice, Error, Result, SysStr, node::is_valid_name},
};

/// Maximum number of attributes allowed on a single device.
const ATTR_SET_CAPACITY: usize = 255;

/// Permissions for a sysfs attribute file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SysPerms {
    readable: bool,
    writable: bool,
}

impl SysPerms {
    /// Read-only permissions (`0444`).
    pub const DEFAULT_RO_ATTR_PERMS: Self = Self {
        readable: true,
        writable: false,
    };

    /// Read-write permissions (`0644`).
    pub const DEFAULT_RW_ATTR_PERMS: Self = Self {
        readable: true,
        writable: true,
    };

    /// Write-only owner permissions (`0200`).
    pub const OWNER_W: Self = Self {
        readable: false,
        writable: true,
    };

    /// Returns whether the attribute can be read.
    pub const fn is_readable(self) -> bool {
        self.readable
    }

    /// Returns whether the attribute can be written.
    pub const fn is_writable(self) -> bool {
        self.writable
    }
}

/// Metadata for a single attribute within an [`AttrSet`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttrMeta {
    id: u8,
    name: SysStr,
    perms: SysPerms,
}

impl AttrMeta {
    /// Returns the attribute's stable local ID within its device's set.
    pub fn id(&self) -> u8 {
        self.id
    }

    /// Returns the attribute name.
    pub fn name(&self) -> &SysStr {
        &self.name
    }

    /// Returns the attribute permissions.
    pub fn perms(&self) -> SysPerms {
        self.perms
    }
}

/// Immutable snapshot of a device's published sysfs attribute metadata.
///
/// Rebuilt on bind/unbind while preserving IDs of surviving attributes so
/// sysfs inode numbers remain stable across driver binding transitions.
#[derive(Clone, Debug, Default)]
pub struct AttrSet {
    attrs: BTreeMap<SysStr, AttrMeta>,
    used_ids: [u64; 4],
}

impl AttrSet {
    /// Returns an empty attribute set.
    pub const fn empty() -> Self {
        Self {
            attrs: BTreeMap::new(),
            used_ids: [0; 4],
        }
    }

    /// Iterates over attributes in name order.
    pub fn iter(&self) -> impl Iterator<Item = &AttrMeta> {
        self.attrs.values()
    }

    /// Returns metadata for `name`, if present.
    pub fn get(&self, name: &str) -> Option<&AttrMeta> {
        self.attrs.get(name)
    }

    fn alloc_id(&mut self) -> Result<u8> {
        for (word_idx, word) in self.used_ids.iter_mut().enumerate() {
            let free = !*word;
            if free != 0 {
                let bit = free.trailing_zeros() as usize;
                let id = word_idx * 64 + bit;
                if id >= ATTR_SET_CAPACITY {
                    return Err(Error::ResourceUnavailable);
                }
                *word |= 1u64 << bit;
                return Ok(id as u8);
            }
        }
        Err(Error::ResourceUnavailable)
    }

    fn release_id(&mut self, id: u8) {
        let idx = id as usize;
        self.used_ids[idx / 64] &= !(1u64 << (idx % 64));
    }
}

/// A function that produces the text of an attribute.
pub type ShowFn<D> = fn(&D, &mut dyn Write) -> Result<()>;

/// A function that consumes the text written to an attribute.
pub type StoreFn<D> = fn(&D, &str) -> Result<()>;

/// A statically declared attribute of devices of type `D`.
pub struct Attr<D: ?Sized> {
    name: &'static str,
    perms: SysPerms,
    show: Option<ShowFn<D>>,
    store: Option<StoreFn<D>>,
}

impl<D: ?Sized> Attr<D> {
    /// Creates a read-only attribute.
    pub const fn ro(name: &'static str, show: ShowFn<D>) -> Self {
        Self {
            name,
            perms: SysPerms::DEFAULT_RO_ATTR_PERMS,
            show: Some(show),
            store: None,
        }
    }

    /// Creates a read-write attribute.
    pub const fn rw(name: &'static str, show: ShowFn<D>, store: StoreFn<D>) -> Self {
        Self {
            name,
            perms: SysPerms::DEFAULT_RW_ATTR_PERMS,
            show: Some(show),
            store: Some(store),
        }
    }

    /// Creates a write-only attribute.
    pub const fn wo(name: &'static str, store: StoreFn<D>) -> Self {
        Self {
            name,
            perms: SysPerms::OWNER_W,
            show: None,
            store: Some(store),
        }
    }

    /// Returns the name.
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// Returns the permissions.
    pub const fn perms(&self) -> SysPerms {
        self.perms
    }
}

impl<D: ?Sized> Clone for Attr<D> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<D: ?Sized> Copy for Attr<D> {}

impl<D: ?Sized> fmt::Debug for Attr<D> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Attr")
            .field("name", &self.name)
            .field("perms", &self.perms)
            .finish()
    }
}

/// An attribute whose callbacks take `&dyn AnyDevice`.
#[derive(Clone)]
pub struct TyErasedAttr {
    name: &'static str,
    perms: SysPerms,
    show: Option<TyErasedShowFn>,
    store: Option<TyErasedStoreFn>,
}

impl fmt::Debug for TyErasedAttr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TyErasedAttr")
            .field("name", &self.name)
            .field("perms", &self.perms)
            .finish()
    }
}

impl TyErasedAttr {
    /// Converts a slice of typed attributes into type-erased attributes.
    pub(crate) fn from_typed_slice<D: AnyDevice>(attrs: &[Attr<D>]) -> Vec<Self> {
        attrs.iter().map(Self::from_typed).collect()
    }

    /// Converts an attribute for devices of Rust type `D` into a type-erased attribute.
    pub(crate) fn from_typed<D: AnyDevice>(attr: &Attr<D>) -> Self {
        let show = attr.show.map(|show| -> TyErasedShowFn {
            Arc::new(move |dev: &dyn AnyDevice, w: &mut dyn Write| {
                let dev = dev.as_any().downcast_ref::<D>().unwrap();
                show(dev, w)
            })
        });
        let store = attr.store.map(|store| -> TyErasedStoreFn {
            Arc::new(move |dev: &dyn AnyDevice, s: &str| {
                let dev = dev.as_any().downcast_ref::<D>().unwrap();
                store(dev, s)
            })
        });
        Self {
            name: attr.name,
            perms: attr.perms,
            show,
            store,
        }
    }

    /// Converts a driver's attributes into type-erased attributes.
    ///
    /// Their callbacks hold the device's binding lock
    /// and check that the device is still bound to the same driver before running.
    pub(crate) fn from_driver<B: Bus>(driver: &Arc<DriverHandle<B>>) -> Vec<Self> {
        let mut attrs = Vec::new();
        for attr in driver.dev_attrs() {
            let show = attr.show.map(|show| -> TyErasedShowFn {
                let owner = Arc::downgrade(driver);
                Arc::new(move |dev, w| {
                    let dev = dev.as_any().downcast_ref::<BusDevice<B>>().unwrap();
                    let _guard = dev.bind_lock().lock();
                    let driver = dev.driver().ok_or(Error::NotBound)?;
                    if !Weak::ptr_eq(&owner, &Arc::downgrade(&driver)) {
                        return Err(Error::NotBound);
                    }
                    show(dev, w)
                })
            });
            let store = attr.store.map(|store| -> TyErasedStoreFn {
                let owner = Arc::downgrade(driver);
                Arc::new(move |dev, s| {
                    let dev = dev.as_any().downcast_ref::<BusDevice<B>>().unwrap();
                    let _guard = dev.bind_lock().lock();
                    let driver = dev.driver().ok_or(Error::NotBound)?;
                    if !Weak::ptr_eq(&owner, &Arc::downgrade(&driver)) {
                        return Err(Error::NotBound);
                    }
                    store(dev, s)
                })
            });
            attrs.push(Self {
                name: attr.name,
                perms: attr.perms,
                show,
                store,
            });
        }
        attrs
    }

    /// Wraps an attribute that already operates on `&dyn AnyDevice`.
    pub(crate) fn from_dyn(attr: &Attr<dyn AnyDevice>) -> Self {
        Self {
            name: attr.name,
            perms: attr.perms,
            show: attr.show.map(|show| -> TyErasedShowFn { Arc::new(show) }),
            store: attr
                .store
                .map(|store| -> TyErasedStoreFn { Arc::new(store) }),
        }
    }
}

/// The attributes of one device: an immutable [`AttrSet`] snapshot plus the
/// callbacks behind each attribute, protected under a single lock so the two
/// never disagree.
#[derive(Debug)]
pub(crate) struct AttrTable {
    inner: RwMutex<TableInner>,
}

impl AttrTable {
    pub(crate) fn new() -> Self {
        Self {
            inner: RwMutex::new(TableInner {
                set: Arc::new(AttrSet::empty()),
                ops: BTreeMap::new(),
            }),
        }
    }

    /// Returns the immutable attribute metadata snapshot.
    pub(crate) fn set(&self) -> Arc<AttrSet> {
        self.inner.read().set.clone()
    }

    /// Returns metadata for attribute `name`, if present.
    pub(crate) fn meta(&self, name: &str) -> Option<AttrMeta> {
        self.inner.read().set.get(name).cloned()
    }

    /// Adds attributes atomically.
    ///
    /// Fails if any name is invalid, is already present, or is repeated within
    /// `attrs`, or if the ID space is exhausted; in every case none is added.
    pub(crate) fn add(&self, attrs: Vec<TyErasedAttr>) -> Result<()> {
        let mut inner = self.inner.write();
        let mut seen = BTreeSet::new();
        for attr in &attrs {
            if !is_valid_name(attr.name) {
                return Err(Error::InvalidName);
            }
            if inner.ops.contains_key(attr.name) || !seen.insert(attr.name) {
                return Err(Error::NameConflict);
            }
        }

        let mut next_set = (*inner.set).clone();
        for attr in &attrs {
            let id = next_set.alloc_id()?;
            let key = SysStr::from(attr.name);
            next_set.attrs.insert(
                key.clone(),
                AttrMeta {
                    id,
                    name: key,
                    perms: attr.perms,
                },
            );
        }

        inner.set = Arc::new(next_set);
        for attr in attrs {
            inner.ops.insert(SysStr::from(attr.name), attr);
        }
        Ok(())
    }

    /// Removes attributes by name; absent names are ignored.
    pub(crate) fn remove(&self, names: &[&'static str]) {
        let mut inner = self.inner.write();
        let mut next_set = (*inner.set).clone();
        for name in names {
            inner.ops.remove(*name);
            if let Some(meta) = next_set.attrs.remove(*name) {
                next_set.release_id(meta.id);
            }
        }
        inner.set = Arc::new(next_set);
    }

    /// Renders attribute `name` for `dev` into a UTF-8 string.
    pub(crate) fn show_string(&self, dev: &dyn AnyDevice, name: &str) -> Result<String> {
        let show = {
            let inner = self.inner.read();
            let attr = inner.ops.get(name).ok_or(Error::NotFound)?;
            attr.show.clone().ok_or(Error::Attribute)?
        };
        let mut out = String::new();
        show(dev, &mut out)?;
        Ok(out)
    }

    /// Writes `text` to attribute `name` for `dev`.
    pub(crate) fn store_str(&self, dev: &dyn AnyDevice, name: &str, text: &str) -> Result<()> {
        let store = {
            let inner = self.inner.read();
            let attr = inner.ops.get(name).ok_or(Error::NotFound)?;
            attr.store.clone().ok_or(Error::Attribute)?
        };
        store(dev, text)
    }
}

#[derive(Debug)]
struct TableInner {
    set: Arc<AttrSet>,
    ops: BTreeMap<SysStr, TyErasedAttr>,
}

type TyErasedShowFn = Arc<dyn Fn(&dyn AnyDevice, &mut dyn Write) -> Result<()> + Send + Sync>;
type TyErasedStoreFn = Arc<dyn Fn(&dyn AnyDevice, &str) -> Result<()> + Send + Sync>;
