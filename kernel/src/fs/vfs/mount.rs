use alloc::sync::Arc;
use alloc::string::String;
use alloc::collections::BTreeMap;
use crate::sync::spinlock::Spinlock;
use crate::fs::errno::VfsError;
use super::dentry::Dentry;
use super::filesystem::{FileSystem, SuperBlock};

/// A single mount point binding a filesystem instance to a path in the VFS tree.
pub struct Mount {
    /// The path this filesystem is mounted at (e.g., "/", "/dev", "/tmp").
    pub mount_point: String,
    /// The superblock for this mounted filesystem instance.
    pub superblock: Arc<SuperBlock>,
    /// The root dentry of this mounted filesystem.
    pub root_dentry: Arc<Dentry>,
}

/// Global table tracking all mounted filesystems.
///
/// Mounts are stored keyed by their mount-point path. The table supports
/// longest-prefix matching for path resolution.
pub struct MountTable {
    mounts: BTreeMap<String, Arc<Mount>>,
}

impl MountTable {
    /// Create an empty mount table.
    pub const fn new() -> Self {
        Self {
            mounts: BTreeMap::new(),
        }
    }

    /// Mount a filesystem at the given path.
    ///
    /// Calls `fs.mount()` to produce a fresh superblock and root inode,
    /// wraps the root inode in a dentry, and registers the mount.
    pub fn mount(&mut self, path: &str, fs: &dyn FileSystem) -> Result<Arc<Mount>, VfsError> {
        let sb = fs.mount()?;
        let root_dentry = Arc::new(Dentry::new(path.into(), sb.root_inode.clone()));
        let mount = Arc::new(Mount {
            mount_point: path.into(),
            superblock: Arc::new(sb),
            root_dentry,
        });
        self.mounts.insert(path.into(), mount.clone());
        Ok(mount)
    }

    /// Find the mount whose mount point is the longest prefix of `path`.
    ///
    /// Returns the mount and the remaining path suffix after the mount point.
    /// For example, looking up "/dev/console" with a mount at "/dev" returns
    /// `(mount_at_dev, "console")`.
    pub fn lookup<'a>(&self, path: &'a str) -> Option<(Arc<Mount>, &'a str)> {
        let mut best: Option<(Arc<Mount>, &'a str)> = None;
        for (mount_path, mount) in &self.mounts {
            if path == mount_path.as_str() {
                // Exact match — no remaining path
                return Some((mount.clone(), ""));
            }
            if mount_path == "/" {
                // Root mount matches everything; remainder is the whole path minus "/"
                let remainder = path.trim_start_matches('/');
                match &best {
                    Some((_, _)) => {} // a longer match already exists, skip
                    None => best = Some((mount.clone(), remainder)),
                }
            } else if path.starts_with(mount_path.as_str())
                && path.as_bytes().get(mount_path.len()) == Some(&b'/')
            {
                let remainder = &path[mount_path.len() + 1..];
                match &best {
                    Some((existing, _)) if existing.mount_point.len() >= mount_path.len() => {}
                    _ => best = Some((mount.clone(), remainder)),
                }
            }
        }
        best
    }

    /// Get the root mount (mounted at "/").
    pub fn root(&self) -> Option<Arc<Mount>> {
        self.mounts.get("/").cloned()
    }
}

/// The global mount table, shared across all filesystem operations.
pub static MOUNT_TABLE: Spinlock<MountTable> = Spinlock::new(MountTable::new());
