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

static INODE_COUNTER: AtomicU64 = AtomicU64::new(1);

struct RamfsInodeInner {
    metadata: Metadata,
    data: Vec<u8>,
    entries: BTreeMap<String, Arc<RamfsInode>>,
}

pub struct RamfsInode {
    inner: Arc<SpinLock<RamfsInodeInner>>,
}

impl RamfsInode {
    pub fn new(file_type: FileType, mode: u32) -> Arc<Self> {
        let inode_num = INODE_COUNTER.fetch_add(1, Ordering::Relaxed);
        Arc::new(Self {
            inner: Arc::new(SpinLock::new(RamfsInodeInner {
                metadata: Metadata {
                    size: 0,
                    file_type,
                    mode,
                    uid: 0,
                    gid: 0,
                    inode_num,
                    nlink: 1,
                },
                data: Vec::new(),
                entries: BTreeMap::new(),
            })),
        })
    }
}

impl InodeOps for RamfsInode {
    fn lookup(&self, name: &str) -> Result<Arc<dyn InodeOps>> {
        let inner = self.inner.lock();
        if inner.metadata.file_type != FileType::Directory {
            return Err(Error::InvalidArgs);
        }
        let child = inner.entries.get(name).cloned().ok_or(Error::InvalidArgs)?;
        Ok(child as Arc<dyn InodeOps>)
    }

    fn create(&self, name: &str, mode: u32) -> Result<Arc<dyn InodeOps>> {
        let mut inner = self.inner.lock();
        if inner.metadata.file_type != FileType::Directory {
            return Err(Error::InvalidArgs);
        }
        if inner.entries.contains_key(name) {
            return Err(Error::InvalidArgs);
        }
        let child = RamfsInode::new(FileType::Regular, mode);
        inner.entries.insert(String::from(name), child.clone());
        Ok(child as Arc<dyn InodeOps>)
    }

    fn mkdir(&self, name: &str, mode: u32) -> Result<Arc<dyn InodeOps>> {
        let mut inner = self.inner.lock();
        if inner.metadata.file_type != FileType::Directory {
            return Err(Error::InvalidArgs);
        }
        if inner.entries.contains_key(name) {
            return Err(Error::InvalidArgs);
        }
        let child = RamfsInode::new(FileType::Directory, mode);
        inner.entries.insert(String::from(name), child.clone());
        Ok(child as Arc<dyn InodeOps>)
    }

    fn symlink(&self, name: &str, target: &str) -> Result<Arc<dyn InodeOps>> {
        let mut inner = self.inner.lock();
        if inner.metadata.file_type != FileType::Directory {
            return Err(Error::InvalidArgs);
        }
        if inner.entries.contains_key(name) {
            return Err(Error::InvalidArgs);
        }
        let child = RamfsInode::new(FileType::Symlink, 0o777);
        let mut child_inner = child.inner.lock();
        child_inner.data = target.as_bytes().to_vec();
        child_inner.metadata.size = target.len();
        drop(child_inner);
        inner.entries.insert(String::from(name), child.clone());
        Ok(child as Arc<dyn InodeOps>)
    }

    fn metadata(&self) -> Result<Metadata> {
        Ok(self.inner.lock().metadata.clone())
    }

    fn chmod(&self, mode: u32) -> Result<()> {
        self.inner.lock().metadata.mode = mode;
        Ok(())
    }

    fn chown(&self, uid: u32, gid: u32) -> Result<()> {
        let mut inner = self.inner.lock();
        inner.metadata.uid = uid;
        inner.metadata.gid = gid;
        Ok(())
    }

    fn read_link(&self) -> Result<String> {
        let inner = self.inner.lock();
        if inner.metadata.file_type != FileType::Symlink {
            return Err(Error::InvalidArgs);
        }
        String::from_utf8(inner.data.clone()).map_err(|_| Error::IoError)
    }

    fn open(&self, _flags: u32) -> Result<Box<dyn FileOps>> {
        Ok(Box::new(RamfsFile {
            inner: self.inner.clone(),
        }))
    }

    fn as_any(&self) -> &dyn core::any::Any {
        self
    }

