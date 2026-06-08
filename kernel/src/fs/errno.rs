/// Unified error type for all VFS and filesystem operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VfsError {
    /// Entry not found during lookup.
    NotFound,
    /// Expected a directory but found a non-directory inode.
    NotDirectory,
    /// Expected a regular file but found a non-file inode.
    NotFile,
    /// Entry already exists (e.g., mkdir/create duplicate).
    AlreadyExists,
    /// Invalid argument or malformed input.
    InvalidInput,
    /// Operation not permitted (e.g., wrong open flags).
    PermissionDenied,
    /// Filesystem is mounted read-only.
    ReadOnlyFs,
    /// Operation not supported by this filesystem or inode type.
    NotSupported,
    /// Bad file descriptor (not open or already closed).
    BadFd,
}
