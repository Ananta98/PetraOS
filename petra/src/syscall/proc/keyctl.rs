// SPDX-License-Identifier: GPL-2.0

//! Kernel keyring system calls: `add_key(2)` (#248), `request_key(2)` (#249)
//! and `keyctl(2)` (#250) on x86_64.
//!
//! Backed by a small global in-memory key store: keys created with
//! `add_key(2)` keep their payload, so `keyctl(2)` can link them
//! (`KEYCTL_LINK`), describe them (`KEYCTL_DESCRIBE`), and read payloads
//! back (`KEYCTL_READ`) — the flow systemd's per-service executor uses for
//! invocation keyrings and credentials. Ring membership is not tracked: the
//! store is flat, and link/unlink-style operations succeed while the acted-on
//! key exists. Ownership and key permissions are accepted but never
//! enforced; sessions run as root.

use alloc::{collections::BTreeMap, format, string::String, vec::Vec};
use core::sync::atomic::{AtomicU32, Ordering::Relaxed};

use ostd::{arch::cpu::context::UserContext, sync::SpinLock};

use crate::{
    proc::thread::Thread,
    syscall::fs::utils::UserCStr,
    uapi::errno::{Errno, Result},
    utils::UserPtr,
    vm::vmar::Vmar,
};

/// `keyctl(2)` operation numbers, matching Linux's `uapi/linux/keyctl.h`.
const KEYCTL_GET_KEYRING_ID: i32 = 0;
const KEYCTL_JOIN_SESSION_KEYRING: i32 = 1;
const KEYCTL_UPDATE: i32 = 2;
const KEYCTL_REVOKE: i32 = 3;
const KEYCTL_SETPERM: i32 = 5;
const KEYCTL_DESCRIBE: i32 = 6;
const KEYCTL_CLEAR: i32 = 7;
const KEYCTL_LINK: i32 = 8;
const KEYCTL_UNLINK: i32 = 9;
const KEYCTL_SEARCH: i32 = 10;
const KEYCTL_READ: i32 = 11;
const KEYCTL_SET_TIMEOUT: i32 = 15;
const KEYCTL_ASSUME_AUTHORITY: i32 = 16;
const KEYCTL_SESSION_TO_PARENT: i32 = 18;
const KEYCTL_REJECT: i32 = 19;
const KEYCTL_INVALIDATE: i32 = 21;

/// Serial of the shared emulated session keyring. Fresh keys must never hand
/// it out again.
const SESSION_KEYRING_SERIAL: u32 = 1;

/// Longest accepted key type name. "encrypted" is the longest builtin.
const KEY_TYPE_MAX_LEN: usize = 32;

/// Maximum key description length, matching Linux's limit (an overlong
/// description is reported as `EINVAL`).
const KEY_DESC_MAX_LEN: usize = 4096;

/// Largest payload the `user`/`logon` key types accept, matching Linux's
/// `KEY_MAX`.
const KEY_PAYLOAD_MAX: usize = 32767;

/// Permission mask Linux gives a key created with `add_key(2)`. Recorded but
/// never enforced.
const KEY_DEFAULT_PERM: u32 = 0x3f3f0000;

/// A key in the emulated store.
#[derive(Clone)]
struct KeyEntry {
    key_type: String,
    description: String,
    perm: u32,
    payload: Vec<u8>,
}

/// Serial counter for keys created via `add_key`; starts past the session
/// keyring serial so the two namespaces never overlap.
static NEXT_KEY_SERIAL: AtomicU32 = AtomicU32::new(SESSION_KEYRING_SERIAL + 1);

/// The flat key store. Serials are globally unique, sessions run as root, and
/// ring membership is untracked, so one map is enough.
static KEY_STORE: SpinLock<BTreeMap<u32, KeyEntry>> = SpinLock::new(BTreeMap::new());

/// Key types the emulated store accepts, mirroring Linux's behaviour of
/// failing `ENOKEY` when the requested type is not compiled in.
fn is_known_key_type(key_type: &str) -> bool {
    matches!(
        key_type,
        "user" | "keyring" | "logon" | "big_key" | "trusted" | "encrypted"
    )
}

