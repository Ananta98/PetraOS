//! POSIX UNIX-like Pseudo-Terminal (PTY) Subsystem
//!
//! Implements master/slave paired pseudo-terminals (/dev/ptmx and /dev/pts/N),
//! with bidirectional communication channels and line discipline processing.

use alloc::collections::{BTreeMap, VecDeque};
use alloc::format;
use alloc::sync::Arc;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::fs::vfs::dentry::Dentry;
use crate::fs::vfs::mount::MOUNT_TABLE;
use crate::fs::vfs::types::{FileOps, Inode, InodeOps, InodeType, Stat, VfsError};
use crate::sync::spinlock::Spinlock;
use crate::tty::termios::{
    FIONREAD, LineDiscipline, TCGETS, TCSETS, TCSETSF, TCSETSW, TIOCGPGRP, TIOCGPTN, TIOCGWINSZ,
    TIOCNOTTY, TIOCSCTTY, TIOCSPGRP, TIOCSPTLCK, TIOCSWINSZ, Termios, WinSize,
};

/// Represents a single bidirectional PTY channel pair.
pub struct PtyPair {
    pub id: u32,
    pub master_buffer: Spinlock<VecDeque<u8>>,
    pub slave_ldisc: Spinlock<LineDiscipline>,
    pub locked: AtomicBool,
    pub slave_open_count: AtomicUsize,
    pub master_open: AtomicBool,
}

impl PtyPair {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            master_buffer: Spinlock::new(VecDeque::with_capacity(1024)),
            slave_ldisc: Spinlock::new(LineDiscipline::new(WinSize::default())),
            locked: AtomicBool::new(true), // Locked by default until unlocked via TIOCSPTLCK
            slave_open_count: AtomicUsize::new(0),
            master_open: AtomicBool::new(true),
        }
    }
}

/// Global PTY manager tracking active master/slave pairs.
pub struct PtyManager {
    pairs: Spinlock<BTreeMap<u32, Arc<PtyPair>>>,
    next_id: AtomicUsize,
}

impl PtyManager {
    pub const fn new() -> Self {
        Self {
            pairs: Spinlock::new(BTreeMap::new()),
            next_id: AtomicUsize::new(0),
        }
    }

    /// Allocate a new PTY pair and register slave at `/dev/pts/N`.
    pub fn allocate_pair(&self) -> Result<Arc<PtyPair>, VfsError> {
        let mut pairs_guard = self.pairs.lock();
        let id = self.next_id.fetch_add(1, Ordering::SeqCst) as u32;

        let pair = Arc::new(PtyPair::new(id));
        pairs_guard.insert(id, pair.clone());

        // Register /dev/pts/N inode in devfs
        register_pts_node(id, pair.clone())?;

        Ok(pair)
    }

    /// Retrieve an existing PTY pair by ID.
    pub fn get_pair(&self, id: u32) -> Option<Arc<PtyPair>> {
        self.pairs.lock().get(&id).cloned()
    }

    /// Remove a closed PTY pair.
    pub fn remove_pair(&self, id: u32) {
        self.pairs.lock().remove(&id);
    }
}

pub static PTY_MANAGER: PtyManager = PtyManager::new();

/// Helper to register a dynamic `/dev/pts/N` inode in devfs.
fn register_pts_node(id: u32, pair: Arc<PtyPair>) -> Result<(), VfsError> {
    let mt = MOUNT_TABLE.read();
    if let Some((mount, _)) = mt.lookup("/dev") {
        let pts_name = format!("{}", id);
        let pts_ino = mount.superblock.alloc_ino();
        let pts_inode = Arc::new(Inode {
            ino: pts_ino,
            inode_type: InodeType::CharDevice,
            ops: Arc::new(PtsInode { pair }),
        });
        drop(mt);
        crate::fs::devfs::register_pts_node(&pts_name, pts_inode);
    }
    Ok(())
}

// -----------------------------------------------------------------------------
// PTMX (/dev/ptmx) Inode & FileOps
// -----------------------------------------------------------------------------

pub struct PtmxInode;

impl InodeOps for PtmxInode {
    fn open(&self) -> Result<Arc<dyn FileOps>, VfsError> {
        let pair = PTY_MANAGER.allocate_pair()?;
        Ok(Arc::new(PtyMasterFileOps { pair }))
    }

    fn stat(&self) -> Result<Stat, VfsError> {
        Ok(Stat {
            mode: 0o020666, // S_IFCHR | 0666
            nlink: 1,
            ..Default::default()
        })
    }
}

pub struct PtyMasterFileOps {
    pub pair: Arc<PtyPair>,
}

