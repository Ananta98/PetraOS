use super::dentry::Dentry;
use super::file::File;
use super::mount::MOUNT_TABLE;
use super::types::{InodeType, VfsError};
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;

/// Maximum symlink traversal depth to prevent infinite circular loops.
pub const MAX_SYMLINK_DEPTH: usize = 8;

// ===== Path Utilities =====

/// Canonicalize/normalize a path relative to `base` (resolves `.` and `..`).
pub fn normalize_path(base: &str, path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();

    if !path.starts_with('/') {
        for segment in base.split('/').filter(|s| !s.is_empty()) {
            parts.push(segment);
        }
    }

    for segment in path.split('/').filter(|s| !s.is_empty()) {
        if segment == "." {
            continue;
        } else if segment == ".." {
            parts.pop();
        } else {
            parts.push(segment);
        }
    }

    if parts.is_empty() {
        String::from("/")
    } else {
        let mut result = String::new();
        for segment in parts {
            result.push('/');
            result.push_str(segment);
        }
        result
    }
}

// ===== Shared Parent-Resolution Helper =====

/// Split a path into (resolved parent dentry, leaf name).
///
/// Both `create_file`, `mkdir`, `unlink`, `rmdir`, `symlink` share this pattern.
/// The parent must already exist; the leaf name must be non-empty.
fn resolve_parent_and_name(path: &str) -> Result<(Arc<Dentry>, &str), VfsError> {
    let clean_path = path.trim_end_matches('/');
    if clean_path.is_empty() {
        return Err(VfsError::InvalidInput);
    }

    if let Some(last_slash) = clean_path.rfind('/') {
        let parent_path = &clean_path[..last_slash];
        let leaf_name = &clean_path[last_slash + 1..];
        if leaf_name.is_empty() {
            return Err(VfsError::InvalidInput);
        }

        let parent_dentry = if parent_path.is_empty() {
            resolve_path("/")?
        } else {
            resolve_path(parent_path)?
        };

        Ok((parent_dentry, leaf_name))
    } else {
        // Relative path without slashes (e.g. "foo") -> parent is current directory
        let parent_dentry = resolve_path(".")?;
        Ok((parent_dentry, clean_path))
    }
}

// ===== Path Resolution =====

/// Resolve an absolute or relative path to a dentry, traversing mount points and symlinks.
pub fn resolve_path(path: &str) -> Result<Arc<Dentry>, VfsError> {
    if !path.starts_with('/') {
        let cwd = crate::proc::current_process()
            .map(|p| p.lock().cwd.clone())
            .unwrap_or_else(|| String::from("/"));
        let norm_path = normalize_path(&cwd, path);
        resolve_path_symlink(&norm_path, 0)
    } else {
        let norm_path = normalize_path("/", path);
        resolve_path_symlink(&norm_path, 0)
    }
}

fn resolve_path_symlink(path: &str, depth: usize) -> Result<Arc<Dentry>, VfsError> {
    if depth >= MAX_SYMLINK_DEPTH {
        return Err(VfsError::TooManySymlinks);
    }

    let mt = MOUNT_TABLE.read();
    let (mount, remainder) = mt.lookup(path).ok_or(VfsError::NotFound)?;

    let mut current = mount.root_dentry.clone();

    if remainder.is_empty() {
        return Ok(current);
    }

    let parts: Vec<&str> = remainder.split('/').filter(|s| !s.is_empty()).collect();

    let multi_mount = mt.mount_count() > 1;

    for (idx, part) in parts.iter().enumerate() {
        // 1. Check local children dentry cache first
        let dentry = if let Some(cached_child) = current.children.lock().get(*part).cloned() {
            cached_child
        } else {
            if current.inode.inode_type != InodeType::Directory {
                log::trace!(
                    "[resolve_path] in '{}', component '{}' failed: node is {:?} (not a directory)",
                    current.full_path(),
                    part,
                    current.inode.inode_type
                );
                return Err(VfsError::NotDirectory);
            }
            let child_inode = match current.inode.ops.lookup(part) {
                Ok(inode) => inode,
                Err(err) => {
                    log::trace!(
                        "[resolve_path] in '{}', lookup('{}') failed: {:?}",
                        current.full_path(),
                        part,
                        err
                    );
                    return Err(err);
                }
            };
            Dentry::add_child(&current, (*part).into(), child_inode)
        };

        // 2. Handle symbolic link resolution
        if dentry.inode.inode_type == InodeType::Symlink {
            let target = dentry.inode.ops.readlink()?;
            drop(mt);
            let symlink_path = dentry.full_path();
            let mut target_full = if target.starts_with('/') {
                target
            } else {
                let parent_end = symlink_path.rfind('/').unwrap_or(0);
                let parent_dir = if parent_end == 0 {
                    "/"
                } else {
                    &symlink_path[..parent_end]
                };
                if parent_dir == "/" {
                    alloc::format!("/{}", target)
                } else {
                    alloc::format!("{}/{}", parent_dir, target)
                }
            };
            for rem in &parts[idx + 1..] {
                target_full.push('/');
                target_full.push_str(rem);
            }
            let norm = normalize_path("/", &target_full);
            return resolve_path_symlink(&norm, depth + 1);
        }

        // 3. Mount boundary traversal — only needed when more than one
        //    filesystem is mounted.  full_path() is O(depth) and allocates;
        //    avoiding it in the single-mount case eliminates the O(N·D)
        //    overhead that caused initramfs extraction to hang.
        if multi_mount {
            let child_path = dentry.full_path();
            if let Some((child_mount, _)) = mt.lookup(&child_path) {
                if child_mount.mount_point == child_path
                    && child_mount.mount_point != mount.mount_point
                {
                    current = child_mount.root_dentry.clone();
                    continue;
                }
            }
        }

        current = dentry;
    }

    Ok(current)
}

