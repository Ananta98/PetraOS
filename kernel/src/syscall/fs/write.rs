use crate::proc::process::Process;
use crate::syscall::SyscallResult;
use crate::vm::vma::VmaManager;
use ostd::Error;

/// System call entry: write to a file descriptor.
pub fn syscall_write(
    arg0: usize,
    arg1: usize,
    arg2: usize,
    _: usize,
    _: usize,
    _: usize,
    vm: &VmaManager,
    _: &mut ostd::arch::cpu::context::UserContext,
) -> SyscallResult {
    SyscallResult::from_result(do_write(arg0 as i32, arg1, arg2, vm))
}

fn do_write(fd: i32, user_buf: usize, len: usize, vm: &VmaManager) -> Result<usize, Error> {
    let mut kbuf = alloc::vec![0u8; len];
    vm.copy_from_user(user_buf, &mut kbuf)
        .map_err(|_| Error::AccessDenied)?;

    // Debug trap: Intercept and log writes to stdout (1) and stderr (2)
    if fd == 1 || fd == 2 {
        if let Ok(s) = core::str::from_utf8(&kbuf) {
            ostd::early_println!("[SYS_WRITE TRAP fd={}] {}", fd, s);
        } else {
            ostd::early_println!("[SYS_WRITE TRAP fd={}] {:?}", fd, kbuf);
        }
    }

    Process::current().fd_table.lock().write(fd, &kbuf)
}

#[repr(C)]
#[derive(Copy, Clone, Default, Debug)]
pub struct IoVec {
    pub iov_base: usize,
    pub iov_len: usize,
}

/// System call entry: `writev(2)` — SYS_writev = 20
pub fn syscall_writev(
    arg0: usize,
    arg1: usize,
    arg2: usize,
    _: usize,
    _: usize,
    _: usize,
    vm: &VmaManager,
    _: &mut ostd::arch::cpu::context::UserContext,
) -> SyscallResult {
    let fd = arg0 as i32;
    let iov_ptr = arg1;
    let iovcnt = arg2;

    if iovcnt == 0 || iov_ptr == 0 {
        return SyscallResult::from_err(Error::InvalidArgs);
    }

    let mut total_bytes_written = 0usize;
    let iovec_size = core::mem::size_of::<IoVec>();

    for i in 0..iovcnt {
        let vec_addr = match iov_ptr.checked_add(i * iovec_size) {
            Some(addr) => addr,
            None => return SyscallResult::from_err(Error::InvalidArgs),
        };

        let mut iov_bytes = [0u8; 16];
        if vm.copy_from_user(vec_addr, &mut iov_bytes).is_err() {
            if total_bytes_written > 0 {
                return SyscallResult::Return(total_bytes_written);
            } else {
                return SyscallResult::from_err(Error::AccessDenied);
            }
        }

        let iov_base = usize::from_le_bytes(iov_bytes[0..8].try_into().unwrap());
        let iov_len = usize::from_le_bytes(iov_bytes[8..16].try_into().unwrap());

        if iov_len == 0 {
            continue;
        }

        match do_write(fd, iov_base, iov_len, vm) {
            Ok(bytes) => {
                total_bytes_written += bytes;
                if bytes < iov_len {
                    break;
                }
            }
            Err(err) => {
                if total_bytes_written > 0 {
                    return SyscallResult::Return(total_bytes_written);
                } else {
                    return SyscallResult::from_err(err);
                }
            }
        }
    }

    SyscallResult::Return(total_bytes_written)
}
