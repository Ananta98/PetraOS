use super::dcache::{dcache_evict, dcache_insert, dcache_lookup};
use super::dentry::Dentry;
use super::mount::MOUNT_TABLE;
use super::types::{InodeType, VfsError};
use alloc::sync::Arc;

/// Maximum symlink traversal depth to prevent infinite circular loops.
pub const MAX_SYMLINK_DEPTH: usize = 8;

/// Resolve an absolute path to a dentry, traversing mount boundaries and symlinks.
pub fn resolve_path(path: &str) -> Result<Arc<Dentry>, VfsError> {
    resolve_path_symlink(path, 0)
}

fn resolve_path_symlink(path: &str, depth: usize) -> Result<Arc<Dentry>, VfsError> {
    if depth >= MAX_SYMLINK_DEPTH {
        return Err(VfsError::NotSupported);
    }

    let mt = MOUNT_TABLE.read();
    let (mount, remainder) = mt.lookup(path).ok_or(VfsError::NotFound)?;

    let mut current = mount.root_dentry.clone();

    if remainder.is_empty() {
        return Ok(current);
    }

    let parts = remainder.split('/').filter(|s| !s.is_empty());

    for part in parts {
        // 1. Check global dcache and local children dentry cache first
        let dentry = if let Some(cached) = dcache_lookup(&current, part) {
            cached
        } else if let Some(cached_child) = current.children.lock().get(part).cloned() {
            dcache_insert(&current, part, cached_child.clone());
            cached_child
        } else {
            if current.inode.inode_type != InodeType::Directory {
                return Err(VfsError::NotDirectory);
            }
            let child_inode = current.inode.ops.lookup(part)?;
            let child_dentry = Dentry::add_child(&current, part.into(), child_inode);
            dcache_insert(&current, part, child_dentry.clone());
            child_dentry
        };

        // 2. Handle symbolic link resolution
        if dentry.inode.inode_type == InodeType::Symlink {
            if let Ok(target) = dentry.inode.ops.readlink() {
                drop(mt);
                return resolve_path_symlink(&target, depth + 1);
            }
        }

        // 3. Mount boundary traversal check
        let child_path = build_path(&dentry);
        if let Some((child_mount, _)) = mt.lookup(&child_path) {
            if child_mount.mount_point == child_path
                && child_mount.mount_point != mount.mount_point
            {
                current = child_mount.root_dentry.clone();
                continue;
            }
        }

        current = dentry;
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

/// Create a new directory at the given absolute path.
pub fn mkdir(path: &str) -> Result<Arc<Dentry>, VfsError> {
    let last_slash = path.rfind('/').ok_or(VfsError::InvalidInput)?;
    let parent_path = &path[..last_slash];
    let dir_name = &path[last_slash + 1..];

    if dir_name.is_empty() {
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

    let child_inode = parent_dentry.inode.ops.mkdir(dir_name)?;
    let child_dentry = Dentry::add_child(&parent_dentry, dir_name.into(), child_inode);
    Ok(child_dentry)
}

/// Unlink (delete) a file entry at the given absolute path.
pub fn unlink(path: &str) -> Result<(), VfsError> {
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

    parent_dentry.inode.ops.unlink(file_name)?;
    Dentry::remove_child(&parent_dentry, file_name);
    dcache_evict(&parent_dentry, file_name);
    Ok(())
}

/// Remove an empty directory entry at the given absolute path.
pub fn rmdir(path: &str) -> Result<(), VfsError> {
    let last_slash = path.rfind('/').ok_or(VfsError::InvalidInput)?;
    let parent_path = &path[..last_slash];
    let dir_name = &path[last_slash + 1..];

    if dir_name.is_empty() {
        return Err(VfsError::InvalidInput);
    }

    let parent_dentry = if parent_path.is_empty() {
        resolve_path("/")?
    } else {
        resolve_path(parent_path)?
    };

    parent_dentry.inode.ops.rmdir(dir_name)?;
    Dentry::remove_child(&parent_dentry, dir_name);
    dcache_evict(&parent_dentry, dir_name);
    Ok(())
}

/// Create a symbolic link at `path` pointing to `target`.
pub fn symlink(path: &str, target: &str) -> Result<Arc<Dentry>, VfsError> {
    let last_slash = path.rfind('/').ok_or(VfsError::InvalidInput)?;
    let parent_path = &path[..last_slash];
    let link_name = &path[last_slash + 1..];

    if link_name.is_empty() {
        return Err(VfsError::InvalidInput);
    }

    let parent_dentry = if parent_path.is_empty() {
        resolve_path("/")?
    } else {
        resolve_path(parent_path)?
    };

    let child_inode = parent_dentry.inode.ops.symlink(link_name, target)?;
    let child_dentry = Dentry::add_child(&parent_dentry, link_name.into(), child_inode);
    Ok(child_dentry)
}

/// Read the target of a symbolic link at `path`.
pub fn readlink(path: &str) -> Result<alloc::string::String, VfsError> {
    let dentry = resolve_path(path)?;
    dentry.inode.ops.readlink()
}

/// Fetch metadata stat structure for file/directory at `path`.
pub fn stat(path: &str) -> Result<super::types::Stat, VfsError> {
    let dentry = resolve_path(path)?;
    dentry.inode.ops.stat()
}

/// Rename an existing path to a new path.
pub fn rename(old_path: &str, new_path: &str) -> Result<(), VfsError> {
    let old_slash = old_path.rfind('/').ok_or(VfsError::InvalidInput)?;
    let old_parent_path = &old_path[..old_slash];
    let old_name = &old_path[old_slash + 1..];

    let new_slash = new_path.rfind('/').ok_or(VfsError::InvalidInput)?;
    let new_parent_path = &new_path[..new_slash];
    let new_name = &new_path[new_slash + 1..];

    let old_parent_dentry = if old_parent_path.is_empty() {
        resolve_path("/")?
    } else {
        resolve_path(old_parent_path)?
    };

    let new_parent_dentry = if new_parent_path.is_empty() {
        resolve_path("/")?
    } else {
        resolve_path(new_parent_path)?
    };

    old_parent_dentry
        .inode
        .ops
        .rename(old_name, &new_parent_dentry.inode, new_name)?;

    if let Some(child_dentry) = old_parent_dentry.children.lock().remove(old_name) {
        new_parent_dentry
            .children
            .lock()
            .insert(new_name.into(), child_dentry);
    }

    Ok(())
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
