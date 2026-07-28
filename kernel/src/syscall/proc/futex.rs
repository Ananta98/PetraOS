use crate::syscall::{SyscallResult, to_continue_i32};
use crate::vm::vma::VmaManager;
use ostd::Error;
use ostd::arch::cpu::context::UserContext;

pub const FUTEX_WAIT: usize = 0;
pub const FUTEX_WAKE: usize = 1;
pub const FUTEX_REQUEUE: usize = 3;
pub const FUTEX_CMP_REQUEUE: usize = 4;
pub const FUTEX_PRIVATE_FLAG: usize = 128;

/// `futex()` — SYS_futex = 202
pub fn syscall_futex(
    uaddr: usize,
    op: usize,
    val: usize,
    _timeout_ptr: usize,
    _uaddr2: usize,
    _val3: usize,
    vm: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    if uaddr == 0 {
        return to_continue_i32(Err(Error::InvalidArgs));
    }

    let cmd = op & !FUTEX_PRIVATE_FLAG;

    match cmd {
        FUTEX_WAIT => {
            let mut current_val_bytes = [0u8; 4];
            if vm.copy_from_user(uaddr, &mut current_val_bytes).is_err() {
                return to_continue_i32(Err(Error::InvalidArgs));
            }
            let current_val = u32::from_ne_bytes(current_val_bytes) as usize;
            if current_val != val {
                return to_continue_i32(Err(Error::InvalidArgs));
            }
            to_continue_i32(Ok(0))
        }
        FUTEX_WAKE => {
            let count = val as i32;
            to_continue_i32(Ok(count.min(1)))
        }
        FUTEX_REQUEUE | FUTEX_CMP_REQUEUE => to_continue_i32(Ok(0)),
        _ => to_continue_i32(Ok(0)),
    }
}
