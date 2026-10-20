// SPDX-License-Identifier: GPL-2.0

//! Random number generation system calls.

use crate::api::errno::{Errno, Result};
use crate::api::random::*;
use crate::proc::thread::Thread;
use crate::utils::userspace::UserPtr;
use ostd::arch::cpu::context::UserContext;

/// Maximum bytes allowed in a single `getrandom(2)` invocation (32MB - 1).
const MAX_RANDOM_BYTES: usize = 33554431;

/// Handles `getrandom(2)` system call (`SYS_GETRANDOM = 318`).
///
/// Fills `buf` with `buflen` cryptographically secure random bytes.
pub fn sys_getrandom(
    buf: usize,
    buflen: usize,
    flags: u32,
    _ctx: &mut UserContext,
) -> Result<usize> {
    if flags & !GRND_VALID_FLAGS_MASK != 0 {
        return Err(Errno::EINVAL);
    }

    if (flags & GRND_RANDOM != 0) && (flags & GRND_INSECURE != 0) {
        return Err(Errno::EINVAL);
    }

    if buflen == 0 {
        return Ok(0);
    }

    if buflen > MAX_RANDOM_BYTES {
        return Err(Errno::EINVAL);
    }

    let insecure = (flags & GRND_INSECURE) != 0;
    let nonblock = (flags & GRND_NONBLOCK) != 0;

    if !crate::crypto::is_initialized() && !insecure {
        if nonblock {
            return Err(Errno::EAGAIN);
        }
        crate::crypto::ensure_initialized();
    }

    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let vmar = proc.vmspace();

    const CHUNK_SIZE: usize = 4096;
    let mut chunk = [0u8; CHUNK_SIZE];
    let mut written = 0;

    while written < buflen {
        let to_write = (buflen - written).min(CHUNK_SIZE);
        crate::crypto::get_random_bytes(&mut chunk[..to_write]);
        UserPtr::new(buf + written).write_bytes(&vmar, &chunk[..to_write])?;
        written += to_write;
    }

    Ok(buflen)
}
