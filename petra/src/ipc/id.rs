// SPDX-License-Identifier: GPL-2.0

//! Unpredictable IPC Identifier Generator using SipHash-2-4.
//!
//! Generates non-linear, unpredictable 31-bit positive identifiers for System V
//! shared memory (`shmid`), message queues (`msqid`), and semaphores (`semid`)
//! to prevent identifier prediction and race-condition attacks.

use alloc::collections::BTreeMap;
use core::sync::atomic::{AtomicU64, Ordering};
use ostd::sync::SpinLock;

use crate::crypto::SipHash24;

/// 128-bit secret keys for SipHash-2-4 PRF, lazy-initialized from CSPRNG.
static SECRET_KEYS: SpinLock<Option<(u64, u64)>> = SpinLock::new(None);

/// Global monotonic allocation counter.
static ALLOC_SEQ: AtomicU64 = AtomicU64::new(1);

/// Retrieves or initializes the 128-bit secret keys.
fn get_siphash_keys() -> (u64, u64) {
    let mut lock = SECRET_KEYS.lock();
    if let Some(keys) = *lock {
        keys
    } else {
        let mut k0_bytes = [0u8; 8];
        let mut k1_bytes = [0u8; 8];
        crate::crypto::get_random_bytes(&mut k0_bytes);
        crate::crypto::get_random_bytes(&mut k1_bytes);
        let k0 = u64::from_ne_bytes(k0_bytes);
        let k1 = u64::from_ne_bytes(k1_bytes);
        *lock = Some((k0, k1));
        (k0, k1)
    }
}

/// Allocates an unpredictable, positive 31-bit IPC identifier (`1..=i32::MAX`)
/// that does not collide with any existing key in `registry`.
pub fn alloc_ipc_id<V>(registry: &BTreeMap<i32, V>) -> i32 {
    let (k0, k1) = get_siphash_keys();

    for _ in 0..1024 {
        let seq = ALLOC_SEQ.fetch_add(1, Ordering::Relaxed);
        let hash = SipHash24::hash(k0, k1, &seq.to_le_bytes());
        let mut id = ((hash as u32) & 0x7fff_ffff) as i32;
        if id <= 0 {
            id = 1;
        }
        if !registry.contains_key(&id) {
            return id;
        }
    }

    // Deterministic fallback: probe from sequence counter to guarantee termination
    let mut fallback = (ALLOC_SEQ.fetch_add(1, Ordering::Relaxed) as i32) & 0x7fff_ffff;
    if fallback <= 0 {
        fallback = 1;
    }
    while registry.contains_key(&fallback) {
        fallback = (fallback + 1) & 0x7fff_ffff;
        if fallback <= 0 {
            fallback = 1;
        }
    }
    fallback
}
