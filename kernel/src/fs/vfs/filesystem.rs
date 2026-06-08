use alloc::sync::Arc;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::fs::errno::VfsError;
use super::inode::Inode;

/// Trait implemented by each filesystem type (ramfs, devfs, tmpfs, procfs, etc.).
///
/// A `FileSystem` acts as a factory: calling `mount()` produces a new `SuperBlock`
/// with its own root inode and inode-number space.
pub trait FileSystem: Send + Sync {
    /// Human-readable name (e.g., "ramfs", "devfs", "procfs").
    fn name(&self) -> &'static str;

    /// Create a fresh superblock and root inode for a new mount instance.
    fn mount(&self) -> Result<SuperBlock, VfsError>;
}

/// Per-mount metadata. Each mounted filesystem instance has exactly one `SuperBlock`.
pub struct SuperBlock {
    /// Name of the filesystem type that created this superblock.
    pub fs_name: &'static str,
    /// Root inode of this filesystem instance.
    pub root_inode: Arc<Inode>,
    /// Monotonically increasing inode number allocator for this fs instance.
    pub next_ino: AtomicU64,
    /// If `true`, write operations are rejected with `VfsError::ReadOnlyFs`.
    pub read_only: bool,
}

impl SuperBlock {
    /// Allocate the next unique inode number within this filesystem instance.
    pub fn alloc_ino(&self) -> u64 {
        self.next_ino.fetch_add(1, Ordering::Relaxed)
    }
}