    fn unlink(&self, name: &str) -> Result<()> {
        let mut inner = self.inner.lock();
        if inner.metadata.file_type != FileType::Directory {
            return Err(Error::InvalidArgs);
        }
        if inner.entries.remove(name).is_some() {
            Ok(())
        } else {
            Err(Error::InvalidArgs)
        }
    }

    fn rename(&self, old_name: &str, new_parent: &Arc<dyn InodeOps>, new_name: &str) -> Result<()> {
        let mut inner = self.inner.lock();
        if inner.metadata.file_type != FileType::Directory {
            return Err(Error::InvalidArgs);
        }
        let child = inner.entries.remove(old_name).ok_or(Error::InvalidArgs)?;

        let new_parent_any = new_parent.as_any();
        if let Some(target_parent) = new_parent_any.downcast_ref::<RamfsInode>() {
            let mut target_inner = target_parent.inner.lock();
            if target_inner.metadata.file_type != FileType::Directory {
                inner.entries.insert(String::from(old_name), child); // restore
                return Err(Error::InvalidArgs);
            }
            if target_inner.entries.contains_key(new_name) {
                inner.entries.insert(String::from(old_name), child); // restore
                return Err(Error::InvalidArgs);
            }
            target_inner.entries.insert(String::from(new_name), child);
            Ok(())
        } else {
            inner.entries.insert(String::from(old_name), child); // restore
            Err(Error::InvalidArgs)
        }
    }
}

pub struct RamfsFile {
    inner: Arc<SpinLock<RamfsInodeInner>>,
}

impl FileOps for RamfsFile {
    fn read(&mut self, buf: &mut [u8], offset: &mut usize) -> Result<usize> {
        let inner = self.inner.lock();
        if inner.metadata.file_type != FileType::Regular {
            return Err(Error::InvalidArgs);
        }
        if *offset >= inner.data.len() {
            return Ok(0);
        }
        let read_len = core::cmp::min(buf.len(), inner.data.len() - *offset);
        buf[..read_len].copy_from_slice(&inner.data[*offset..*offset + read_len]);
        *offset += read_len;
        Ok(read_len)
    }

    fn write(&mut self, buf: &[u8], offset: &mut usize) -> Result<usize> {
        let mut inner = self.inner.lock();
        if inner.metadata.file_type != FileType::Regular {
            return Err(Error::InvalidArgs);
        }
        let write_end = *offset + buf.len();
        if write_end > inner.data.len() {
            inner.data.resize(write_end, 0);
        }
        inner.data[*offset..write_end].copy_from_slice(buf);
        inner.metadata.size = core::cmp::max(inner.metadata.size, write_end);
        *offset = write_end;
        Ok(buf.len())
    }

    fn seek(&mut self, pos: SeekFrom, offset: &mut usize) -> Result<usize> {
        let inner = self.inner.lock();
        let new_offset = match pos {
            SeekFrom::Start(val) => val as isize,
            SeekFrom::Current(val) => *offset as isize + val,
            SeekFrom::End(val) => inner.data.len() as isize + val,
        };
        if new_offset < 0 {
            return Err(Error::InvalidArgs);
        }
        *offset = new_offset as usize;
        Ok(*offset)
    }

    fn readdir(&mut self) -> Result<Vec<DirEntry>> {
        let inner = self.inner.lock();
        if inner.metadata.file_type != FileType::Directory {
            return Err(Error::InvalidArgs);
        }
        let mut result = Vec::new();
        // Add "." and ".."
        result.push(DirEntry {
            name: String::from("."),
            inode_num: inner.metadata.inode_num,
            file_type: FileType::Directory,
        });
        result.push(DirEntry {
            name: String::from(".."),
            inode_num: 0, // Placeholder
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

pub struct RamFs;

impl FileSystem for RamFs {
    fn name(&self) -> &'static str {
        "ramfs"
    }

    fn mount(&self, _flags: u32, _data: &[u8]) -> Result<Arc<SuperBlock>> {
        let root_inode = RamfsInode::new(FileType::Directory, 0o755);
        let sb = Arc::new(SuperBlock {
            fs_type: String::from(self.name()),
            root_dentry: SpinLock::new(None),
        });
        let root_dentry = Dentry::new("/", root_inode as Arc<dyn InodeOps>, None);
        *sb.root_dentry.lock() = Some(root_dentry);
        Ok(sb)
    }
}
