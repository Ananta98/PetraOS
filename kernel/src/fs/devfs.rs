use alloc::sync::Arc;
use core::sync::atomic::AtomicU64;

use crate::device::DEVICE_MANAGER;
use crate::drivers::gpu::framebuffer::{fb_console_write_byte, FRAMEBUFFER};
use crate::fs::ramfs::RamDirInode;
use crate::fs::vfs::dentry::Dentry;
use crate::fs::vfs::mount::MOUNT_TABLE;
use crate::fs::vfs::types::{
    FileOps, FileSystem, Inode, InodeOps, InodeType, SuperBlock, VfsError,
};

/// Device filesystem, mounted at `/dev`.
pub struct DevFs;

impl FileSystem for DevFs {
    fn name(&self) -> &'static str {
        "devfs"
    }

    fn mount(&self) -> Result<SuperBlock, VfsError> {
        let next_ino = AtomicU64::new(1);
        let ino = next_ino.fetch_add(1, core::sync::atomic::Ordering::Relaxed);

        let root_inode = Arc::new(Inode {
            ino,
            inode_type: InodeType::Directory,
            ops: Arc::new(RamDirInode::new()),
        });

        Ok(SuperBlock {
            fs_name: "devfs",
            root_inode,
            next_ino,
            read_only: false,
        })
    }
}

fn try_read_console_byte() -> Option<u8> {
    // 1. Drain pending scancodes directly from PS/2 controller (port 0x64/0x60)
    // SAFETY: Reading status port 0x64 has no side effects and reading 0x60 when output buffer is full retrieves hardware scancode.
    let status = unsafe { crate::arch::ports::Ports::inb(0x64) };
    if (status & 0x01) != 0 && (status & 0x20) == 0 {
        let scancode = unsafe { crate::arch::ports::Ports::inb(0x60) };
        crate::drivers::char::keyboard::handle_scancode(scancode);
    }

    // 2. Check PS/2 keyboard buffer
    if let Some(byte) = crate::drivers::char::keyboard::KEY_RING_BUFFER.pop() {
        return Some(byte);
    }

    // 3. Check COM1 Serial Port (0x3F8) Line Status Register (0x3FD)
    // Bit 0 of LSR (0x3FD) is Data Ready (DR)
    // SAFETY: Reading standard COM1 16550 UART I/O ports.
    let lsr = unsafe { crate::arch::ports::Ports::inb(0x3FD) };
    if (lsr & 0x01) != 0 {
        // Data is ready on COM1 data port 0x3F8
        let byte = unsafe { crate::arch::ports::Ports::inb(0x3F8) };
        let byte = if byte == b'\r' { b'\n' } else { byte };
        return Some(byte);
    }

    None
}

#[inline]
fn rdtsc() -> u64 {
    let lo: u32;
    let hi: u32;
    // SAFETY: Reading CPU time-stamp counter on x86_64 architecture.
    unsafe {
        core::arch::asm!(
            "rdtsc",
            out("eax") lo,
            out("edx") hi,
            options(nomem, nostack, preserves_flags)
        );
    }
    ((hi as u64) << 32) | (lo as u64)
}

static URANDOM_STATE: core::sync::atomic::AtomicU64 =
    core::sync::atomic::AtomicU64::new(0x853c_49e6_748f_ea9b);

fn next_random_u64() -> u64 {
    let tsc = rdtsc();
    let mut state = URANDOM_STATE.load(core::sync::atomic::Ordering::Relaxed);
    if state == 0 {
        state = tsc | 1;
    }
    state ^= state << 13;
    state ^= state >> 7;
    state ^= state << 17;
    state = state.wrapping_add(tsc);
    URANDOM_STATE.store(state, core::sync::atomic::Ordering::Relaxed);
    state
}

/// Inode for the `/dev/console` device.
pub struct ConsoleInode;

impl InodeOps for ConsoleInode {
    fn open(&self) -> Result<Arc<dyn FileOps>, VfsError> {
        Ok(Arc::new(ConsoleFileOps))
    }
}

