// SPDX-License-Identifier: GPL-2.0

//! Device identity and lifecycle state shared by bus and class devices.

use alloc::string::String;
use spin::Mutex;

use super::privacy::Sealed;
use crate::uapi::errno::{Errno, Result};

/// A type-erased view of a kernel device.
///
/// Bus devices and class devices alike carry a [`DeviceBase`], which gives
/// every device shared rules for naming and registration.
pub trait AnyDevice: Send + Sync + Sealed {
    /// Returns the shared identity and lifecycle of this device.
    fn base(&self) -> &DeviceBase;

    /// Returns the unique name of the device.
    fn name(&self) -> &str {
        self.base().name()
    }
}

/// The lifecycle of a device; every transition happens exactly once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DeviceState {
    /// Built but not yet registered with a bus or class.
    Unregistered,
    /// Registered; consumers can find it through its bus or class handle.
    Registered,
    /// Unregistered; the object may still be referenced but is dead.
    Removed,
}

/// Identity and lifecycle state shared by every device.
pub struct DeviceBase {
    name: String,
    state: Mutex<DeviceState>,
}

impl DeviceBase {
    /// Creates the shared state for a device named `name`.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            state: Mutex::new(DeviceState::Unregistered),
        }
    }

    /// Returns the device name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the current lifecycle state.
    pub(crate) fn state(&self) -> DeviceState {
        *self.state.lock()
    }

    /// Marks the device registered.
    ///
    /// Fails if the device is already registered or has been removed; a
    /// failed registration leaves the state untouched.
    pub(crate) fn register(&self) -> Result<()> {
        let mut state = self.state.lock();
        match *state {
            DeviceState::Unregistered => {
                *state = DeviceState::Registered;
                Ok(())
            }
            DeviceState::Registered => Err(Errno::EEXIST),
            DeviceState::Removed => Err(Errno::ENXIO),
        }
    }

    /// Marks the device unregistered.
    ///
    /// Fails if the device was never registered or has already been removed.
    pub(crate) fn unregister(&self) -> Result<()> {
        let mut state = self.state.lock();
        match *state {
            DeviceState::Registered => {
                *state = DeviceState::Removed;
                Ok(())
            }
            _ => Err(Errno::ENODEV),
        }
    }
}
