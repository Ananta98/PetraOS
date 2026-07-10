use crate::fs::vfs::{
    Dentry, DirEntry, FileOps, FileSystem, FileType, InodeOps, Metadata, Result, SeekFrom,
    SuperBlock,
};
use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use ostd::Error;
use ostd::sync::SpinLock;
use spin::Once;

static INODE_COUNTER: AtomicU64 = AtomicU64::new(1000);
static DEVFS_ROOT: Once<Arc<DevfsInode>> = Once::new();

pub fn get_devfs_root() -> Arc<DevfsInode> {
    DEVFS_ROOT
        .call_once(|| DevfsInode::new_directory(0o755))
        .clone()
}

pub struct DevfsInodeInner {
    metadata: Metadata,
    entries: BTreeMap<String, Arc<DevfsInode>>,
    device: Option<Arc<dyn InodeOps>>,
}

pub struct DevfsInode {
    inner: Arc<SpinLock<DevfsInodeInner>>,
}

impl DevfsInode {
    pub fn new_directory(mode: u32) -> Arc<Self> {
        let inode_num = INODE_COUNTER.fetch_add(1, Ordering::Relaxed);
        Arc::new(Self {
            inner: Arc::new(SpinLock::new(DevfsInodeInner {
                metadata: Metadata {
                    size: 0,
                    file_type: FileType::Directory,
                    mode,
                    inode_num,
                    nlink: 1,
                },
                entries: BTreeMap::new(),
                device: None,
            })),
        })
    }

    pub fn new_device(file_type: FileType, mode: u32, device: Arc<dyn InodeOps>) -> Arc<Self> {
        let inode_num = INODE_COUNTER.fetch_add(1, Ordering::Relaxed);
        Arc::new(Self {
            inner: Arc::new(SpinLock::new(DevfsInodeInner {
                metadata: Metadata {
                    size: 0,
                    file_type,
                    mode,
                    inode_num,
                    nlink: 1,
                },
                entries: BTreeMap::new(),
                device: Some(device),
            })),
        })
    }

    pub fn device(&self) -> Option<Arc<dyn InodeOps>> {
        self.inner.lock().device.clone()
    }
}

impl InodeOps for DevfsInode {
    fn lookup(&self, name: &str) -> Result<Arc<dyn InodeOps>> {
        let inner = self.inner.lock();
        if inner.metadata.file_type != FileType::Directory {
            return Err(Error::InvalidArgs);
        }
        let child = inner.entries.get(name).cloned().ok_or(Error::InvalidArgs)?;
        Ok(child as Arc<dyn InodeOps>)
    }

    fn create(&self, _name: &str, _mode: u32) -> Result<Arc<dyn InodeOps>> {
        Err(Error::InvalidArgs)
    }

    fn mkdir(&self, _name: &str, _mode: u32) -> Result<Arc<dyn InodeOps>> {
        Err(Error::InvalidArgs)
    }

    fn symlink(&self, _name: &str, _target: &str) -> Result<Arc<dyn InodeOps>> {
        Err(Error::InvalidArgs)
    }

    fn metadata(&self) -> Result<Metadata> {
        let inner = self.inner.lock();
        if let Some(ref dev) = inner.device {
            if let Ok(mut dev_meta) = dev.metadata() {
                dev_meta.inode_num = inner.metadata.inode_num;
                dev_meta.file_type = inner.metadata.file_type;
                return Ok(dev_meta);
            }
        }
        Ok(inner.metadata.clone())
    }

    fn read_link(&self) -> Result<String> {
        Err(Error::InvalidArgs)
    }

    fn open(&self, flags: u32) -> Result<Box<dyn FileOps>> {
        let inner = self.inner.lock();
        if let Some(ref dev) = inner.device {
            dev.open(flags)
        } else if inner.metadata.file_type == FileType::Directory {
            Ok(Box::new(DevfsFile {
                inner: self.inner.clone(),
            }))
        } else {
            Err(Error::InvalidArgs)
        }
    }