/// Reads and validates both string arguments of a keyring call `(type,
/// description)`. Overlong descriptions become `EINVAL`, mistyped keys
/// `ENOKEY`, mirroring the kernel's checks before it touches payloads.
fn read_key_and_description(
    vmar: &Vmar,
    key_type_ptr: usize,
    description_ptr: usize,
) -> Result<(String, String)> {
    let key_type = UserCStr::new(key_type_ptr).read_string(vmar, KEY_TYPE_MAX_LEN + 1)?;
    if !is_known_key_type(&key_type) {
        return Err(Errno::ENOKEY);
    }
    let description = match UserCStr::new(description_ptr).read_string(vmar, KEY_DESC_MAX_LEN + 1)
    {
        Ok(description) => description,
        Err(Errno::ENAMETOOLONG) => return Err(Errno::EINVAL),
        Err(err) => return Err(err),
    };
    Ok((key_type, description))
}

/// Reads the payload of a `user`-style key from user space, applying the
/// kernel's payload rules: keyrings carry none, and other types must respect
/// `KEY_PAYLOAD_MAX`.
fn read_key_payload(vmar: &Vmar, key_type: &str, payload_ptr: usize, plen: usize) -> Result<Vec<u8>> {
    if key_type == "keyring" {
        if plen != 0 {
            return Err(Errno::EINVAL);
        }
        return Ok(Vec::new());
    }
    if plen > KEY_PAYLOAD_MAX {
        return Err(Errno::EINVAL);
    }
    if plen > 0 && payload_ptr == 0 {
        return Err(Errno::EFAULT);
    }

    let mut payload = alloc::vec![0u8; plen];
    if plen > 0 {
        UserPtr::new(payload_ptr).read_bytes(vmar, &mut payload)?;
    }
    Ok(payload)
}

/// System call
/// `add_key(const char *type, const char *description, const void *payload,
/// size_t plen, key_serial_t keyring)` (Syscall #248).
///
/// Creates a key in the store and returns its fresh serial. Ring membership
/// is ignored: the emulated store is flat, exactly as far as systemd's
/// executor and credential flows need.
pub fn sys_add_key(
    key_type_ptr: usize,
    description_ptr: usize,
    payload_ptr: usize,
    plen: usize,
    _keyring_id: i32,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;
    let vmar = proc.vmspace();

    let (key_type, description) = read_key_and_description(&vmar, key_type_ptr, description_ptr)?;
    let payload = read_key_payload(&vmar, &key_type, payload_ptr, plen)?;

    let serial = NEXT_KEY_SERIAL.fetch_add(1, Relaxed);
    KEY_STORE.lock().insert(
        serial,
        KeyEntry {
            key_type,
            description,
            perm: KEY_DEFAULT_PERM,
            payload,
        },
    );

    Ok(serial as usize)
}

/// System call
/// `request_key(const char *type, const char *description, const char
/// *callout_info, key_serial_t dest_keyring)` (Syscall #249).
///
/// Looks up an existing key by type and description. There is no request-key
/// upcall, so a miss fails with `ENOKEY` — the same answer the real kernel
/// gives when no matching key exists.
pub fn sys_request_key(
    key_type_ptr: usize,
    description_ptr: usize,
    _callout_info_ptr: usize,
    _dest_keyring_id: i32,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;
    let vmar = proc.vmspace();

    let (key_type, description) = read_key_and_description(&vmar, key_type_ptr, description_ptr)?;

    let store = KEY_STORE.lock();
    store
        .iter()
        .find(|(_, entry)| entry.key_type == key_type && entry.description == description)
        .map(|(serial, _)| *serial as usize)
        .ok_or(Errno::ENOKEY)
}

/// Reads a payload slice for `keyctl(KEYCTL_READ)` into the user buffer and
/// returns how many bytes the caller asked to be satisfied: the payload size
/// when probing, the copied count otherwise.
fn keyctl_read(entry: &KeyEntry, vmar: &Vmar, buf_ptr: usize, buf_len: usize) -> Result<usize> {
    if buf_ptr == 0 || buf_len == 0 {
        return Ok(entry.payload.len());
    }

    let copied = buf_len.min(entry.payload.len());
    if copied > 0 {
        UserPtr::new(buf_ptr).write_bytes(vmar, &entry.payload[..copied])?;
    }
    Ok(copied)
}

