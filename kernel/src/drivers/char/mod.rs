pub mod console;
pub mod keyboard;
pub mod mouse;

use super::{Device, DeviceType, register_device};
use crate::fs::vfs::{DirEntry, FileOps, FileType, InodeOps, Metadata, SeekFrom};
use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use ostd::sync::SpinLock;

pub trait CharDevice: Send + Sync {
    fn read(&self, buf: &mut [u8]) -> Result<usize, ostd::Error>;
    fn write(&self, buf: &[u8]) -> Result<usize, ostd::Error>;
}

// =============================================================================
// InputBuffer – generic ring buffer for input char devices
// =============================================================================

/// A fixed-capacity byte ring buffer shared between interrupt handlers
/// (producers) and `CharDevice::read` (consumer).
pub struct InputBuffer {
    buf: SpinLock<VecDeque<u8>>,
    max_len: usize,
}

impl InputBuffer {
    pub fn new(max_len: usize) -> Self {
        Self {
            buf: SpinLock::new(VecDeque::with_capacity(max_len)),
            max_len,
        }
    }

    /// Push a byte slice into the buffer, discarding oldest bytes when full.
    pub fn push(&self, data: &[u8]) {
        let mut guard = self.buf.lock();
        for &byte in data {
            if guard.len() >= self.max_len {
                guard.pop_front();
            }
            guard.push_back(byte);
        }
    }

    /// Drain up to `buf.len()` bytes into `buf`. Returns the number of bytes
    /// actually copied (0 when the buffer is empty).
    pub fn read_into(&self, buf: &mut [u8]) -> Result<usize, ostd::Error> {
        let mut guard = self.buf.lock();
        let count = core::cmp::min(buf.len(), guard.len());
        for (i, byte) in guard.drain(..count).enumerate() {
            buf[i] = byte;
        }
        Ok(count)
    }

    /// Return how many bytes are currently buffered.
    pub fn available(&self) -> usize {
        self.buf.lock().len()
    }
}

// =============================================================================
// CharDeviceWrapper – bridges CharDevice into the Device / devfs framework
// =============================================================================

struct CharDeviceWrapper {
    name: String,
    device: Arc<dyn CharDevice>,
}

impl Device for CharDeviceWrapper {
    fn name(&self) -> &str {
        &self.name
    }

    fn device_type(&self) -> DeviceType {
        DeviceType::Char
    }

    fn inode_ops(&self) -> Option<Arc<dyn InodeOps>> {
        Some(Arc::new(CharDeviceInode::new(self.device.clone())))
    }
}

pub fn register_char_device(name: &str, device: Arc<dyn CharDevice>) -> Result<(), ostd::Error> {
    let wrapper = Arc::new(CharDeviceWrapper {
        name: String::from(name),
        device,
    });
    register_device(wrapper)
}

// =============================================================================
// CharDeviceInode & CharDeviceFile – glue for the VFS layer
// =============================================================================

pub struct CharDeviceInode {
    device: Arc<dyn CharDevice>,
}

impl CharDeviceInode {
    pub fn new(device: Arc<dyn CharDevice>) -> Self {
        Self { device }
    }
}

impl InodeOps for CharDeviceInode {
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
        Ok(Metadata {
            size: 0,
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
        Ok(Box::new(CharDeviceFile {
            device: self.device.clone(),
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

pub struct CharDeviceFile {
    device: Arc<dyn CharDevice>,
}

impl FileOps for CharDeviceFile {
    fn read(&mut self, buf: &mut [u8], _offset: &mut usize) -> Result<usize, ostd::Error> {
        self.device.read(buf)
    }
    fn write(&mut self, buf: &[u8], _offset: &mut usize) -> Result<usize, ostd::Error> {
        self.device.write(buf)
    }
    fn seek(&mut self, _pos: SeekFrom, offset: &mut usize) -> Result<usize, ostd::Error> {
        Ok(*offset)
    }
    fn readdir(&mut self) -> Result<Vec<DirEntry>, ostd::Error> {
        Err(ostd::Error::InvalidArgs)
    }
}

// =============================================================================
// Initialization — register all built-in char devices with devfs
// =============================================================================

/// Register all character devices (keyboard, mouse, console).
///
/// Safe to call before `fs::init()` – devfs uses a lazy root inode, so device
/// nodes merely become visible once `/dev` is mounted.
pub fn init() {
    keyboard::init();
    mouse::init();
    console::init();
}
