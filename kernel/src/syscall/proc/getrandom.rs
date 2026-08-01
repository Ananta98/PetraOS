use crate::drivers::timer::{Timer, Tsc};
use crate::syscall::SyscallResult;
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
    SyscallResult::from_result(do_getrandom(buf_ptr, buflen, vm))
}

fn do_getrandom(buf_ptr: usize, buflen: usize, vm: &VmaManager) -> Result<i32, Error> {
    if buf_ptr == 0 {
        return Err(Error::InvalidArgs);
    }
    if buflen == 0 {
        return Ok(0);
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

    vm.copy_to_user(buf_ptr, &dummy_buf)
        .map_err(|_| Error::InvalidArgs)?;

    Ok(buflen as i32)
}