// ===== Filesystem Mutation Operations =====

/// Create a new regular file at the given absolute path.
///
/// The parent directory must already exist.
pub fn create_file(path: &str) -> Result<Arc<Dentry>, VfsError> {
    let (parent_dentry, file_name) = resolve_parent_and_name(path)?;

    if parent_dentry.inode.inode_type != InodeType::Directory {
        return Err(VfsError::NotDirectory);
    }

    let child_inode = parent_dentry.inode.ops.create(file_name)?;
    Ok(Dentry::add_child(
        &parent_dentry,
        file_name.into(),
        child_inode,
    ))
}

/// Create a new directory at the given absolute path.
pub fn mkdir(path: &str) -> Result<Arc<Dentry>, VfsError> {
    let (parent_dentry, dir_name) = resolve_parent_and_name(path)?;

    if parent_dentry.inode.inode_type != InodeType::Directory {
        return Err(VfsError::NotDirectory);
    }

    let child_inode = parent_dentry.inode.ops.mkdir(dir_name)?;
    Ok(Dentry::add_child(
        &parent_dentry,
        dir_name.into(),
        child_inode,
    ))
}

/// Unlink (delete) a file entry at the given absolute path.
pub fn unlink(path: &str) -> Result<(), VfsError> {
    let (parent_dentry, file_name) = resolve_parent_and_name(path)?;
    parent_dentry.inode.ops.unlink(file_name)?;
    Dentry::remove_child(&parent_dentry, file_name);
    Ok(())
}

/// Remove an empty directory entry at the given absolute path.
pub fn rmdir(path: &str) -> Result<(), VfsError> {
    let (parent_dentry, dir_name) = resolve_parent_and_name(path)?;
    parent_dentry.inode.ops.rmdir(dir_name)?;
    Dentry::remove_child(&parent_dentry, dir_name);
    Ok(())
}

/// Create a symbolic link at `path` pointing to `target`.
pub fn symlink(path: &str, target: &str) -> Result<Arc<Dentry>, VfsError> {
    let (parent_dentry, link_name) = resolve_parent_and_name(path)?;
    let child_inode = parent_dentry.inode.ops.symlink(link_name, target)?;
    Ok(Dentry::add_child(
        &parent_dentry,
        link_name.into(),
        child_inode,
    ))
}

/// Read the target of a symbolic link at `path`.
///
/// Uses `resolve_path_nofollow` so the final path component is not followed,
/// returning the symlink dentry itself rather than its destination.
pub fn readlink(path: &str) -> Result<String, VfsError> {
    let dentry = resolve_path_nofollow(path)?;
    if dentry.inode.inode_type != InodeType::Symlink {
        return Err(VfsError::InvalidInput);
    }
    dentry.inode.ops.readlink()
}

/// Rename an existing path to a new path.
pub fn rename(old_path: &str, new_path: &str) -> Result<(), VfsError> {
    let (old_parent_dentry, old_name) = resolve_parent_and_name(old_path)?;
    let (new_parent_dentry, new_name) = resolve_parent_and_name(new_path)?;

    old_parent_dentry
        .inode
        .ops
        .rename(old_name, &new_parent_dentry.inode, new_name)?;

    // Evict old entry from old parent dentry cache
    Dentry::remove_child(&old_parent_dentry, old_name);

    // Evict overwritten new entry from new parent dentry cache (if any)
    Dentry::remove_child(&new_parent_dentry, new_name);

    // Insert new dentry into new parent cache
    if let Ok(new_inode) = new_parent_dentry.inode.ops.lookup(new_name) {
        Dentry::add_child(&new_parent_dentry, new_name.into(), new_inode);
    }

    Ok(())
}

