pub mod errno;
pub mod flags;
pub mod path;
pub mod init;
pub mod vfs;
pub mod fd;
pub mod ramfs;
pub mod devfs;
pub mod tmpfs;
pub mod procfs;

use alloc::sync::Arc;
use crate::arch::CpuArch;

// ── Public re-exports (preserves existing API surface) ───────────────────────

pub use errno::VfsError;
pub use flags::{O_RDONLY, O_WRONLY, O_RDWR, O_CREAT};
pub use vfs::file::File;
pub use vfs::inode::{Inode, InodeOps, InodeType};
pub use vfs::dentry::Dentry;
pub use vfs::mount::Mount;
pub use vfs::filesystem::{FileSystem, SuperBlock};
pub use vfs::file::FileOps;

// ── Public API (unchanged signatures from the original) ──────────────────────

/// Initialize the VFS, mount all built-in filesystems, and set up standard FDs.
pub fn init() {
    init::init();
}

/// Determine the current process ID from the running CPU's thread.
fn current_process_id() -> Option<crate::proc::ProcessId> {
    let cpu_id = crate::arch::ArchImpl::cpu_id();
    let tid = crate::proc::current_thread_id(cpu_id)?;
    let tm = crate::proc::THREAD_MANAGER.lock();
    let thread = tm.threads.get(&tid)?;
    Some(thread.process_id)
}

/// Open a file at `path` with the given `flags`.
///
/// If `O_CREAT` is set and the file does not exist, a new regular file is created.
/// Returns a file descriptor on success.
pub fn open(path: &str, flags: u32) -> Result<i32, VfsError> {
    let dentry = match path::resolve_path(path) {
        Ok(d) => d,
        Err(VfsError::NotFound) if (flags & O_CREAT) != 0 => {
            path::create_file(path)?
        }
        Err(e) => return Err(e),
    };

    // O_CREAT should only create regular files
    if (flags & O_CREAT) != 0 && dentry.inode.inode_type != InodeType::File
        && dentry.inode.inode_type != InodeType::CharDevice {
        return Err(VfsError::NotSupported);
    }

    let file_ops = dentry.inode.ops.open()?;
    let file = Arc::new(File::new(dentry, flags, file_ops));

    let pid = current_process_id().ok_or(VfsError::PermissionDenied)?;
    let pm = crate::proc::PROCESS_MANAGER.lock();
    let proc = pm.get_process(pid).ok_or(VfsError::PermissionDenied)?;

    let fd = proc.fd_table.alloc(file);
    Ok(fd)
}

/// Read from an open file descriptor into `buf`.
///
/// Respects open flags: `O_WRONLY` files cannot be read.
pub fn read(fd: i32, buf: &mut [u8]) -> Result<usize, VfsError> {
    let pid = current_process_id().ok_or(VfsError::PermissionDenied)?;
    let pm = crate::proc::PROCESS_MANAGER.lock();
    let proc = pm.get_process(pid).ok_or(VfsError::PermissionDenied)?;

    let file = proc.fd_table.get(fd)?;
    file.read(buf)
}

/// Write to an open file descriptor from `buf`.
///
/// Respects open flags: `O_RDONLY` files cannot be written.
pub fn write(fd: i32, buf: &[u8]) -> Result<usize, VfsError> {
    let pid = current_process_id().ok_or(VfsError::PermissionDenied)?;
    let pm = crate::proc::PROCESS_MANAGER.lock();
    let proc = pm.get_process(pid).ok_or(VfsError::PermissionDenied)?;

    let file = proc.fd_table.get(fd)?;
    file.write(buf)
}

/// Close an open file descriptor.
pub fn close(fd: i32) -> Result<(), VfsError> {
    let pid = current_process_id().ok_or(VfsError::PermissionDenied)?;
    let pm = crate::proc::PROCESS_MANAGER.lock();
    let proc = pm.get_process(pid).ok_or(VfsError::PermissionDenied)?;

    proc.fd_table.close(fd)
}
