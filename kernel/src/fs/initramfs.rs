//! In-Memory Initramfs (CPIO) Unpacker and Subsystem
//!
//! Parses CPIO archives loaded into memory via bootloader modules (Limine)
//! and extracts directory hierarchies and files directly into the root VFS.

use crate::fs::vfs::dentry::Dentry;
use crate::fs::vfs::types::{InodeType, VfsError};
use crate::utils::cpio::{CpioArchive, CpioHeader};
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;

type HardlinkMap = BTreeMap<u32, (Option<String>, Vec<String>)>;

/// Helper function to create all parent directories recursively, returning the leaf directory Dentry.
pub fn mkdir_p(path: &str) -> Result<Arc<Dentry>, VfsError> {
    let clean = path.trim_start_matches('/');
    if clean.is_empty() {
        return crate::fs::vfs::path::resolve_path("/");
    }

    // Obtain the VFS root dentry directly from the mount table to avoid the
    // full resolve_path() round-trip (MOUNT_TABLE.read + normalize + loop) on
    // every component.  We then walk forward through the dentry cache and only
    // fall back to ops.mkdir / ops.lookup when a child is not yet cached.
    let root_dentry = {
        let mt = crate::fs::vfs::mount::MOUNT_TABLE.read();
        mt.root()
            .map(|m| m.root_dentry.clone())
            .ok_or(VfsError::NotFound)?
    };

    let mut current_dentry = root_dentry;
    for part in clean.split('/').filter(|s| !s.is_empty()) {
        let cached = { current_dentry.children.lock().get(part).cloned() };
        if let Some(child) = cached {
            if child.inode.inode_type != InodeType::Directory {
                return Err(VfsError::NotDirectory);
            }
            current_dentry = child;
            continue;
        }

        // Slow path: create or discover the directory via inode ops.
        let next_dentry = match current_dentry.inode.ops.mkdir(part) {
            Ok(inode) => Dentry::add_child(&current_dentry, part.into(), inode),
            Err(VfsError::AlreadyExists) => {
                let inode = current_dentry.inode.ops.lookup(part)?;
                if inode.inode_type != InodeType::Directory {
                    return Err(VfsError::NotDirectory);
                }
                // Populate the dentry cache so the next lookup is a fast-path hit.
                Dentry::add_child(&current_dentry, part.into(), inode)
            }
            Err(err) => return Err(err),
        };
        current_dentry = next_dentry;
    }
    Ok(current_dentry)
}

/// Helper function to ensure parent directories exist before creating an entry.
pub fn ensure_parent_dir(path: &str) -> Result<(), VfsError> {
    if let Some(last_slash) = path.rfind('/') {
        let parent = &path[..last_slash];
        if !parent.is_empty() {
            mkdir_p(parent)?;
        }
    }
    Ok(())
}

/// Helper function to create a regular file and write its payload, creating parent dirs if needed,
/// and restoring permissions, ownership, and timestamps from the CPIO archive header.
pub fn create_file_with_parents(
    path: &str,
    data: &[u8],
    mode: u32,
    uid: u32,
    gid: u32,
    mtime: u64,
) -> Result<Arc<Dentry>, VfsError> {
    ensure_parent_dir(path)?;

    let dentry = match crate::fs::vfs::path::create_file(path) {
        Ok(d) => d,
        Err(VfsError::AlreadyExists) => crate::fs::resolve_path(path)?,
        Err(err) => return Err(err),
    };

    let file_ops = dentry.inode.ops.open()?;
    let _ = file_ops.truncate(0);
    file_ops.write(0, data)?;

    // Restore permissions, ownership, and timestamps from archive
    let _ = dentry.inode.ops.chmod(mode);
    let _ = dentry.inode.ops.chown(uid, gid);
    if mtime != 0 {
        let _ = dentry.inode.ops.utimens(mtime, mtime);
    }

    Ok(dentry)
}

