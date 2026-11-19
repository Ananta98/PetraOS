// SPDX-License-Identifier: MPL-2.0
//
// Ported from Asterinas `aster-device` (`kernel/core/comps/device/src`):
// https://github.com/asterinas/asterinas/tree/e36b5b3b759e616febf2c8f0e88d8cb2c103f266/kernel/core/comps/device/src
// Copyright (c) Asterinas contributors. Adapted for PetraOS.

//! Notifications of class membership changes.

use alloc::sync::Arc;

use super::{Class, ClassDevice};

/// An observer of devices joining or leaving a class.
///
/// An observer is notified when devices join or leave the class
/// through [`on_device_added`] and [`on_device_removed`], respectively.
/// When the observer is registered or unregistered,
/// the corresponding callback is also called for every device currently in the class.
/// Each device is announced once, regardless of whether it or the observer is registered first.
///
/// Callbacks are serialized with changes to this class's devices and observers.
/// To avoid deadlocks, callbacks must not add or remove devices in this class,
/// register or unregister observers of this class, bind or unbind bus devices,
/// or read or write driver attributes. Queue such work for another thread instead.
///
/// [`on_device_added`]: Self::on_device_added
/// [`on_device_removed`]: Self::on_device_removed
pub trait ClassObserver<C: Class>: Send + Sync + 'static {
    /// Handles a device joining the class or being announced when this observer is registered.
    fn on_device_added(&self, dev: &Arc<ClassDevice<C>>);

    /// Handles a device leaving the class or this observer being unregistered.
    fn on_device_removed(&self, _dev: &Arc<ClassDevice<C>>) {}
}