    fn as_any(&self) -> &dyn core::any::Any {
        self
    }

    fn unlink(&self, _name: &str) -> Result<()> {
        Err(Error::InvalidArgs)
    }

    fn rename(
        &self,
        _old_name: &str,
        _new_parent: &Arc<dyn InodeOps>,
        _new_name: &str,
    ) -> Result<()> {
        Err(Error::InvalidArgs)
    }
}

pub struct DevfsFile {
    inner: Arc<SpinLock<DevfsInodeInner>>,
}

impl FileOps for DevfsFile {
    fn read(&mut self, _buf: &mut [u8], _offset: &mut usize) -> Result<usize> {
        Err(Error::InvalidArgs)
    }

    fn write(&mut self, _buf: &[u8], _offset: &mut usize) -> Result<usize> {
        Err(Error::InvalidArgs)
    }

    fn seek(&mut self, _pos: SeekFrom, _offset: &mut usize) -> Result<usize> {
        Err(Error::InvalidArgs)
    }

    fn readdir(&mut self) -> Result<Vec<DirEntry>> {
        let inner = self.inner.lock();
        if inner.metadata.file_type != FileType::Directory {
            return Err(Error::InvalidArgs);
        }
        let mut result = Vec::new();
        result.push(DirEntry {
            name: String::from("."),
            inode_num: inner.metadata.inode_num,
            file_type: FileType::Directory,
        });
        result.push(DirEntry {
            name: String::from(".."),
            inode_num: 0,
            file_type: FileType::Directory,
        });
        for (name, child) in &inner.entries {
            let child_meta = child.metadata()?;
            result.push(DirEntry {
                name: name.clone(),
                inode_num: child_meta.inode_num,
                file_type: child_meta.file_type,
            });
        }
        Ok(result)
    }
}

pub fn register_device(
    path: &str,
    file_type: FileType,
    mode: u32,
    device: Arc<dyn InodeOps>,
) -> Result<()> {
    let root = get_devfs_root();
    let mut current = root;
    let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    if parts.is_empty() {
        return Err(Error::InvalidArgs);
    }

    for (i, part) in parts.iter().enumerate() {
        let is_last = i == parts.len() - 1;
        let mut inner = current.inner.lock();
        if inner.metadata.file_type != FileType::Directory {
            return Err(Error::InvalidArgs);
        }

        if is_last {
            if inner.entries.contains_key(*part) {
                return Err(Error::InvalidArgs);
            }
            let dev_inode = DevfsInode::new_device(file_type, mode, device.clone());
            inner.entries.insert(String::from(*part), dev_inode);
            break;
        } else {
            let next = if let Some(child) = inner.entries.get(*part) {
                if child.inner.lock().metadata.file_type != FileType::Directory {
                    return Err(Error::InvalidArgs);
                }
                child.clone()
            } else {
                let dir = DevfsInode::new_directory(0o755);
                inner.entries.insert(String::from(*part), dir.clone());
                dir
            };
            drop(inner);
            current = next;
        }
    }
    Ok(())
}

pub fn unregister_device(path: &str) -> Result<()> {
    let root = get_devfs_root();
    let mut current = root;
    let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    if parts.is_empty() {
        return Err(Error::InvalidArgs);
    }

    for (i, part) in parts.iter().enumerate() {
        let is_last = i == parts.len() - 1;
        let mut inner = current.inner.lock();
        if inner.metadata.file_type != FileType::Directory {
            return Err(Error::InvalidArgs);
        }

        if is_last {
            if inner.entries.remove(*part).is_some() {
                break;
            } else {
                return Err(Error::InvalidArgs);
            }
        } else {
            let next = inner
                .entries
                .get(*part)
                .cloned()
                .ok_or(Error::InvalidArgs)?;
            drop(inner);
            current = next;
        }
    }
    Ok(())
}

