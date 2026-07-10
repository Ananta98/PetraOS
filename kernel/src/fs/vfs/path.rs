use crate::fs::vfs::dcache::DENTRY_CACHE;
use crate::fs::vfs::mount::{CWD_DENTRY, ROOT_DENTRY};
use crate::fs::vfs::types::{Dentry, FileType, Result};
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use ostd::Error;

const MAX_SYMLINKS: usize = 8;

/// Resolve a pathname string starting from the root directory or CWD.
pub fn resolve_path(path: &str) -> Result<Arc<Dentry>> {
    resolve_path_ext(path, 0)
}

fn resolve_path_ext(path: &str, symlink_depth: usize) -> Result<Arc<Dentry>> {
    if symlink_depth > MAX_SYMLINKS {
        return Err(Error::InvalidArgs);
    }

    let mut current = if path.starts_with('/') {
        ROOT_DENTRY
            .lock()
            .as_ref()
            .cloned()
            .ok_or(Error::InvalidArgs)?
    } else {
        CWD_DENTRY
            .lock()
            .as_ref()
            .cloned()
            .ok_or(Error::InvalidArgs)?
    };

    let components: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();

    for component in components {
        // Cross down into mounted filesystem if the current dentry is a mount point.
        // We evaluate and drop locks inside a separate scope in each iteration.
        loop {
            let next_current = {
                let sb_opt = current.mounted_sb.lock().clone();
                if let Some(sb) = sb_opt {
                    sb.root_dentry.lock().clone()
                } else {
                    None
                }
            };
            if let Some(root_dentry) = next_current {
                current = root_dentry;
            } else {
                break;
            }
        }

        match component {
            "." => {}
            ".." => {
                // Cross up across mount points if we are at a mounted root.
                let mp_opt = current.mount_point.lock().clone();
                if let Some(mp) = mp_opt {
                    if let Some(mp_dentry) = mp.upgrade() {
                        current = mp_dentry;
                    }
                }
                if let Some(parent_weak) = &current.parent {
                    if let Some(parent_dentry) = parent_weak.upgrade() {
                        current = parent_dentry;
                    }
                }
            }
            name => {
                let metadata = current.inode.metadata()?;
                if metadata.file_type != FileType::Directory {
                    return Err(Error::InvalidArgs);
                }

                // Check cache first
                let next =
                    if let Some(cached_dentry) = DENTRY_CACHE.lookup(metadata.inode_num, name) {
                        cached_dentry
                    } else {
                        // Lock children, lookup inode, populate children cache, and drop locks.
                        let child = {
                            let mut children = current.children.lock();
                            if let Some(child) = children.get(name) {
                                child.clone()
                            } else {
                                let child_inode = current.inode.lookup(name)?;
                                let child_dentry =
                                    Dentry::new(name, child_inode, Some(Arc::downgrade(&current)));
                                children.insert(String::from(name), child_dentry.clone());
                                child_dentry
                            }
                        };
                        // Insert to global dcache AFTER releasing the children spinlock to prevent lock ordering deadlocks
                        DENTRY_CACHE.insert(metadata.inode_num, name, child.clone());
                        child
                    };

                let child_metadata = next.inode.metadata()?;
                if child_metadata.file_type == FileType::Symlink {
                    let target = next.inode.read_link()?;
                    if target.starts_with('/') {
                        current = resolve_path_ext(&target, symlink_depth + 1)?;
                    } else {
                        current = resolve_path_from(current, &target, symlink_depth + 1)?;
                    }
                } else {
                    current = next;
                }
            }
        }
    }

    // Cross down one last time in case the resolved dentry is a mount point.
    loop {
        let next_current = {
            let sb_opt = current.mounted_sb.lock().clone();
            if let Some(sb) = sb_opt {
                sb.root_dentry.lock().clone()
            } else {
                None
            }
        };
        if let Some(root_dentry) = next_current {
            current = root_dentry;
        } else {
            break;
        }
    }

    Ok(current)
}

fn resolve_path_from(start: Arc<Dentry>, path: &str, symlink_depth: usize) -> Result<Arc<Dentry>> {
    if symlink_depth > MAX_SYMLINKS {
        return Err(Error::InvalidArgs);
    }

    let mut current = if path.starts_with('/') {
        ROOT_DENTRY
            .lock()
            .as_ref()
            .cloned()
            .ok_or(Error::InvalidArgs)?
    } else {
        start
    };

    let components: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();

    for component in components {
        loop {
            let next_current = {
                let sb_opt = current.mounted_sb.lock().clone();
                if let Some(sb) = sb_opt {
                    sb.root_dentry.lock().clone()
                } else {
                    None
                }
            };
            if let Some(root_dentry) = next_current {
                current = root_dentry;
            } else {
                break;
            }
        }

        match component {
            "." => {}
            ".." => {
                let mp_opt = current.mount_point.lock().clone();
                if let Some(mp) = mp_opt {
                    if let Some(mp_dentry) = mp.upgrade() {
                        current = mp_dentry;
                    }
                }
                if let Some(parent_weak) = &current.parent {
                    if let Some(parent_dentry) = parent_weak.upgrade() {
                        current = parent_dentry;
                    }
                }
            }
            name => {
                let metadata = current.inode.metadata()?;
                if metadata.file_type != FileType::Directory {
                    return Err(Error::InvalidArgs);
                }

                // Check cache first
                let next =
                    if let Some(cached_dentry) = DENTRY_CACHE.lookup(metadata.inode_num, name) {
                        cached_dentry
                    } else {
                        let child = {
                            let mut children = current.children.lock();
                            if let Some(child) = children.get(name) {
                                child.clone()
                            } else {
                                let child_inode = current.inode.lookup(name)?;
                                let child_dentry =
                                    Dentry::new(name, child_inode, Some(Arc::downgrade(&current)));
                                children.insert(String::from(name), child_dentry.clone());
                                child_dentry
                            }
                        };
                        DENTRY_CACHE.insert(metadata.inode_num, name, child.clone());
                        child
                    };

                let child_metadata = next.inode.metadata()?;
                if child_metadata.file_type == FileType::Symlink {
                    let target = next.inode.read_link()?;
                    if target.starts_with('/') {
                        current = resolve_path_ext(&target, symlink_depth + 1)?;
                    } else {
                        current = resolve_path_from(current, &target, symlink_depth + 1)?;
                    }
                } else {
                    current = next;
                }
            }
        }
    }

    loop {
        let next_current = {
            let sb_opt = current.mounted_sb.lock().clone();
            if let Some(sb) = sb_opt {
                sb.root_dentry.lock().clone()
            } else {
                None
            }
        };
        if let Some(root_dentry) = next_current {
            current = root_dentry;
        } else {
            break;
        }
    }

    Ok(current)
}
