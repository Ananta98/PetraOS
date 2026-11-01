// SPDX-License-Identifier: GPL-2.0

//! Kernel keyring system calls: `add_key(2)` (#248), `request_key(2)` (#249)
//! and `keyctl(2)` (#250) on x86_64.
//!
//! There is no backing keyring store; the calls emulate the pieces systemd
//! and session managers rely on so that userspace works without fatal
//! errors: joining a session keyring returns a dummy keyring identifier,
//! created keys get unique dummy serials, and key lookups fail with `ENOKEY`
//! so callers take their normal "key not found" path.

use core::sync::atomic::{AtomicU32, Ordering::Relaxed};

use ostd::arch::cpu::context::UserContext;

use crate::{
    proc::thread::Thread,
    syscall::fs::utils::UserCStr,
    uapi::errno::{Errno, Result},
    vm::vmar::Vmar,
};

const KEYCTL_GET_KEYRING_ID: i32 = 0;
const KEYCTL_JOIN_SESSION_KEYRING: i32 = 1;
const KEYCTL_CLEAR: i32 = 7;
const KEYCTL_SESSION_TO_PARENT: i32 = 18;

/// Serial of the emulated session keyring chosen by [`sys_keyctl`]. Fresh
/// keys must never hand it out again.
const SESSION_KEYRING_SERIAL: u32 = 1;

/// Serial counter for keys created via `add_key`. Starts past the session
/// keyring serial so the two namespaces never overlap.
static NEXT_KEY_SERIAL: AtomicU32 = AtomicU32::new(SESSION_KEYRING_SERIAL + 1);

/// Longest accepted key type name. "encrypted" is the longest builtin.
const KEY_TYPE_MAX_LEN: usize = 32;

/// Maximum key description length, matching Linux's limit (an overlong
/// description is reported as `EINVAL`).
const KEY_DESC_MAX_LEN: usize = 4096;

/// Largest payload the `user`/`logon` key types accept, matching Linux's
/// `KEY_MAX`.
const KEY_PAYLOAD_MAX: usize = 32767;

/// Key types this emulated keyring accepts, mirroring Linux's behaviour of
/// failing `ENOKEY` when the requested type is not compiled in.
fn is_known_key_type(key_type: &str) -> bool {
    matches!(
        key_type,
        "user" | "keyring" | "logon" | "big_key" | "trusted" | "encrypted"
    )
}

/// Validates a key description pointer the way the real kernel does and
/// discards the string: the emulated keyring has no store to keep it in.
/// Overlong descriptions become `EINVAL`, mirroring the kernel.
fn validate_key_description(vmar: &Vmar, ptr: usize) -> Result<()> {
    let _ = match UserCStr::new(ptr).read_string(vmar, KEY_DESC_MAX_LEN + 1) {
        Ok(description) => description,
        Err(Errno::ENAMETOOLONG) => return Err(Errno::EINVAL),
        Err(err) => return Err(err),
    };
    Ok(())
}

/// System call
/// `add_key(const char *type, const char *description, const void *payload,
/// size_t plen, key_serial_t keyring)` (Syscall #248).
///
/// Creates a key in the emulated keyring: the arguments are validated like
/// the real kernel but the payload is not stored, and a fresh serial is
/// returned so callers cannot confuse the key with an existing ring.
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

    let key_type = UserCStr::new(key_type_ptr).read_string(&vmar, KEY_TYPE_MAX_LEN + 1)?;
    if !is_known_key_type(&key_type) {
        return Err(Errno::ENOKEY);
    }

    // Payload checks matching the real key types: keyrings take no payload,
    // every other type takes one bounded by `KEY_PAYLOAD_MAX`.
    if key_type == "keyring" {
        if plen != 0 {
            return Err(Errno::EINVAL);
        }
    } else if plen > 0 && payload_ptr == 0 {
        return Err(Errno::EFAULT);
    } else if plen > KEY_PAYLOAD_MAX {
        return Err(Errno::EINVAL);
    }

    validate_key_description(&vmar, description_ptr)?;

    Ok(NEXT_KEY_SERIAL.fetch_add(1, Relaxed) as usize)
}

/// System call
/// `request_key(const char *type, const char *description, const char
/// *callout_info, key_serial_t dest_keyring)` (Syscall #249).
///
/// Key lookup against the emulated keyring never finds anything, so this
/// always fails with `ENOKEY` — the same answer the real kernel gives when
/// there is no matching key and no request-key upcall is configured.
pub fn sys_request_key(
    key_type_ptr: usize,
    description_ptr: usize,
    _callout_info_ptr: usize,
    _dest_keyring_id: i32,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;
    let vmar = proc.vmspace();

    let key_type = UserCStr::new(key_type_ptr).read_string(&vmar, KEY_TYPE_MAX_LEN + 1)?;
    if !is_known_key_type(&key_type) {
        return Err(Errno::ENOKEY);
    }
    validate_key_description(&vmar, description_ptr)?;

    Err(Errno::ENOKEY)
}

/// System call `keyctl(int operation, ...)` (Syscall #250).
pub fn sys_keyctl(
    operation: i32,
    _arg2: usize,
    _arg3: usize,
    _arg4: usize,
    _arg5: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    match operation {
        KEYCTL_GET_KEYRING_ID => Ok(SESSION_KEYRING_SERIAL as usize),
        KEYCTL_JOIN_SESSION_KEYRING => Ok(SESSION_KEYRING_SERIAL as usize),
        KEYCTL_CLEAR | KEYCTL_SESSION_TO_PARENT => Ok(0),
        _ => Err(Errno::ENOSYS),
    }
}