impl FileOps for PtyMasterFileOps {
    fn read(&self, _offset: usize, buf: &mut [u8]) -> Result<usize, VfsError> {
        if buf.is_empty() {
            return Ok(0);
        }
        loop {
            let mut mb = self.pair.master_buffer.lock();
            if !mb.is_empty() {
                let mut count = 0;
                while count < buf.len() {
                    if let Some(byte) = mb.pop_front() {
                        buf[count] = byte;
                        count += 1;
                    } else {
                        break;
                    }
                }
                return Ok(count);
            }
            if self.pair.slave_open_count.load(Ordering::SeqCst) == 0 {
                return Ok(0);
            }
            drop(mb);
            #[cfg(target_arch = "x86_64")]
            x86_64::instructions::interrupts::enable_and_hlt();
            #[cfg(not(target_arch = "x86_64"))]
            crate::proc::thread::Thread::yield_cpu();
        }
    }

    fn write(&self, _offset: usize, buf: &[u8]) -> Result<usize, VfsError> {
        if buf.is_empty() {
            return Ok(0);
        }
        let mut ldisc = self.pair.slave_ldisc.lock();
        let mut master_buf = self.pair.master_buffer.lock();

        for &byte in buf {
            let echo = ldisc.accept_input_byte(byte);
            for b in echo {
                master_buf.push_back(b);
            }
        }
        Ok(buf.len())
    }

    fn ioctl(&self, cmd: u64, arg: usize) -> Result<usize, VfsError> {
        match cmd {
            TIOCGPTN => {
                if !crate::syscalls::is_user_ptr_valid(arg as u64, 4) {
                    return Err(VfsError::InvalidInput);
                }
                // SAFETY: Pointer is user space verified.
                unsafe {
                    *(arg as *mut u32) = self.pair.id;
                }
                Ok(0)
            }
            TIOCSPTLCK => {
                if !crate::syscalls::is_user_ptr_valid(arg as u64, 4) {
                    return Err(VfsError::InvalidInput);
                }
                // SAFETY: Pointer is user space verified.
                let lock_val = unsafe { *(arg as *const i32) };
                self.pair.locked.store(lock_val != 0, Ordering::SeqCst);
                Ok(0)
            }
            TIOCGWINSZ => {
                if !crate::syscalls::is_user_ptr_valid(
                    arg as u64,
                    core::mem::size_of::<WinSize>(),
                ) {
                    return Err(VfsError::InvalidInput);
                }
                let ws = self.pair.slave_ldisc.lock().winsize;
                // SAFETY: Pointer is user space verified.
                unsafe {
                    *(arg as *mut WinSize) = ws;
                }
                Ok(0)
            }
            TIOCSWINSZ => {
                if !crate::syscalls::is_user_ptr_valid(
                    arg as u64,
                    core::mem::size_of::<WinSize>(),
                ) {
                    return Err(VfsError::InvalidInput);
                }
                // SAFETY: Pointer is user space verified.
                let ws = unsafe { *(arg as *const WinSize) };
                let mut ldisc = self.pair.slave_ldisc.lock();
                ldisc.winsize = ws;
                if ldisc.foreground_pgid > 0 {
                    let _ = crate::ipc::signal::send_signal_to_process_group(
                        ldisc.foreground_pgid,
                        crate::ipc::signal::SIGWINCH,
                    );
                }
                Ok(0)
            }
            FIONREAD => {
                if !crate::syscalls::is_user_ptr_valid(arg as u64, 4) {
                    return Err(VfsError::InvalidInput);
                }
                let len = self.pair.master_buffer.lock().len() as i32;
                // SAFETY: Pointer is user space verified.
                unsafe {
                    *(arg as *mut i32) = len;
                }
                Ok(0)
            }
            _ => Err(VfsError::NotSupported),
        }
    }

    fn isatty(&self) -> bool {
        true
    }
}

// -----------------------------------------------------------------------------
// PTS (/dev/pts/N) Inode & FileOps
// -----------------------------------------------------------------------------

pub struct PtsInode {
    pub pair: Arc<PtyPair>,
}

impl InodeOps for PtsInode {
    fn open(&self) -> Result<Arc<dyn FileOps>, VfsError> {
        if self.pair.locked.load(Ordering::SeqCst) {
            return Err(VfsError::PermissionDenied);
        }
        self.pair.slave_open_count.fetch_add(1, Ordering::SeqCst);
        Ok(Arc::new(PtySlaveFileOps {
            pair: self.pair.clone(),
        }))
    }

    fn stat(&self) -> Result<Stat, VfsError> {
        Ok(Stat {
            mode: 0o020620, // S_IFCHR | 0620
            nlink: 1,
            ..Default::default()
        })
    }
}

pub struct PtySlaveFileOps {
    pub pair: Arc<PtyPair>,
}

