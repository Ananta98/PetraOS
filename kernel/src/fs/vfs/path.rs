use alloc::sync::Arc;
use super::types::{InodeType, VfsError};
use super::dcache::Dentry;
use super::mount::MOUNT_TABLE;

/// Resolve an absolute path to a dentry, traversing mount boundaries.
///
/// Finds the longest-prefix mount for `path`, then walks each path component
/// through the dentry cache (or via `InodeOps::lookup` on cache miss).
/// If a component's dentry corresponds to a mount point for another filesystem,
/// resolution switches to that mount's root dentry.
pub fn resolve_path(path: &str) -> Result<Arc<Dentry>, VfsError> {
    let mt = MOUNT_TABLE.lock();
    let (mount, remainder) = mt.lookup(path).ok_or(VfsError::NotFound)?;

    let mut current = mount.root_dentry.clone();

    if remainder.is_empty() {
        return Ok(current);
    }

    let parts = remainder.split('/').filter(|s| !s.is_empty());

    for part in parts {
        // Check dentry cache first
        let cached = current.children.lock().get(part).cloned();
        if let Some(child) = cached {
            // Check if this child is itself a mount point
            let child_path = build_path(&child);
            if let Some((child_mount, _)) = mt.lookup(&child_path) {
                if child_mount.mount_point == child_path && child_mount.mount_point != mount.mount_point {
                    current = child_mount.root_dentry.clone();
                    continue;
                }
            }
            current = child;
        } else {
            if current.inode.inode_type != InodeType::Directory {
                return Err(VfsError::NotDirectory);
            }
            let child_inode = current.inode.ops.lookup(part)?;
            let child_dentry = Dentry::add_child(&current, part.into(), child_inode);

            // Check if new child is a mount point
            let child_path = build_path(&child_dentry);
            if let Some((child_mount, _)) = mt.lookup(&child_path) {
                if child_mount.mount_point == child_path && child_mount.mount_point != mount.mount_point {
                    current = child_mount.root_dentry.clone();
                    continue;
                }
            }
            current = child_dentry;
        }
    }

    Ok(current)
}

/// Create a new regular file at the given absolute path.
///
/// The parent directory must already exist.
pub fn create_file(path: &str) -> Result<Arc<Dentry>, VfsError> {
    let last_slash = path.rfind('/').ok_or(VfsError::InvalidInput)?;
    let parent_path = &path[..last_slash];
    let file_name = &path[last_slash + 1..];

    if file_name.is_empty() {
        return Err(VfsError::InvalidInput);
    }

    let parent_dentry = if parent_path.is_empty() {
        resolve_path("/")?
    } else {
        resolve_path(parent_path)?
    };

    if parent_dentry.inode.inode_type != InodeType::Directory {
        return Err(VfsError::NotDirectory);
    }

    let child_inode = parent_dentry.inode.ops.create(file_name)?;
    let child_dentry = Dentry::add_child(&parent_dentry, file_name.into(), child_inode);
    Ok(child_dentry)
}

/// Build an absolute path from a dentry by walking up the parent chain.
fn build_path(dentry: &Dentry) -> alloc::string::String {
    use alloc::vec::Vec;

    let mut components = Vec::new();
    components.push(dentry.name.clone());

    let mut current_parent = dentry.parent.lock().as_ref().and_then(|w| w.upgrade());
    while let Some(parent) = current_parent {
        if parent.name != "/" {
            components.push(parent.name.clone());
        }
        current_parent = parent.parent.lock().as_ref().and_then(|w| w.upgrade());
    }

    components.reverse();
    let mut path = alloc::string::String::from("/");
    for (i, comp) in components.iter().enumerate() {
        if comp == "/" {
            continue;
        }
        if i > 0 && !path.ends_with('/') {
            path.push('/');
        }
        path.push_str(comp);
    }
    if path.is_empty() {
        path.push('/');
    }
    path
}
