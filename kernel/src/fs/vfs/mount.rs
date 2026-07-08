use crate::fs::vfs::types::{Dentry, FileSystem, Result, SuperBlock};
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use ostd::Error;
use ostd::sync::SpinLock;

/// Registry of all registered filesystem types.
static FILESYSTEM_TYPES: SpinLock<BTreeMap<String, Arc<dyn FileSystem>>> =
    SpinLock::new(BTreeMap::new());

/// Global root dentry of the entire VFS tree.
pub static ROOT_DENTRY: SpinLock<Option<Arc<Dentry>>> = SpinLock::new(None);

/// Global current working directory (CWD) dentry of the kernel/process.
pub static CWD_DENTRY: SpinLock<Option<Arc<Dentry>>> = SpinLock::new(None);

/// Register a filesystem driver type (e.g. "tmpfs").
pub fn register_filesystem(fs: Arc<dyn FileSystem>) -> Result<()> {
    let mut types = FILESYSTEM_TYPES.lock();
    let name = String::from(fs.name());
    if types.contains_key(&name) {
        return Err(Error::InvalidArgs);
    }
    types.insert(name, fs);
    Ok(())
}

/// Unregister a filesystem driver type.
pub fn unregister_filesystem(name: &str) -> Result<()> {
    let mut types = FILESYSTEM_TYPES.lock();
    if types.remove(name).is_none() {
        return Err(Error::InvalidArgs);
    }
    Ok(())
}

/// Look up a filesystem driver type by name.
pub fn get_filesystem(name: &str) -> Option<Arc<dyn FileSystem>> {
    let types = FILESYSTEM_TYPES.lock();
    types.get(name).cloned()
}

/// Initialize the root filesystem.
pub fn init_root_fs(fs_type: &str, data: &[u8]) -> Result<()> {
    let fs = get_filesystem(fs_type).ok_or(Error::InvalidArgs)?;
    let sb = fs.mount(0, data)?;

    let root_dentry = sb.root_dentry.lock().clone().ok_or(Error::InvalidArgs)?;

    *ROOT_DENTRY.lock() = Some(root_dentry.clone());
    *CWD_DENTRY.lock() = Some(root_dentry);
    Ok(())
}

/// Mount a filesystem at a target path.
pub fn mount(fs_type: &str, target_path: &str, flags: u32, data: &[u8]) -> Result<()> {
    let fs = get_filesystem(fs_type).ok_or(Error::InvalidArgs)?;
    let target_dentry = crate::fs::vfs::path::resolve_path(target_path)?;

    let target_meta = target_dentry.inode.metadata()?;
    if target_meta.file_type != crate::fs::vfs::types::FileType::Directory {
        return Err(Error::InvalidArgs);
    }

    let sb = fs.mount(flags, data)?;
    let root_dentry = sb.root_dentry.lock().clone().ok_or(Error::InvalidArgs)?;

    *root_dentry.mount_point.lock() = Some(Arc::downgrade(&target_dentry));
    *target_dentry.mounted_sb.lock() = Some(sb);

    Ok(())
}