impl FileOps for PtySlaveFileOps {
    fn read(&self, _offset: usize, buf: &mut [u8]) -> Result<usize, VfsError> {
        if buf.is_empty() {
            return Ok(0);
        }
        loop {
            let mut ldisc = self.pair.slave_ldisc.lock();
            let bytes_read = ldisc.read_bytes(buf);
            if bytes_read > 0 {
                return Ok(bytes_read);
            }
            if !self.pair.master_open.load(Ordering::SeqCst) {
                return Ok(0); // Master closed (EOF)
            }
            drop(ldisc);
            #[cfg(target_arch = "x86_64")]
            x86_64::instructions::interrupts::enable_and_hlt();
            #[cfg(not(target_arch = "x86_64"))]
            crate::proc::thread::Thread::yield_cpu();
        }
    }

    fn write(&self, _offset: usize, buf: &[u8]) -> Result<usize, VfsError> {
        if buf.is_empty() {
            return Ok(0);
        }
        let ldisc = self.pair.slave_ldisc.lock();
        let processed = ldisc.process_output_bytes(buf);
        drop(ldisc);

        let mut mb = self.pair.master_buffer.lock();
        for byte in processed {
            mb.push_back(byte);
        }
        Ok(buf.len())
    }

    fn ioctl(&self, cmd: u64, arg: usize) -> Result<usize, VfsError> {
        match cmd {
            TCGETS => {
                if !crate::syscalls::is_user_ptr_valid(
                    arg as u64,
                    core::mem::size_of::<Termios>(),
                ) {
                    return Err(VfsError::InvalidInput);
                }
                let t = self.pair.slave_ldisc.lock().termios;
                // SAFETY: Pointer is user space verified.
                unsafe {
                    *(arg as *mut Termios) = t;
                }
                Ok(0)
            }
            TCSETS | TCSETSW | TCSETSF => {
                if !crate::syscalls::is_user_ptr_valid(
                    arg as u64,
                    core::mem::size_of::<Termios>(),
                ) {
                    return Err(VfsError::InvalidInput);
                }
                // SAFETY: Pointer is user space verified.
                let t = unsafe { *(arg as *const Termios) };
                self.pair.slave_ldisc.lock().termios = t;
                Ok(0)
            }
            TIOCGWINSZ => {
                if !crate::syscalls::is_user_ptr_valid(
                    arg as u64,
                    core::mem::size_of::<WinSize>(),
                ) {
                    return Err(VfsError::InvalidInput);
                }
                let ws = self.pair.slave_ldisc.lock().winsize;
                // SAFETY: Pointer is user space verified.
                unsafe {
                    *(arg as *mut WinSize) = ws;
                }
                Ok(0)
            }
            TIOCSWINSZ => {
                if !crate::syscalls::is_user_ptr_valid(
                    arg as u64,
                    core::mem::size_of::<WinSize>(),
                ) {
                    return Err(VfsError::InvalidInput);
                }
                // SAFETY: Pointer is user space verified.
                let ws = unsafe { *(arg as *const WinSize) };
                let mut ldisc = self.pair.slave_ldisc.lock();
                ldisc.winsize = ws;
                if ldisc.foreground_pgid > 0 {
                    let _ = crate::ipc::signal::send_signal_to_process_group(
                        ldisc.foreground_pgid,
                        crate::ipc::signal::SIGWINCH,
                    );
                }
                Ok(0)
            }
            TIOCGPGRP => {
                if !crate::syscalls::is_user_ptr_valid(arg as u64, 4) {
                    return Err(VfsError::InvalidInput);
                }
                let pgid = self.pair.slave_ldisc.lock().foreground_pgid;
                // SAFETY: Pointer is user space verified.
                unsafe {
                    *(arg as *mut i32) = pgid;
                }
                Ok(0)
            }
            TIOCSPGRP => {
                if !crate::syscalls::is_user_ptr_valid(arg as u64, 4) {
                    return Err(VfsError::InvalidInput);
                }
                // SAFETY: Pointer is user space verified.
                let pgid = unsafe { *(arg as *const i32) };
                self.pair.slave_ldisc.lock().foreground_pgid = pgid;
                Ok(0)
            }
            TIOCSCTTY => Ok(0),
            TIOCNOTTY => Ok(0),
            FIONREAD => {
                if !crate::syscalls::is_user_ptr_valid(arg as u64, 4) {
                    return Err(VfsError::InvalidInput);
                }
                let len = self.pair.slave_ldisc.lock().available_read_bytes() as i32;
                // SAFETY: Pointer is user space verified.
                unsafe {
                    *(arg as *mut i32) = len;
                }
                Ok(0)
            }
            _ => Err(VfsError::NotSupported),
        }
    }

    fn isatty(&self) -> bool {
        true
    }
}