/// Helper function to create a symbolic link, creating parent dirs if needed.
pub fn create_symlink_with_parents(path: &str, target: &str) -> Result<Arc<Dentry>, VfsError> {
    ensure_parent_dir(path)?;

    match crate::fs::vfs::path::symlink(path, target) {
        Ok(d) => Ok(d),
        Err(VfsError::AlreadyExists) => crate::fs::resolve_path_nofollow(path),
        Err(err) => Err(err),
    }
}

/// Helper function to create a hard link, creating parent dirs if needed.
pub fn create_hardlink_with_parents(
    target_path: &str,
    link_path: &str,
) -> Result<Arc<Dentry>, VfsError> {
    ensure_parent_dir(link_path)?;

    match crate::fs::vfs::path::link(target_path, link_path) {
        Ok(d) => Ok(d),
        Err(VfsError::AlreadyExists) => crate::fs::resolve_path_nofollow(link_path),
        Err(err) => Err(err),
    }
}

fn apply_attributes(dentry: &Arc<Dentry>, mode: u32, uid: u32, gid: u32, mtime: u64) {
    let _ = dentry.inode.ops.chmod(mode);
    let _ = dentry.inode.ops.chown(uid, gid);
    if mtime != 0 {
        let _ = dentry.inode.ops.utimens(mtime, mtime);
    }
}

fn extract_directory_entry(full_path: &str, mode: u32, uid: u32, gid: u32, mtime: u64) {
    match mkdir_p(full_path) {
        Ok(dentry) => apply_attributes(&dentry, mode, uid, gid, mtime),
        Err(err) => {
            log::warn!("[Initramfs] Failed to mkdir '{}': {:?}", full_path, err);
        }
    }
}

fn extract_symlink_entry(full_path: &str, data: &[u8]) -> usize {
    let Ok(raw_target) = core::str::from_utf8(data) else {
        return 0;
    };
    let target = raw_target.trim_end_matches('\0');
    match create_symlink_with_parents(full_path, target) {
        Ok(_) => 1,
        Err(err) => {
            log::warn!(
                "[Initramfs] Failed to create symlink '{}' -> '{}': {:?}",
                full_path,
                target,
                err
            );
            0
        }
    }
}

/// Handle duplicate hard-link entries in SVR4 archives where payload is empty.
fn handle_deferred_hardlink(
    full_path: &str,
    header: &CpioHeader,
    hardlinks: &mut HardlinkMap,
) -> usize {
    if let Some((Some(primary), _)) = hardlinks.get(&header.ino) {
        match create_hardlink_with_parents(primary, full_path) {
            Ok(dentry) => {
                apply_attributes(
                    &dentry,
                    header.mode,
                    header.uid,
                    header.gid,
                    header.mtime as u64,
                );
                1
            }
            Err(err) => {
                log::warn!(
                    "[Initramfs] Failed to hardlink '{}' to '{}': {:?}",
                    full_path,
                    primary,
                    err
                );
                0
            }
        }
    } else {
        let entry_record = hardlinks
            .entry(header.ino)
            .or_insert_with(|| (None, Vec::new()));
        entry_record.1.push(full_path.into());
        0
    }
}

/// Resolve queued hardlinks once the primary entry containing the data is extracted.
fn resolve_pending_hardlinks(
    primary_path: &str,
    header: &CpioHeader,
    hardlinks: &mut HardlinkMap,
) -> usize {
    let entry_record = hardlinks
        .entry(header.ino)
        .or_insert_with(|| (None, Vec::new()));
    entry_record.0 = Some(primary_path.into());
    let pending_paths = core::mem::take(&mut entry_record.1);

    let mut resolved_count = 0;
    for pending in pending_paths {
        match create_hardlink_with_parents(primary_path, &pending) {
            Ok(dentry) => {
                resolved_count += 1;
                apply_attributes(
                    &dentry,
                    header.mode,
                    header.uid,
                    header.gid,
                    header.mtime as u64,
                );
            }
            Err(err) => {
                log::warn!(
                    "[Initramfs] Failed to link pending '{}' to '{}': {:?}",
                    pending,
                    primary_path,
                    err
                );
            }
        }
    }
    resolved_count
}

