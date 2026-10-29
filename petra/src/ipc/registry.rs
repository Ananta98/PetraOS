// SPDX-License-Identifier: GPL-2.0

//! Generic registry for System V IPC object families (shm, sem, msg).
//!
//! Every family keeps the same two tables: live objects indexed by an
//! unpredictable id, and the caller key → id map that `get(2)` flows use to
//! find or claim existing objects. [`IpcRegistry`] implements those tables
//! once; flows holding [`IpcRegistry::lock`] keep their multi-step create /
//! destroy logic atomic, and the private-key handling for the key table is
//! centralized in [`bind_key`](IpcRegistry::bind_key) and
//! [`unbind_key`](IpcRegistry::unbind_key).

use alloc::{collections::BTreeMap, sync::Arc};
use ostd::sync::{PreemptDisabled, SpinLock, SpinLockGuard};

use crate::uapi::ipc::IPC_PRIVATE;

/// Id- and key-indexed registry for one System V IPC object family.
///
/// `T` mirrors the type each family stores (`Arc`-wrapped contents beyond
/// that, e.g. `SpinLock<MsgQueue>`).
pub struct IpcRegistry<T: ?Sized> {
    objects: SpinLock<BTreeMap<i32, Arc<T>>>,
    keys: SpinLock<BTreeMap<i32, i32>>,
}

impl<T: ?Sized> IpcRegistry<T> {
    /// Creates an empty registry.
    pub const fn new() -> Self {
        Self {
            objects: SpinLock::new(BTreeMap::new()),
            keys: SpinLock::new(BTreeMap::new()),
        }
    }

    /// Exclusive access to the id-indexed object table.
    pub fn lock(&self) -> SpinLockGuard<'_, BTreeMap<i32, Arc<T>>, PreemptDisabled> {
        self.objects.lock()
    }

    /// Clones the object registered under `id`.
    pub fn get(&self, id: i32) -> Option<Arc<T>> {
        self.objects.lock().get(&id).cloned()
    }

    /// The id currently bound to `key`.
    pub fn lookup_key(&self, key: i32) -> Option<i32> {
        self.keys.lock().get(&key).copied()
    }

    /// Binds `key` to `id`; `IPC_PRIVATE` objects stay unbound.
    pub fn bind_key(&self, key: i32, id: i32) {
        if key != IPC_PRIVATE {
            self.keys.lock().insert(key, id);
        }
    }

    /// Drops the binding for `key`.
    pub fn unbind_key(&self, key: i32) {
        if key != IPC_PRIVATE {
            self.keys.lock().remove(&key);
        }
    }
}