/// Create a hard link from `old_path` to `new_path`.
pub fn link(old_path: &str, new_path: &str) -> Result<Arc<Dentry>, VfsError> {
    let target_dentry = resolve_path(old_path)?;
    if target_dentry.inode.inode_type == InodeType::Directory {
        return Err(VfsError::PermissionDenied);
    }
    let (new_parent, new_name) = resolve_parent_and_name(new_path)?;
    new_parent.inode.ops.link(new_name, &target_dentry.inode)?;
    Ok(Dentry::add_child(
        &new_parent,
        new_name.into(),
        target_dentry.inode.clone(),
    ))
}

/// Change mode permissions of the file at `path`.
pub fn chmod(path: &str, mode: u32) -> Result<(), VfsError> {
    let dentry = resolve_path(path)?;
    dentry.inode.ops.chmod(mode)
}

/// Change ownership (uid, gid) of the file at `path`.
pub fn chown(path: &str, uid: u32, gid: u32) -> Result<(), VfsError> {
    let dentry = resolve_path(path)?;
    dentry.inode.ops.chown(uid, gid)
}

/// Truncate file at `path` to `size` bytes.
pub fn truncate(path: &str, size: usize) -> Result<(), VfsError> {
    let dentry = resolve_path(path)?;
    dentry.inode.ops.truncate(size)
}

/// Update timestamps of the file at `path`.
pub fn utimens(path: &str, atime: u64, mtime: u64) -> Result<(), VfsError> {
    let dentry = resolve_path(path)?;
    dentry.inode.ops.utimens(atime, mtime)
}

// ===== File I/O Shortcuts =====

/// Fetch metadata stat structure for the file/directory at `path`.
pub fn stat(path: &str) -> Result<super::types::Stat, VfsError> {
    let dentry = resolve_path(path)?;
    let mut stat = dentry.inode.ops.stat()?;
    if stat.ino == 0 {
        stat.ino = dentry.inode.ino;
    }
    Ok(stat)
}

/// Fetch metadata stat without following the final symbolic link component.
pub fn lstat(path: &str) -> Result<super::types::Stat, VfsError> {
    let dentry = resolve_path_nofollow(path)?;
    let mut stat = dentry.inode.ops.stat()?;
    if stat.ino == 0 {
        stat.ino = dentry.inode.ino;
    }
    Ok(stat)
}

/// Resolve path without following the final symlink component.
pub fn resolve_path_nofollow(path: &str) -> Result<Arc<Dentry>, VfsError> {
    let norm_path = if !path.starts_with('/') {
        let cwd = crate::proc::current_process()
            .map(|p| p.lock().cwd.clone())
            .unwrap_or_else(|| String::from("/"));
        normalize_path(&cwd, path)
    } else {
        normalize_path("/", path)
    };

    if norm_path == "/" {
        return resolve_path("/");
    }

    let last_slash = norm_path.rfind('/').unwrap_or(0);
    let parent_path = &norm_path[..last_slash];
    let leaf_name = &norm_path[last_slash + 1..];

    if leaf_name.is_empty() {
        return resolve_path(&norm_path);
    }

    let parent_dentry = if parent_path.is_empty() {
        resolve_path("/")?
    } else {
        resolve_path(parent_path)?
    };

    if let Some(cached_child) = parent_dentry.children.lock().get(leaf_name).cloned() {
        return Ok(cached_child);
    }

    if parent_dentry.inode.inode_type != InodeType::Directory {
        return Err(VfsError::NotDirectory);
    }

    let child_inode = parent_dentry.inode.ops.lookup(leaf_name)?;
    Ok(Dentry::add_child(
        &parent_dentry,
        leaf_name.into(),
        child_inode,
    ))
}

/// Read the entire contents of a file at `path` into a byte vector.
pub fn read_file(path: &str) -> Result<alloc::vec::Vec<u8>, VfsError> {
    let dentry = match resolve_path(path) {
        Ok(d) => d,
        Err(err) => {
            log::trace!("[read_file] resolve_path('{}') failed: {:?}", path, err);
            return Err(err);
        }
    };
    let stat = dentry.inode.ops.stat()?;
    let file_ops = dentry.inode.ops.open()?;

    let alloc_size = if stat.size > 0 {
        stat.size as usize
    } else {
        4096
    };
    let mut buf = alloc::vec![0u8; alloc_size];
    let bytes_read = file_ops.read(0, &mut buf)?;
    buf.truncate(bytes_read);
    Ok(buf)
}

/// Open a file at `path` with `flags`, returning an open [`File`] instance.
pub fn open_file(path: &str, flags: u32) -> Result<Arc<File>, VfsError> {
    let dentry = match resolve_path(path) {
        Ok(d) => d,
        Err(VfsError::NotFound) if (flags & super::types::O_CREAT) != 0 => create_file(path)?,
        Err(err) => return Err(err),
    };

    let file_ops = dentry.inode.ops.open()?;
    Ok(Arc::new(File::new(dentry, flags, file_ops)))
}
