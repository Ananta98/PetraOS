use crate::syscall::SyscallResult;
use crate::vm::vma::{AdviseFlag, VmaManager};
use ostd::Error;
use ostd::arch::cpu::context::UserContext;

const MADV_NORMAL: usize = 0;
const MADV_RANDOM: usize = 1;
const MADV_SEQUENTIAL: usize = 2;
const MADV_WILLNEED: usize = 3;
const MADV_DONTNEED: usize = 4;

pub fn syscall_madvise(
    arg0: usize,
    arg1: usize,
    arg2: usize,
    _arg3: usize,
    _arg4: usize,
    _arg5: usize,
    vm: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    let start = arg0;
    let length = arg1;
    let advice_raw = arg2;

    let advice = match advice_raw {
        MADV_NORMAL => AdviseFlag::Normal,
        MADV_RANDOM => AdviseFlag::Random,
        MADV_SEQUENTIAL => AdviseFlag::Sequential,
        MADV_WILLNEED => AdviseFlag::WillNeed,
        MADV_DONTNEED => AdviseFlag::DontNeed,
        _ => return SyscallResult::Return(-(Error::InvalidArgs as isize) as usize),
    };

    match vm.madvise(start, length, advice) {
        Ok(()) => SyscallResult::Return(0),
        Err(e) => SyscallResult::Return(-(e as isize) as usize),
    }
}
