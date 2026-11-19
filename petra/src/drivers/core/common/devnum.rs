// SPDX-License-Identifier: MPL-2.0
//
// Ported from Asterinas `aster-device` (`kernel/core/comps/device/src`):
// https://github.com/asterinas/asterinas/tree/e36b5b3b759e616febf2c8f0e88d8cb2c103f266/kernel/core/comps/device/src
// Copyright (c) Asterinas contributors. Adapted for PetraOS.

//! Device numbers (`major:minor`) and device node kinds.

use core::fmt;

/// Major and minor device number pair.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct DeviceId {
    major: u32,
    minor: u32,
}

impl DeviceId {
    /// Creates a new `DeviceId` from major and minor numbers.
    pub const fn new(major: u32, minor: u32) -> Self {
        Self { major, minor }
    }

    /// Returns the major device number.
    pub const fn major(self) -> u32 {
        self.major
    }

    /// Returns the minor device number.
    pub const fn minor(self) -> u32 {
        self.minor
    }

    /// Encodes the major/minor pair into the Linux `dev_t` representation.
    pub const fn to_raw(self) -> u64 {
        crate::uapi::dev::makedev(self.major, self.minor)
    }
}

/// Whether a device number names a character or a block device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DevKind {
    Char,
    Block,
}

/// A device number together with its kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DevNum {
    kind: DevKind,
    id: DeviceId,
}

impl DevNum {
    /// Creates a character device number.
    pub const fn char(id: DeviceId) -> Self {
        Self {
            kind: DevKind::Char,
            id,
        }
    }

    /// Creates a block device number.
    pub const fn block(id: DeviceId) -> Self {
        Self {
            kind: DevKind::Block,
            id,
        }
    }

    /// Returns the device kind.
    pub const fn kind(&self) -> DevKind {
        self.kind
    }

    /// Returns the major and minor number.
    pub const fn id(&self) -> DeviceId {
        self.id
    }
}

/// The default mode of a device node when neither the device type nor the class overrides it.
pub const DEFAULT_DEVNODE_MODE: u16 = 0o600;

impl fmt::Display for DevNum {
    /// Formats as `major:minor`, the form used by `/sys/dev` and the `dev` attribute.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.id.major(), self.id.minor())
    }
}