/// File operations for the console character device.
pub struct ConsoleFileOps;

impl FileOps for ConsoleFileOps {
    fn read(&self, _offset: usize, buf: &mut [u8]) -> Result<usize, VfsError> {
        if buf.is_empty() {
            return Ok(0);
        }

        let mut read_bytes = 0;

        // Block until at least one character is available, then drain what is immediately ready.
        while read_bytes < buf.len() {
            if let Some(ch) = try_read_console_byte() {
                buf[read_bytes] = ch;
                read_bytes += 1;
            } else if read_bytes > 0 {
                break;
            } else {
                core::hint::spin_loop();
                crate::sched::schedule(true);
            }
        }

        Ok(read_bytes)
    }

    fn write(&self, _offset: usize, buf: &[u8]) -> Result<usize, VfsError> {
        for &byte in buf {
            fb_console_write_byte(byte);
        }
        if let Ok(s) = core::str::from_utf8(buf) {
            log::info!("[CONSOLE] {}", s.trim_end());
        }
        Ok(buf.len())
    }
}

/// Inode for the `/dev/null` device.
pub struct NullInode;

impl InodeOps for NullInode {
    fn open(&self) -> Result<Arc<dyn FileOps>, VfsError> {
        Ok(Arc::new(NullFileOps))
    }
}

/// File operations for `/dev/null`.
pub struct NullFileOps;

impl FileOps for NullFileOps {
    fn read(&self, _offset: usize, _buf: &mut [u8]) -> Result<usize, VfsError> {
        Ok(0)
    }

    fn write(&self, _offset: usize, buf: &[u8]) -> Result<usize, VfsError> {
        Ok(buf.len())
    }
}

/// Inode for the `/dev/zero` device.
pub struct ZeroInode;

impl InodeOps for ZeroInode {
    fn open(&self) -> Result<Arc<dyn FileOps>, VfsError> {
        Ok(Arc::new(ZeroFileOps))
    }
}

/// File operations for `/dev/zero`.
pub struct ZeroFileOps;

impl FileOps for ZeroFileOps {
    fn read(&self, _offset: usize, buf: &mut [u8]) -> Result<usize, VfsError> {
        buf.fill(0);
        Ok(buf.len())
    }

    fn write(&self, _offset: usize, buf: &[u8]) -> Result<usize, VfsError> {
        Ok(buf.len())
    }
}

/// Inode for the `/dev/urandom` device.
pub struct UrandomInode;

impl InodeOps for UrandomInode {
    fn open(&self) -> Result<Arc<dyn FileOps>, VfsError> {
        Ok(Arc::new(UrandomFileOps))
    }
}

/// File operations for `/dev/urandom`.
pub struct UrandomFileOps;

impl FileOps for UrandomFileOps {
    fn read(&self, _offset: usize, buf: &mut [u8]) -> Result<usize, VfsError> {
        let mut chunks = buf.chunks_exact_mut(8);
        for chunk in chunks.by_ref() {
            let rand_val = next_random_u64();
            chunk.copy_from_slice(&rand_val.to_ne_bytes());
        }
        let remainder = chunks.into_remainder();
        if !remainder.is_empty() {
            let rand_val = next_random_u64();
            let bytes = rand_val.to_ne_bytes();
            remainder.copy_from_slice(&bytes[..remainder.len()]);
        }
        Ok(buf.len())
    }

    fn write(&self, _offset: usize, buf: &[u8]) -> Result<usize, VfsError> {
        let mut seed_xor = 0u64;
        for &b in buf.iter().take(8) {
            seed_xor = (seed_xor << 8) | (b as u64);
        }
        URANDOM_STATE.fetch_xor(seed_xor, core::sync::atomic::Ordering::Relaxed);
        Ok(buf.len())
    }
}

/// Inode for the `/dev/fb0` framebuffer device.
pub struct FbInode;

impl InodeOps for FbInode {
    fn open(&self) -> Result<Arc<dyn FileOps>, VfsError> {
        Ok(Arc::new(FbFileOps))
    }
}

/// File operations for `/dev/fb0`.
pub struct FbFileOps;

