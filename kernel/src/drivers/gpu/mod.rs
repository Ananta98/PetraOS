use crate::device::{Device, DeviceType, register_device};
use crate::drivers::gpu::framebuffer::{Framebuffer, VideoMode};
use crate::fs::vfs::{DirEntry, FileOps, FileType, InodeOps, Metadata, SeekFrom};
use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use ostd::sync::SpinLock;

pub mod framebuffer;

/// Trait that all current and future GPU/display drivers must implement.
pub trait GpuDriver: Send + Sync {
    /// Retrieve the unique name of the GPU driver.
    fn name(&self) -> &str;

    /// Retrieve the current active video mode.
    fn current_mode(&self) -> VideoMode;

    /// Set a new video mode.
    fn set_mode(&self, mode: VideoMode) -> Result<(), ostd::Error>;

    /// Retrieve a list of supported video modes.
    fn supported_modes(&self) -> &[VideoMode];

    /// Retrieve the generic framebuffer struct managed by the driver.
    fn framebuffer(&self) -> Arc<Framebuffer>;
}

/// GPU driver manager for registering and accessing GPU devices.
pub struct GpuManager {
    drivers: SpinLock<BTreeMap<String, Arc<dyn GpuDriver>>>,
}

impl GpuManager {
    /// Create a new GpuManager instance.
    pub const fn new() -> Self {
        Self {
            drivers: SpinLock::new(BTreeMap::new()),
        }
    }

    /// Register a GPU driver and expose its framebuffer device in `/dev/fbN`.
    pub fn register_driver(&self, driver: Arc<dyn GpuDriver>) -> Result<(), ostd::Error> {
        let name = String::from(driver.name());
        let mut drivers = self.drivers.lock();
        if drivers.contains_key(&name) {
            return Err(ostd::Error::InvalidArgs);
        }

        let id = drivers.len();
        let fb_dev_name = format!("fb{}", id);

        drivers.insert(name, driver.clone());

        // Create the FbDevice representation for devfs
        let device = Arc::new(GpuDevice {
            name: fb_dev_name,
            driver,
        });

        register_device(device)?;
        Ok(())
    }

    /// Get a registered GPU driver by name.
    pub fn get_driver(&self, name: &str) -> Option<Arc<dyn GpuDriver>> {
        self.drivers.lock().get(name).cloned()
    }
}

/// Global static GPU manager.
pub static GPU_MANAGER: GpuManager = GpuManager::new();

/// GpuDevice exposes a specific GpuDriver's framebuffer to the VFS.
struct GpuDevice {
    name: String,
    driver: Arc<dyn GpuDriver>,
}

impl Device for GpuDevice {
    fn name(&self) -> &str {
        &self.name
    }

    fn device_type(&self) -> DeviceType {
        DeviceType::Char
    }

    fn inode_ops(&self) -> Option<Arc<dyn InodeOps>> {
        Some(Arc::new(GpuInode {
            driver: self.driver.clone(),
        }))
    }
}

struct GpuInode {
    driver: Arc<dyn GpuDriver>,
}

impl InodeOps for GpuInode {
    fn lookup(&self, _name: &str) -> Result<Arc<dyn InodeOps>, ostd::Error> {
        Err(ostd::Error::InvalidArgs)
    }

    fn create(&self, _name: &str, _mode: u32) -> Result<Arc<dyn InodeOps>, ostd::Error> {
        Err(ostd::Error::InvalidArgs)
    }

    fn mkdir(&self, _name: &str, _mode: u32) -> Result<Arc<dyn InodeOps>, ostd::Error> {
        Err(ostd::Error::InvalidArgs)
    }

    fn symlink(&self, _name: &str, _target: &str) -> Result<Arc<dyn InodeOps>, ostd::Error> {
        Err(ostd::Error::InvalidArgs)
    }

    fn metadata(&self) -> Result<Metadata, ostd::Error> {
        let mode = self.driver.current_mode();
        let size = (mode.pitch * mode.height) as usize;
        Ok(Metadata {
            size,
            file_type: FileType::CharDevice,
            mode: 0o660,
            inode_num: 0,
            nlink: 1,
        })
    }

    fn read_link(&self) -> Result<String, ostd::Error> {
        Err(ostd::Error::InvalidArgs)
    }

    fn open(&self, _flags: u32) -> Result<Box<dyn FileOps>, ostd::Error> {
        Ok(Box::new(GpuFile {
            fb: self.driver.framebuffer(),
        }))
    }

    fn as_any(&self) -> &dyn core::any::Any {
        self
    }

    fn unlink(&self, _name: &str) -> Result<(), ostd::Error> {
        Err(ostd::Error::InvalidArgs)
    }

    fn rename(
        &self,
        _old_name: &str,
        _new_parent: &Arc<dyn InodeOps>,
        _new_name: &str,
    ) -> Result<(), ostd::Error> {
        Err(ostd::Error::InvalidArgs)
    }
}

struct GpuFile {
    fb: Arc<Framebuffer>,
}

impl FileOps for GpuFile {
    fn read(&mut self, buf: &mut [u8], offset: &mut usize) -> Result<usize, ostd::Error> {
        let mode = self.fb.mode();
        let size = (mode.pitch * mode.height) as usize;
        if *offset >= size {
            return Ok(0);
        }
        let pixels = self.fb.pixels.lock();
        let len = core::cmp::min(buf.len(), size - *offset);
        buf[..len].copy_from_slice(&pixels[*offset..*offset + len]);
        *offset += len;
        Ok(len)
    }

    fn write(&mut self, buf: &[u8], offset: &mut usize) -> Result<usize, ostd::Error> {
        let mode = self.fb.mode();
        let size = (mode.pitch * mode.height) as usize;
        if *offset >= size {
            return Err(ostd::Error::InvalidArgs);
        }
        let mut pixels = self.fb.pixels.lock();
        let len = core::cmp::min(buf.len(), size - *offset);
        pixels[*offset..*offset + len].copy_from_slice(&buf[..len]);
        *offset += len;
        Ok(len)
    }

    fn seek(&mut self, pos: SeekFrom, offset: &mut usize) -> Result<usize, ostd::Error> {
        let mode = self.fb.mode();
        let size = (mode.pitch * mode.height) as usize;
        let new_offset = match pos {
            SeekFrom::Start(n) => n,
            SeekFrom::Current(n) => {
                let val = *offset as isize + n;
                if val < 0 {
                    return Err(ostd::Error::InvalidArgs);
                }
                val as usize
            }
            SeekFrom::End(n) => {
                let val = size as isize + n;
                if val < 0 {
                    return Err(ostd::Error::InvalidArgs);
                }
                val as usize
            }
        };
        *offset = core::cmp::min(new_offset, size);
        Ok(*offset)
    }

    fn readdir(&mut self) -> Result<Vec<DirEntry>, ostd::Error> {
        Err(ostd::Error::InvalidArgs)
    }
}

/// Initialize all GPU/display related drivers.
pub fn init() {
    framebuffer::init();
}
