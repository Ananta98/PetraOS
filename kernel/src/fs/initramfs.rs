//! In-Memory Initramfs (CPIO) Unpacker and Subsystem
//!
//! Parses CPIO archives loaded into memory via bootloader modules (Limine)
//! and extracts directory hierarchies and files directly into the root VFS.

use crate::fs::vfs::types::VfsError;
use crate::utils::cpio::CpioArchive;
use alloc::format;
use alloc::string::String;

/// Helper function to create all parent directories recursively.
pub fn mkdir_p(path: &str) -> Result<(), VfsError> {
    let clean = path.trim_start_matches('/');
    if clean.is_empty() {
        return Ok(());
    }

    let mut current_path = String::from("/");
    for part in clean.split('/').filter(|s| !s.is_empty()) {
        if current_path != "/" {
            current_path.push('/');
        }
        current_path.push_str(part);

        match crate::fs::vfs::path::mkdir(&current_path) {
            Ok(_) | Err(VfsError::AlreadyExists) => {}
            Err(err) => return Err(err),
        }
    }
    Ok(())
}

/// Helper function to create a regular file and write its payload, creating parent dirs if needed.
pub fn create_file_with_parents(path: &str, data: &[u8]) -> Result<(), VfsError> {
    if let Some(last_slash) = path.rfind('/') {
        let parent = &path[..last_slash];
        if !parent.is_empty() {
            mkdir_p(parent)?;
        }
    }

    let dentry = match crate::fs::vfs::path::create_file(path) {
        Ok(d) => d,
        Err(VfsError::AlreadyExists) => crate::fs::resolve_path(path)?,
        Err(err) => return Err(err),
    };

    let file_ops = dentry.inode.ops.open()?;
    let _ = file_ops.truncate(0);
    file_ops.write(0, data)?;
    Ok(())
}

/// Helper function to create a symbolic link, creating parent dirs if needed.
pub fn create_symlink_with_parents(path: &str, target: &str) -> Result<(), VfsError> {
    if let Some(last_slash) = path.rfind('/') {
        let parent = &path[..last_slash];
        if !parent.is_empty() {
            mkdir_p(parent)?;
        }
    }

    match crate::fs::vfs::path::symlink(path, target) {
        Ok(_) | Err(VfsError::AlreadyExists) => Ok(()),
        Err(err) => Err(err),
    }
}

/// Unpack an in-memory CPIO archive slice into the active root VFS.
pub fn extract_cpio_archive(data: &[u8]) -> Result<usize, &'static str> {
    let archive = CpioArchive::new(data);
    let mut extracted_count = 0;

    for entry_res in archive.entries() {
        let entry = entry_res.map_err(|_| "Failed to parse CPIO entry header")?;
        let raw_name = entry
            .name()
            .trim_start_matches("./")
            .trim_start_matches('/');

        if raw_name.is_empty() || raw_name == "." {
            continue;
        }

        let full_path = format!("/{}", raw_name);

        if entry.is_directory() {
            if let Err(err) = mkdir_p(&full_path) {
                log::warn!("[Initramfs] Failed to mkdir '{}': {:?}", full_path, err);
            }
        } else if entry.is_regular_file() {
            if let Err(err) = create_file_with_parents(&full_path, entry.data()) {
                log::warn!(
                    "[Initramfs] Failed to create file '{}': {:?}",
                    full_path,
                    err
                );
            } else {
                extracted_count += 1;
            }
        } else if entry.is_symlink() {
            if let Ok(raw_target) = core::str::from_utf8(entry.data()) {
                let target = raw_target.trim_end_matches('\0');
                if let Err(err) = create_symlink_with_parents(&full_path, target) {
                    log::warn!(
                        "[Initramfs] Failed to create symlink '{}' -> '{}': {:?}",
                        full_path,
                        target,
                        err
                    );
                } else {
                    extracted_count += 1;
                }
            }
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
crate::MODULE_LICENSE!("BSD-2-Clause");
crate::MODULE_AUTHOR!("PetraOS Development Team");
crate::MODULE_DESCRIPTION!("In-Memory CPIO Initramfs Unpacker");