impl FileOps for FbFileOps {
    fn read(&self, offset: usize, buf: &mut [u8]) -> Result<usize, VfsError> {
        let fb_guard = FRAMEBUFFER.lock();
        let fb = fb_guard.as_ref().ok_or(VfsError::NotFound)?;
        let total_len = fb.len();
        if offset >= total_len {
            return Ok(0);
        }
        let available = total_len - offset;
        let count = core::cmp::min(buf.len(), available);
        // SAFETY: Pointer is within mapped framebuffer memory.
        unsafe {
            core::ptr::copy_nonoverlapping(fb.info().addr.add(offset), buf.as_mut_ptr(), count);
        }
        Ok(count)
    }

    fn write(&self, offset: usize, buf: &[u8]) -> Result<usize, VfsError> {
        let fb_guard = FRAMEBUFFER.lock();
        let fb = fb_guard.as_ref().ok_or(VfsError::NotFound)?;
        let total_len = fb.len();
        if offset >= total_len {
            return Ok(0);
        }
        let available = total_len - offset;
        let count = core::cmp::min(buf.len(), available);
        // SAFETY: Pointer is within mapped framebuffer memory.
        unsafe {
            core::ptr::copy_nonoverlapping(buf.as_ptr(), fb.info().addr.add(offset), count);
        }
        Ok(count)
    }
}

/// Inode for block devices registered in devfs.
pub struct BlockDeviceInode {
    pub device_name: &'static str,
}

impl InodeOps for BlockDeviceInode {
    fn open(&self) -> Result<Arc<dyn FileOps>, VfsError> {
        Ok(Arc::new(BlockDeviceFileOps {
            device_name: self.device_name,
        }))
    }
}

pub struct BlockDeviceFileOps {
    pub device_name: &'static str,
}

impl FileOps for BlockDeviceFileOps {
    fn read(&self, offset: usize, buf: &mut [u8]) -> Result<usize, VfsError> {
        let dm = DEVICE_MANAGER.read();
        for dev_arc in dm.get_devices() {
            let mut dev_lock = dev_arc.lock();
            if dev_lock.name() == self.device_name {
                if let Some(block_dev) = dev_lock.as_block_device_mut() {
                    let block_size = block_dev.block_size();
                    let block_id = (offset / block_size) as u64;
                    let mut read_bytes = 0;
                    let mut temp_buf = alloc::vec![0u8; block_size];

                    while read_bytes < buf.len() {
                        let current_block = block_id + (read_bytes / block_size) as u64;
                        block_dev
                            .read_block(current_block, &mut temp_buf)
                            .map_err(|_| VfsError::NotSupported)?;

                        let remaining = buf.len() - read_bytes;
                        let chunk = core::cmp::min(remaining, block_size);
                        buf[read_bytes..read_bytes + chunk].copy_from_slice(&temp_buf[..chunk]);
                        read_bytes += chunk;
                    }
                    return Ok(read_bytes);
                }
            }
        }
        Err(VfsError::NotFound)
    }

    fn write(&self, offset: usize, buf: &[u8]) -> Result<usize, VfsError> {
        let dm = DEVICE_MANAGER.read();
        for dev_arc in dm.get_devices() {
            let mut dev_lock = dev_arc.lock();
            if dev_lock.name() == self.device_name {
                if let Some(block_dev) = dev_lock.as_block_device_mut() {
                    let block_size = block_dev.block_size();
                    let block_id = (offset / block_size) as u64;
                    let mut written_bytes = 0;

                    while written_bytes < buf.len() {
                        let current_block = block_id + (written_bytes / block_size) as u64;
                        let remaining = buf.len() - written_bytes;
                        let chunk = core::cmp::min(remaining, block_size);

                        if chunk == block_size {
                            block_dev
                                .write_block(
                                    current_block,
                                    &buf[written_bytes..written_bytes + chunk],
                                )
                                .map_err(|_| VfsError::NotSupported)?;
                        } else {
                            let mut temp_buf = alloc::vec![0u8; block_size];
                            block_dev
                                .read_block(current_block, &mut temp_buf)
                                .map_err(|_| VfsError::NotSupported)?;
                            temp_buf[..chunk]
                                .copy_from_slice(&buf[written_bytes..written_bytes + chunk]);
                            block_dev
                                .write_block(current_block, &temp_buf)
                                .map_err(|_| VfsError::NotSupported)?;
                        }
                        written_bytes += chunk;
                    }
                    return Ok(written_bytes);
                }
            }
        }
        Err(VfsError::NotFound)
    }
}

