// SPDX-License-Identifier: MPL-2.0
//
// Ported from Asterinas `aster-device` (`kernel/core/comps/device/src`):
// https://github.com/asterinas/asterinas/tree/e36b5b3b759e616febf2c8f0e88d8cb2c103f266/kernel/core/comps/device/src
// Copyright (c) Asterinas contributors. Adapted for PetraOS.

//! Errors reported by the device model and conversions to kernel `Errno`.

use core::fmt;

use crate::uapi::errno::Errno;

/// Errors reported by the device model.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// The object has already been added, or is being added.
    AlreadyAdded,
    /// The device has not been added, or has already been removed.
    NotAdded,
    /// The parent device is not currently added.
    ParentNotAdded,
    /// The device still has child devices.
    HasChildren,
    /// A sibling with the same name already exists.
    NameConflict,
    /// The named object does not exist.
    NotFound,
    /// The name is empty, `.`, `..`, or contains `/` or `NUL`.
    InvalidName,
    /// No driver accepted the device.
    NoDriver,
    /// The device is already bound to a driver.
    AlreadyBound,
    /// The device is not bound to a driver.
    NotBound,
    /// The driver rejected the device in `on_probe`.
    ProbeFailed,
    /// The driver has been unregistered.
    DriverUnregistered,
    /// An attribute callback failed.
    Attribute,
    /// The device has no device number, so it has no `dev` attribute.
    NoDevNum,
    /// Formatting an attribute value failed.
    Format,
    /// The value written to an attribute is invalid.
    InvalidValue,
    /// A resource (such as attribute IDs) is exhausted.
    ResourceUnavailable,
    /// The kernel hook failed to create or delete a device node.
    Hook,
}

/// Result alias for device-model operations.
pub type Result<T, E = Error> = core::result::Result<T, E>;

impl Error {
    /// Converts a kernel `Errno` from a driver probe or callback into a device
    /// model error.
    pub fn from_errno(errno: Errno) -> Self {
        match errno {
            Errno::EEXIST => Self::NameConflict,
            Errno::ENOENT => Self::NotFound,
            Errno::EBUSY => Self::AlreadyBound,
            Errno::ENODEV => Self::NoDriver,
            Errno::EINVAL => Self::InvalidValue,
            Errno::ENOSPC => Self::ResourceUnavailable,
            _ => Self::ProbeFailed,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::AlreadyAdded => write!(f, "the object has already been added"),
            Error::NotAdded => write!(f, "the device has not been added"),
            Error::ParentNotAdded => write!(f, "the parent device has not been added"),
            Error::HasChildren => write!(f, "the device still has child devices"),
            Error::NameConflict => write!(f, "a sibling with the same name exists"),
            Error::NotFound => write!(f, "the object does not exist"),
            Error::InvalidName => write!(f, "the name is invalid"),
            Error::NoDriver => write!(f, "no driver accepted the device"),
            Error::AlreadyBound => write!(f, "the device is already bound to a driver"),
            Error::NotBound => write!(f, "the device is not bound to a driver"),
            Error::ProbeFailed => write!(f, "the driver rejected the device"),
            Error::DriverUnregistered => write!(f, "the driver has been unregistered"),
            Error::NoDevNum => write!(f, "the device has no device number"),
            Error::Format => write!(f, "formatting an attribute value failed"),
            Error::Attribute => write!(f, "attribute operation failed"),
            Error::InvalidValue => write!(f, "invalid attribute value"),
            Error::ResourceUnavailable => write!(f, "resource unavailable"),
            Error::Hook => write!(f, "kernel hook failed"),
        }
    }
}

impl From<fmt::Error> for Error {
    fn from(_: fmt::Error) -> Self {
        Error::Format
    }
}

impl From<Error> for Errno {
    fn from(err: Error) -> Self {
        match err {
            Error::AlreadyAdded | Error::NameConflict => Errno::EEXIST,
            Error::NotAdded | Error::NoDriver | Error::NotBound => Errno::ENODEV,
            Error::ParentNotAdded | Error::NotFound => Errno::ENOENT,
            Error::HasChildren | Error::AlreadyBound => Errno::EBUSY,
            Error::InvalidName | Error::InvalidValue | Error::Format => Errno::EINVAL,
            Error::DriverUnregistered | Error::NoDevNum => Errno::ENXIO,
            Error::ResourceUnavailable => Errno::ENOSPC,
            Error::Attribute | Error::ProbeFailed | Error::Hook => Errno::EIO,
        }
    }
}