pub struct NullInode;

impl InodeOps for NullInode {
    fn lookup(&self, _name: &str) -> Result<Arc<dyn InodeOps>> {
        Err(Error::InvalidArgs)
    }
    fn create(&self, _name: &str, _mode: u32) -> Result<Arc<dyn InodeOps>> {
        Err(Error::InvalidArgs)
    }
    fn mkdir(&self, _name: &str, _mode: u32) -> Result<Arc<dyn InodeOps>> {
        Err(Error::InvalidArgs)
    }
    fn symlink(&self, _name: &str, _target: &str) -> Result<Arc<dyn InodeOps>> {
        Err(Error::InvalidArgs)
    }
    fn metadata(&self) -> Result<Metadata> {
        Ok(Metadata {
            size: 0,
            file_type: FileType::CharDevice,
            mode: 0o666,
            inode_num: 0,
            nlink: 1,
        })
    }
    fn read_link(&self) -> Result<String> {
        Err(Error::InvalidArgs)
    }
    fn open(&self, _flags: u32) -> Result<Box<dyn FileOps>> {
        Ok(Box::new(NullFile))
    }
    fn as_any(&self) -> &dyn core::any::Any {
        self
    }
    fn unlink(&self, _name: &str) -> Result<()> {
        Err(Error::InvalidArgs)
    }
    fn rename(
        &self,
        _old_name: &str,
        _new_parent: &Arc<dyn InodeOps>,
        _new_name: &str,
    ) -> Result<()> {
        Err(Error::InvalidArgs)
    }
}

pub struct NullFile;

impl FileOps for NullFile {
    fn read(&mut self, _buf: &mut [u8], _offset: &mut usize) -> Result<usize> {
        Ok(0)
    }
    fn write(&mut self, buf: &[u8], _offset: &mut usize) -> Result<usize> {
        Ok(buf.len())
    }
    fn seek(&mut self, _pos: SeekFrom, offset: &mut usize) -> Result<usize> {
        Ok(*offset)
    }
    fn readdir(&mut self) -> Result<Vec<DirEntry>> {
        Err(Error::InvalidArgs)
    }
}

pub struct ZeroInode;

impl InodeOps for ZeroInode {
    fn lookup(&self, _name: &str) -> Result<Arc<dyn InodeOps>> {
        Err(Error::InvalidArgs)
    }
    fn create(&self, _name: &str, _mode: u32) -> Result<Arc<dyn InodeOps>> {
        Err(Error::InvalidArgs)
    }
    fn mkdir(&self, _name: &str, _mode: u32) -> Result<Arc<dyn InodeOps>> {
        Err(Error::InvalidArgs)
    }
    fn symlink(&self, _name: &str, _target: &str) -> Result<Arc<dyn InodeOps>> {
        Err(Error::InvalidArgs)
    }
    fn metadata(&self) -> Result<Metadata> {
        Ok(Metadata {
            size: 0,
            file_type: FileType::CharDevice,
            mode: 0o666,
            inode_num: 0,
            nlink: 1,
        })
    }
    fn read_link(&self) -> Result<String> {
        Err(Error::InvalidArgs)
    }
    fn open(&self, _flags: u32) -> Result<Box<dyn FileOps>> {
        Ok(Box::new(ZeroFile))
    }
    fn as_any(&self) -> &dyn core::any::Any {
        self
    }
    fn unlink(&self, _name: &str) -> Result<()> {
        Err(Error::InvalidArgs)
    }
    fn rename(
        &self,
        _old_name: &str,
        _new_parent: &Arc<dyn InodeOps>,
        _new_name: &str,
    ) -> Result<()> {
        Err(Error::InvalidArgs)
    }
}

pub struct ZeroFile;

