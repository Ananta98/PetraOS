// SPDX-License-Identifier: GPL-2.0

//! System V Semaphore Set representations.

use alloc::{sync::Arc, vec::Vec};
use ostd::sync::{SpinLock, WaitQueue};

use crate::uapi::sem::{SEMMSL, SemIdDs};

/// A single semaphore within a set.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Semaphore {
    /// Current semaphore value (0..=SEMVMX).
    pub val: u16,
    /// Process ID of last operation (0 if none yet).
    pub pid: u32,
    /// Number of threads waiting for semval to increase.
    pub ncnt: u16,
    /// Number of threads waiting for semval to become zero.
    pub zcnt: u16,
}

/// A System V semaphore set holding multiple semaphores.
pub struct SemSet {
    /// Unique identifier for this semaphore set.
    pub semid: i32,
    /// Key associated with this set (or IPC_PRIVATE).
    pub key: i32,
    /// Number of semaphores in this set.
    pub nsems: usize,
    /// Standard metadata and permissions.
    pub ds: SemIdDs,
    /// Array of individual semaphores.
    pub sems: Vec<Semaphore>,
    /// Wait queue for blocking `semop` threads.
    pub wq: WaitQueue,
    /// True if set has been marked destroyed via IPC_RMID.
    pub destroyed: bool,
}

impl SemSet {
    /// Creates a new semaphore set with `nsems` semaphores initialized to 0.
    pub fn new(semid: i32, key: i32, nsems: usize, ds: SemIdDs) -> Arc<SpinLock<Self>> {
        assert!(nsems <= SEMMSL, "nsems exceeds SEMMSL");
        let mut sems = Vec::with_capacity(nsems);
        sems.resize(nsems, Semaphore::default());

        Arc::new(SpinLock::new(Self {
            semid,
            key,
            nsems,
            ds,
            sems,
            wq: WaitQueue::new(),
            destroyed: false,
        }))
    }
}
