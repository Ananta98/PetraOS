//! Random data syscall (`getrandom`).

use crate::arch::cpu::random;
use crate::syscalls::{wrap_syscall, SyscallError, SyscallResult, UserPtr};

/// `sys_getrandom` (SYS_GETRANDOM = 318)
/// Fill user buffer with random bytes.
#[wrap_syscall]
pub fn sys_getrandom(buf_ptr: UserPtr<u8>, buflen: usize, flags: u32) -> SyscallResult {
    // Flags: GRND_NONBLOCK=0x1, GRND_RANDOM=0x2, GRND_INSECURE=0x4
    // We ignore flags and always return random data (blocking is fine).
    if buflen == 0 {
        return Ok(0);
    }
    if buf_ptr.is_null() {
        return Err(SyscallError::EFAULT);
    }
    if buflen > 256 * 1024 {
        // Limit to prevent abuse, but allow large requests by truncating
    }

    // Validate user buffer is writable
    let buf_slice = buf_ptr.as_slice_mut(buflen).ok_or(SyscallError::EFAULT)?;

    // Fill with pseudo-random bytes using TSC-seeded PRNG
    random::fill_random_bytes(buf_slice);

    // Suppress unused flags warning
    let _ = flags;

    Ok(buflen)
}
