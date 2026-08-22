use super::dcache::{dcache_evict, dcache_insert, dcache_lookup};
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

/// Build an absolute path string by walking up the dentry parent chain.
pub fn build_path(dentry: &Dentry) -> String {
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
    let mut path = String::from("/");
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

// ===== Shared Parent-Resolution Helper =====

/// Split a path into (resolved parent dentry, leaf name).
///
/// Both `create_file`, `mkdir`, `unlink`, `rmdir`, `symlink` share this pattern.
/// The parent must already exist; the leaf name must be non-empty.
fn resolve_parent_and_name(path: &str) -> Result<(Arc<Dentry>, &str), VfsError> {
    let last_slash = path.rfind('/').ok_or(VfsError::InvalidInput)?;
    let parent_path = &path[..last_slash];
    let leaf_name = &path[last_slash + 1..];

    if leaf_name.is_empty() {
        return Err(VfsError::InvalidInput);
    }

    let parent_dentry = if parent_path.is_empty() {
        resolve_path("/")?
    } else {
        resolve_path(parent_path)?
    };

    Ok((parent_dentry, leaf_name))
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

    for (idx, part) in parts.iter().enumerate() {
        // 1. Check global dcache and local children dentry cache first
        let dentry = if let Some(cached) = dcache_lookup(&current, part) {
            cached
        } else if let Some(cached_child) = current.children.lock().get(*part).cloned() {
            dcache_insert(&current, part, cached_child.clone());
            cached_child
        } else {
            if current.inode.inode_type != InodeType::Directory {
                return Err(VfsError::NotDirectory);
            }
            let child_inode = current.inode.ops.lookup(part)?;
            let child_dentry = Dentry::add_child(&current, (*part).into(), child_inode);
            dcache_insert(&current, part, child_dentry.clone());
            child_dentry
        };

        // 2. Handle symbolic link resolution
        if dentry.inode.inode_type == InodeType::Symlink {
            if let Ok(target) = dentry.inode.ops.readlink() {
                drop(mt);
                let current_dir = build_path(&current);
                let mut target_full = if target.starts_with('/') {
                    target
                } else {
                    let base = current_dir.trim_end_matches('/');
                    alloc::format!("{}/{}", base, target)
                };
                for rem in &parts[idx + 1..] {
                    target_full.push('/');
                    target_full.push_str(rem);
                }
                let norm = normalize_path("/", &target_full);
                return resolve_path_symlink(&norm, depth + 1);
            }
        }

        // 3. Mount boundary traversal
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
    let child_dentry = Dentry::add_child(&parent_dentry, file_name.into(), child_inode);
    Ok(child_dentry)
}

/// Create a new directory at the given absolute path.
pub fn mkdir(path: &str) -> Result<Arc<Dentry>, VfsError> {
    let (parent_dentry, dir_name) = resolve_parent_and_name(path)?;

    if parent_dentry.inode.inode_type != InodeType::Directory {
        return Err(VfsError::NotDirectory);
    }

    let child_inode = parent_dentry.inode.ops.mkdir(dir_name)?;
    let child_dentry = Dentry::add_child(&parent_dentry, dir_name.into(), child_inode);
    Ok(child_dentry)
}

/// Unlink (delete) a file entry at the given absolute path.
pub fn unlink(path: &str) -> Result<(), VfsError> {
    let (parent_dentry, file_name) = resolve_parent_and_name(path)?;
    parent_dentry.inode.ops.unlink(file_name)?;
    Dentry::remove_child(&parent_dentry, file_name);
    dcache_evict(&parent_dentry, file_name);
    Ok(())
}

/// Remove an empty directory entry at the given absolute path.
pub fn rmdir(path: &str) -> Result<(), VfsError> {
    let (parent_dentry, dir_name) = resolve_parent_and_name(path)?;
    parent_dentry.inode.ops.rmdir(dir_name)?;
    Dentry::remove_child(&parent_dentry, dir_name);
    dcache_evict(&parent_dentry, dir_name);
    Ok(())
}

/// Create a symbolic link at `path` pointing to `target`.
pub fn symlink(path: &str, target: &str) -> Result<Arc<Dentry>, VfsError> {
    let (parent_dentry, link_name) = resolve_parent_and_name(path)?;
    let child_inode = parent_dentry.inode.ops.symlink(link_name, target)?;
    let child_dentry = Dentry::add_child(&parent_dentry, link_name.into(), child_inode);
    Ok(child_dentry)
}

/// Read the target of a symbolic link at `path`.
pub fn readlink(path: &str) -> Result<String, VfsError> {
    let dentry = resolve_path(path)?;
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

    if let Some(child_dentry) = old_parent_dentry.children.lock().remove(old_name) {
        new_parent_dentry
            .children
            .lock()
            .insert(new_name.into(), child_dentry);
    }

    Ok(())
}

// ===== File I/O Shortcuts =====

/// Fetch metadata stat structure for the file/directory at `path`.
pub fn stat(path: &str) -> Result<super::types::Stat, VfsError> {
    let dentry = resolve_path(path)?;
    dentry.inode.ops.stat()
}

/// Read the entire contents of a file at `path` into a byte vector.
pub fn read_file(path: &str) -> Result<alloc::vec::Vec<u8>, VfsError> {
    let dentry = resolve_path(path)?;
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
