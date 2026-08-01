use crate::syscall::SyscallResult;
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
    SyscallResult::from_result(do_futex(uaddr, op, val, vm))
}

fn do_futex(uaddr: usize, op: usize, val: usize, vm: &VmaManager) -> Result<i32, Error> {
    if uaddr == 0 {
        return Err(Error::InvalidArgs);
    }

    let cmd = op & !FUTEX_PRIVATE_FLAG;

    match cmd {
        FUTEX_WAIT => {
            let mut current_val_bytes = [0u8; 4];
            vm.copy_from_user(uaddr, &mut current_val_bytes)
                .map_err(|_| Error::InvalidArgs)?;
            let current_val = u32::from_ne_bytes(current_val_bytes) as usize;
            if current_val != val {
                return Err(Error::InvalidArgs);
            }
            Ok(0)
        }
        FUTEX_WAKE => {
            let count = val as i32;
            Ok(count.min(1))
        }
        FUTEX_REQUEUE | FUTEX_CMP_REQUEUE => Ok(0),
        _ => Ok(0),
    }
}
