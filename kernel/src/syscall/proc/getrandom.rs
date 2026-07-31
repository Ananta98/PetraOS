use crate::drivers::timer::{Timer, Tsc};
use crate::syscall::{SyscallResult, to_continue_i32};
use crate::vm::vma::VmaManager;
use ostd::Error;
use ostd::arch::cpu::context::UserContext;

/// `getrandom()` — SYS_getrandom = 318
pub fn syscall_getrandom(
    buf_ptr: usize,
    buflen: usize,
    _flags: usize,
    _: usize,
    _: usize,
    _: usize,
    vm: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    if buf_ptr == 0 {
        return to_continue_i32(Err(Error::InvalidArgs));
    }
    if buflen == 0 {
        return to_continue_i32(Ok(0));
    }

    let seed = Tsc::new().current_time_ns();
    let mut state = seed;
    let mut dummy_buf = alloc::vec![0u8; buflen];
    for byte in dummy_buf.iter_mut() {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        *byte = (state >> 32) as u8;
    }

    if vm.copy_to_user(buf_ptr, &dummy_buf).is_err() {
        return to_continue_i32(Err(Error::InvalidArgs));
    }

    to_continue_i32(Ok(buflen as i32))
}
