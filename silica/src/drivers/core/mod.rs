// SPDX-License-Identifier: GPL-2.0

//! The kernel device model: device identity, buses, and classes.
//!
//! Every concrete device joins the kernel through one of the generic
//! registries defined here:
//!
//! - A [`bus::Bus`] owns devices discovered by enumeration (PCI, USB, ...).
//!   It matches them against registered [`bus::Driver`]s and drives the
//!   probe/bind/release life cycle of every binding.
//! - A [`class::Class`] is what a device looks like to user space regardless
//!   of which bus provided it (`char`, `block`, `net`, ...). It is the single
//!   place that stores, finds, and unregisters devices of one kind, replacing
//!   per-subsystem global device lists.
//!
//! Both share one identity and lifecycle type ([`base::DeviceBase`]), so the
//! same registration rules (a device registers exactly once, and cannot be
//! re-registered after removal) apply everywhere.

pub mod base;
pub mod bus;
pub mod class;

pub use base::{AnyDevice, DeviceBase};
pub use bus::{Bus, BusDevice, BusHandle, Driver};
pub use class::{Class, ClassDevice, ClassHandle};

mod privacy {
    /// Seals [`AnyDevice`]; only the device model may implement it.
    pub trait Sealed {}
}
