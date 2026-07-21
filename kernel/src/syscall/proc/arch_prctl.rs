use crate::proc::thread_local::{get_fs_base, get_gs_base, set_fs_base, set_gs_base};
use crate::syscall::{SyscallResult, to_continue_unit};
use crate::vm::vma::VmaManager;
use ostd::Error;
use ostd::arch::cpu::context::UserContext;

const ARCH_SET_GS: usize = 0x1001;
const ARCH_SET_FS: usize = 0x1002;
const ARCH_GET_FS: usize = 0x1003;
const ARCH_GET_GS: usize = 0x1004;

pub fn syscall_arch_prctl(
    code: usize,
    addr: usize,
    _arg2: usize,
    _arg3: usize,
    _arg4: usize,
    _arg5: usize,
    vm: &VmaManager,
    _context: &mut UserContext,
) -> SyscallResult {
    let result = match code {
        ARCH_SET_FS => {
            set_fs_base(addr);
            Ok(())
        }
        ARCH_GET_FS => {
            let fs_base = get_fs_base();
            let addr_buf = fs_base.to_le_bytes();
            vm.copy_to_user(addr, &addr_buf)
        }
        ARCH_SET_GS => {
            set_gs_base(addr);
            Ok(())
        }
        ARCH_GET_GS => {
            let gs_base = get_gs_base();
            let addr_buf = gs_base.to_le_bytes();
            vm.copy_to_user(addr, &addr_buf)
        }
        _ => Err(Error::InvalidArgs),
    };
    to_continue_unit(result)
}
