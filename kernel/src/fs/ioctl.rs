use crate::fs::vfs::types::VfsError;

/// Dispatch ioctl commands on file descriptors.
pub fn do_ioctl(_fd: i32, _cmd: u64, _arg: usize) -> Result<i32, VfsError> {
    Err(VfsError::NotSupported)
}