impl FileOps for ZeroFile {
    fn read(&mut self, buf: &mut [u8], _offset: &mut usize) -> Result<usize> {
        buf.fill(0);
        Ok(buf.len())
    }
    fn write(&mut self, buf: &[u8], _offset: &mut usize) -> Result<usize> {
        Ok(buf.len())
    }
    fn seek(&mut self, _pos: SeekFrom, offset: &mut usize) -> Result<usize> {
        Ok(*offset)
    }
    fn readdir(&mut self) -> Result<Vec<DirEntry>> {
        Err(Error::InvalidArgs)
    }
}

pub fn init_devices() {
    let _ = register_device("null", FileType::CharDevice, 0o666, Arc::new(NullInode));
    let _ = register_device("zero", FileType::CharDevice, 0o666, Arc::new(ZeroInode));
}

pub struct DevFs;

static DEVFS_INIT: Once<()> = Once::new();

impl FileSystem for DevFs {
    fn name(&self) -> &'static str {
        "devfs"
    }

    fn mount(&self, _flags: u32, _data: &[u8]) -> Result<Arc<SuperBlock>> {
        DEVFS_INIT.call_once(|| {
            init_devices();
        });

        let sb = Arc::new(SuperBlock {
            fs_type: String::from(self.name()),
            root_dentry: SpinLock::new(None),
        });
        let root_dentry = Dentry::new("/", get_devfs_root() as Arc<dyn InodeOps>, None);
        *sb.root_dentry.lock() = Some(root_dentry);
        Ok(sb)
    }
}

#[cfg(ktest)]
mod tests {
    use super::*;
    use crate::fs::ramfs::RamFs;
    use crate::fs::vfs::{init_root_fs, mount, register_filesystem, resolve_path};
    use ostd::prelude::ktest;

    #[ktest]
    fn test_devfs() {
        let ramfs = Arc::new(RamFs);
        let _ = register_filesystem(ramfs);
        let _ = init_root_fs("ramfs", &[]);

        let devfs = Arc::new(DevFs);
        register_filesystem(devfs).unwrap();

        let root = crate::fs::vfs::ROOT_DENTRY
            .lock()
            .as_ref()
            .cloned()
            .unwrap();
        root.inode.mkdir("dev", 0o755).unwrap();

        mount("devfs", "/dev", 0, &[]).unwrap();

        let null_dentry = resolve_path("/dev/null").unwrap();
        let null_meta = null_dentry.inode.metadata().unwrap();
        assert_eq!(null_meta.file_type, FileType::CharDevice);

        let mut null_ops = null_dentry.inode.open(0).unwrap();
        let mut offset = 0;
        let written = null_ops.write(b"hello", &mut offset).unwrap();
        assert_eq!(written, 5);

        let mut buf = [0u8; 10];
        let mut read_offset = 0;
        let read_len = null_ops.read(&mut buf, &mut read_offset).unwrap();
        assert_eq!(read_len, 0);

        let zero_dentry = resolve_path("/dev/zero").unwrap();
        let zero_meta = zero_dentry.inode.metadata().unwrap();
        assert_eq!(zero_meta.file_type, FileType::CharDevice);

        let mut zero_ops = zero_dentry.inode.open(0).unwrap();
        let mut zero_read_offset = 0;
        let mut zero_buf = [5u8; 5];
        let zero_read_len = zero_ops.read(&mut zero_buf, &mut zero_read_offset).unwrap();
        assert_eq!(zero_read_len, 5);
        assert_eq!(zero_buf, [0u8; 5]);

        // Clean up to keep other tests isolated and prevent double-registration errors
        crate::fs::vfs::unregister_filesystem("devfs").unwrap();
        crate::fs::vfs::unregister_filesystem("ramfs").unwrap();
        *crate::fs::vfs::ROOT_DENTRY.lock() = None;
        *crate::fs::vfs::CWD_DENTRY.lock() = None;
        crate::fs::vfs::DENTRY_CACHE.clear();
    }
}