fn extract_regular_file_entry(
    full_path: &str,
    payload: &[u8],
    header: &CpioHeader,
    hardlinks: &mut HardlinkMap,
) -> usize {
    if header.nlink > 1 && payload.is_empty() {
        return handle_deferred_hardlink(full_path, header, hardlinks);
    }

    match create_file_with_parents(
        full_path,
        payload,
        header.mode,
        header.uid,
        header.gid,
        header.mtime as u64,
    ) {
        Ok(_) => {
            let mut count = 1;
            if header.nlink > 1 {
                count += resolve_pending_hardlinks(full_path, header, hardlinks);
            }
            count
        }
        Err(err) => {
            log::warn!(
                "[Initramfs] Failed to create file '{}': {:?}",
                full_path,
                err
            );
            0
        }
    }
}

/// Unpack an in-memory CPIO archive slice into the active root VFS.
pub fn extract_cpio_archive(data: &[u8]) -> Result<usize, &'static str> {
    let archive = CpioArchive::new(data);
    let mut extracted_count = 0;

    // Track inodes with nlink > 1: ino -> (Option<String> /* primary with data */, Vec<String> /* pending links */)
    let mut hardlinks: HardlinkMap = BTreeMap::new();

    for entry_res in archive.entries() {
        let entry = match entry_res {
            Ok(e) => e,
            Err(e) => {
                log::warn!(
                    "[Initramfs] CPIO parse error at file {}: {:?}",
                    extracted_count,
                    e
                );
                return Err("Failed to parse CPIO entry header");
            }
        };
        let raw_name = entry
            .name()
            .trim_start_matches("./")
            .trim_start_matches('/');

        if raw_name.is_empty() || raw_name == "." {
            continue;
        }

        let full_path = format!("/{}", raw_name);
        let header = entry.header();

        if entry.is_directory() {
            extract_directory_entry(
                &full_path,
                header.mode,
                header.uid,
                header.gid,
                header.mtime as u64,
            );
        } else if entry.is_regular_file() {
            extracted_count +=
                extract_regular_file_entry(&full_path, entry.data(), header, &mut hardlinks);
        } else if entry.is_symlink() {
            extracted_count += extract_symlink_entry(&full_path, entry.data());
        }
    }

    Ok(extracted_count)
}

/// Initramfs subsystem manager.
pub struct Initramfs;

impl Initramfs {
    /// Initialize Initramfs by reading Limine boot modules and unpacking them into the root VFS.
    pub fn init() -> Result<(), &'static str> {
        log::info!("[Initramfs] Initializing Initramfs Subsystem...");

        let module_response = match crate::limine::MODULE_REQUEST.get_response() {
            Some(resp) => resp,
            None => {
                log::info!("[Initramfs] No Limine module response provided by bootloader.");
                return Ok(());
            }
        };

        let modules = module_response.modules();
        if modules.is_empty() {
            log::info!("[Initramfs] No modules loaded by Limine bootloader.");
            return Ok(());
        }

        for module_file in modules {
            let path_str = module_file.path().to_str().unwrap_or("<unknown>");
            let size = module_file.size() as usize;

            if size == 0 {
                continue;
            }

            log::info!(
                "[Initramfs] Loading boot module '{}' ({} bytes)...",
                path_str,
                size
            );

            // SAFETY: Limine guarantees module memory addresses are valid and mapped.
            let raw_data = unsafe { core::slice::from_raw_parts(module_file.addr(), size) };

            match extract_cpio_archive(raw_data) {
                Ok(count) => {
                    log::info!(
                        "✔ [Initramfs] Successfully extracted {} file(s) from '{}' into root VFS.",
                        count,
                        path_str
                    );
                }
                Err(err) => {
                    log::warn!(
                        "[Initramfs] Module '{}' is not a valid CPIO archive ({})",
                        path_str,
                        err
                    );
                }
            }
        }

        Ok(())
    }
}

crate::late_initcall!(Initramfs::init);
crate::MODULE_LICENSE!("GPL-2.0");
crate::MODULE_AUTHOR!("PetraOS Development Team");
crate::MODULE_DESCRIPTION!("In-Memory CPIO Initramfs Unpacker");
