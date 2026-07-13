pub(crate) mod close;
pub(crate) mod dup;
pub(crate) mod dup2;
pub(crate) mod lseek;
pub(crate) mod mount;
pub(crate) mod open;
pub(crate) mod pipe;
pub(crate) mod read;
pub(crate) mod write;

pub(crate) use close::syscall_close;
pub(crate) use dup::syscall_dup;
pub(crate) use dup2::syscall_dup2;
pub(crate) use lseek::syscall_lseek;
pub(crate) use mount::syscall_mount;
pub(crate) use open::syscall_open;
pub(crate) use pipe::syscall_pipe2;
pub(crate) use read::syscall_read;
pub(crate) use write::syscall_write;

use crate::syscall::SyscallResult;
use crate::vm::vma::VmaManager;
use alloc::string::String;
use alloc::vec::Vec;
use ostd::Error;

/// Helper to read a null-terminated string from user space.
pub(crate) fn read_user_string(vm: &VmaManager, user_ptr: usize) -> Result<String, Error> {
    let mut buf = Vec::new();
    let mut offset = 0;
    loop {
        let mut char_buf = [0u8; 1];
        vm.copy_from_user(user_ptr + offset, &mut char_buf)?;
        if char_buf[0] == 0 {
            break;
        }
        buf.push(char_buf[0]);
        offset += 1;
        if offset > 4096 {
            return Err(Error::InvalidArgs);
        }
    }
    String::from_utf8(buf).map_err(|_| Error::InvalidArgs)
}

/// Helper to read a byte slice from user space.
pub(crate) fn read_user_slice(
    vm: &VmaManager,
    user_ptr: usize,
    len: usize,
) -> Result<Vec<u8>, Error> {
    if len > 1024 * 1024 {
        return Err(Error::InvalidArgs);
    }
    let mut buf = alloc::vec![0u8; len];
    vm.copy_from_user(user_ptr, &mut buf)?;
    Ok(buf)
}