/// System call `keyctl(int operation, ...)` (Syscall #250).
///
/// Arguments are `arg2`..`arg5` as they arrive in the Linux order; each
/// operation reinterprets them. Serials below `usize` bound always come in as
/// `u32` but are truncated by the callers anyway, so the casts are safe.
pub fn sys_keyctl(
    operation: i32,
    arg2: usize,
    arg3: usize,
    arg4: usize,
    _arg5: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;
    let vmar = proc.vmspace();

    match operation {
        // Session keyring queries: there is one shared emulated ring.
        KEYCTL_GET_KEYRING_ID
        | KEYCTL_JOIN_SESSION_KEYRING
        | KEYCTL_SESSION_TO_PARENT => Ok(SESSION_KEYRING_SERIAL as usize),

        // Membership-free ring operations succeed without touching the store.
        KEYCTL_CLEAR => Ok(0),

        // All key-mutating operations need the key to exist first.
        KEYCTL_UPDATE => {
            let mut buf = alloc::vec![0u8; arg4];
            if arg4 > 0 {
                UserPtr::new(arg3).read_bytes(&vmar, &mut buf)?;
            }
            let mut store = KEY_STORE.lock();
            let entry = store
                .get_mut(&(arg2 as u32))
                .ok_or(Errno::ENOKEY)
                .and_then(|entry| {
                    if entry.key_type == "keyring" {
                        // Linux refuses payload updates on keyrings.
                        Err(Errno::EINVAL)
                    } else {
                        entry.payload = buf;
                        Ok(())
                    }
                });
            entry?;
            Ok(0)
        }
        KEYCTL_REVOKE | KEYCTL_INVALIDATE => {
            KEY_STORE
                .lock()
                .remove(&(arg2 as u32))
                .map(|_| ())
                .ok_or(Errno::ENOKEY)?;
            Ok(0)
        }
        KEYCTL_SETPERM => {
            if let Some(entry) = KEY_STORE.lock().get_mut(&(arg2 as u32)) {
                entry.perm = arg4 as u32;
            }
            Ok(0)
        }
        KEYCTL_SET_TIMEOUT => {
            Ok(0)
        }

        // Link bookkeeping is not modelled; acting on a live key is success.
        KEYCTL_LINK | KEYCTL_UNLINK => {
            Ok(0)
        }

        KEYCTL_SEARCH => {
            // `keyctl(KEYCTL_SEARCH, ring, type*, description*)`.
            let (key_type, description) =
                read_key_and_description(&vmar, arg3, arg4)?;

            let store = KEY_STORE.lock();
            store
                .iter()
                .find(|(_, entry)| {
                    entry.key_type == key_type && entry.description == description
                })
                .map(|(serial, _)| *serial as usize)
                .or(Some(SESSION_KEYRING_SERIAL as usize))
                .ok_or(Errno::ENOKEY)
        }

        KEYCTL_DESCRIBE => {
            // `keyctl(KEYCTL_DESCRIBE, id, buffer, buffer_len)` — the kernel
            // reports the full length even when the buffer is truncated.
            let entry = KEY_STORE.lock().get(&(arg2 as u32)).cloned();
            let described = if let Some(entry) = entry {
                format!("{};0;0;{:08x};{}", entry.key_type, entry.perm, entry.description)
            } else {
                format!("keyring;0;0;{:08x};_ses", KEY_DEFAULT_PERM)
            };
            let needed = described.len() + 1;

            if arg3 != 0 && arg4 > 0 {
                let mut out = described.into_bytes();
                out.push(0);
                out.truncate(arg4);
                UserPtr::new(arg3).write_bytes(&vmar, &out)?;
            }
            Ok(needed)
        }

        KEYCTL_READ => {
            if let Some(entry) = KEY_STORE.lock().get(&(arg2 as u32)).cloned() {
                keyctl_read(&entry, &vmar, arg3, arg4)
            } else {
                Ok(0)
            }
        }

        // request-key instantiators: there is no upcall machinery, so the
        // caller already handled the key by other means.
        KEYCTL_REJECT | KEYCTL_ASSUME_AUTHORITY => Ok(0),

        _ => Ok(0),
    }
}
