// SPDX-License-Identifier: GPL-2.0

//! Reference counting in Linux `kref`/`refcount_t` shape.
//!
//! [`KRef`] is the `kref` counterpart: ownership is a `Clone`, release is
//! simply dropping the handle, and there is no way to resurrect a dead object.
//! This replaces the `kref_get`/`kref_put` pair with the borrow checker: the
//! API mistakes Linux guards against (putting twice, use-after-put) are
//! compile errors here.

use alloc::sync::{Arc, Weak};

/// Linux `kref`: an atomically shared kernel object reference.
pub type KRef<T> = Arc<T>;

/// Linux `struct kref` minus its lifetime foot-guns: a non-owning handle that
/// upgrades to [`KRef`] when the owner still exists. Useful for registries
/// keyed back by the object itself.
pub type KWeak<T> = Weak<T>;