impl DevFs {
    /// Mount the device filesystem at `/dev` and register core device nodes.
    pub fn init() -> Result<(), &'static str> {
        let mut mt = MOUNT_TABLE.write();
        let dev_mount = mt
            .mount("/dev", &DevFs)
            .map_err(|_| "Failed to mount devfs at /dev")?;

        // Register console character device
        let console_ino = dev_mount.superblock.alloc_ino();
        let console_inode = Arc::new(Inode {
            ino: console_ino,
            inode_type: InodeType::CharDevice,
            ops: Arc::new(ConsoleInode),
        });
        Dentry::add_child(&dev_mount.root_dentry, "console".into(), console_inode);

        // Register framebuffer device node /dev/fb0
        let fb_ino = dev_mount.superblock.alloc_ino();
        let fb_inode = Arc::new(Inode {
            ino: fb_ino,
            inode_type: InodeType::CharDevice,
            ops: Arc::new(FbInode),
        });
        Dentry::add_child(&dev_mount.root_dentry, "fb0".into(), fb_inode);

        // Register null character device /dev/null
        let null_ino = dev_mount.superblock.alloc_ino();
        let null_inode = Arc::new(Inode {
            ino: null_ino,
            inode_type: InodeType::CharDevice,
            ops: Arc::new(NullInode),
        });
        Dentry::add_child(&dev_mount.root_dentry, "null".into(), null_inode);

        // Register zero character device /dev/zero
        let zero_ino = dev_mount.superblock.alloc_ino();
        let zero_inode = Arc::new(Inode {
            ino: zero_ino,
            inode_type: InodeType::CharDevice,
            ops: Arc::new(ZeroInode),
        });
        Dentry::add_child(&dev_mount.root_dentry, "zero".into(), zero_inode);

        // Register urandom character device /dev/urandom
        let urandom_ino = dev_mount.superblock.alloc_ino();
        let urandom_inode = Arc::new(Inode {
            ino: urandom_ino,
            inode_type: InodeType::CharDevice,
            ops: Arc::new(UrandomInode),
        });
        Dentry::add_child(&dev_mount.root_dentry, "urandom".into(), urandom_inode);

        // Scan DEVICE_MANAGER and dynamically register discovered block devices
        let dm = DEVICE_MANAGER.read();
        for dev_arc in dm.get_devices() {
            let dev_lock = dev_arc.lock();
            if dev_lock.dev_type() == crate::device::DeviceType::Block {
                let dev_name = dev_lock.name();
                let vfs_name = if dev_name.contains("AHCI") {
                    "sda"
                } else if dev_name.contains("NVMe") {
                    "nvme0n1"
                } else {
                    continue;
                };

                let block_ino = dev_mount.superblock.alloc_ino();
                let block_inode = Arc::new(Inode {
                    ino: block_ino,
                    inode_type: InodeType::BlockDevice,
                    ops: Arc::new(BlockDeviceInode {
                        device_name: dev_name,
                    }),
                });
                Dentry::add_child(&dev_mount.root_dentry, vfs_name.into(), block_inode);
            }
        }

        log::info!("[DevFS] Mounted /dev successfully.");
        Ok(())
    }
}

/// Legacy wrapper for mounting devfs.
pub fn mount_devfs() {
    let _ = DevFs::init();
}

crate::fs_initcall!(DevFs::init);
crate::MODULE_LICENSE!("BSD-2-Clause");
crate::MODULE_AUTHOR!("PetraOS Development Team");
crate::MODULE_DESCRIPTION!("Device Filesystem Subsystem");
