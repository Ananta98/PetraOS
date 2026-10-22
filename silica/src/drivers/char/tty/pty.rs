// SPDX-License-Identifier: GPL-2.0

//! Pseudo-Terminal (PTY) Subsystem.

use alloc::{
    boxed::Box,
    collections::{btree_map::BTreeMap, vec_deque::VecDeque},
    format,
    string::String,
    sync::{Arc, Weak},
    vec::Vec,
};
use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicUsize, Ordering};
use ostd::sync::WaitQueue;
use spin::Mutex;

use super::{Tty, TtyDriver};
use crate::{
    drivers::{CharacterDevice, register_chrdev},
    fs::poll::PollEvents,
    fs::vfs::{FileOps, INode, Mode, NodeOps, OpenFlags},
    uapi::errno::{Errno, Result},
    uapi::ioctl,
    uapi::signal::Signal,
    uapi::termios::winsize,
    utils::userspace::user_ptr::UserPtr,
};

// PTY ioctl constants (Linux standard)
pub const TIOCGPTN: u32 = 0x80045430;
pub const TIOCSPTLCK: u32 = 0x40045431;
pub const TIOCGPTLCK: u32 = 0x80045439;
pub const TIOCPKT: u32 = 0x5420;
pub const TIOCGPKT: u32 = 0x80045438;
pub const TIOCSIG: u32 = 0x40045436;

pub const TIOCPKT_DATA: u8 = 0x00;
pub const TIOCPKT_FLUSHREAD: u8 = 0x01;
pub const TIOCPKT_FLUSHWRITE: u8 = 0x02;
pub const TIOCPKT_STOP: u8 = 0x04;
pub const TIOCPKT_START: u8 = 0x08;
pub const TIOCPKT_NOSTOP: u8 = 0x10;
pub const TIOCPKT_DOSTOP: u8 = 0x20;
pub const TIOCPKT_IOCTL: u8 = 0x40;

const MASTER_BUF_SIZE: usize = 4096;

static PTY_INDEX: AtomicU32 = AtomicU32::new(0);
static PTY_TABLE: Mutex<BTreeMap<u32, Arc<PtyPair>>> = Mutex::new(BTreeMap::new());

/// State of an active master/slave PTY pair.
pub struct PtyPair {
    pub index: u32,
    pub tty: Arc<Tty>,
    pub master_buf: Mutex<VecDeque<u8>>,
    pub master_wq: WaitQueue,
    pub unlocked: AtomicBool,
    pub slave_count: AtomicUsize,
    pub packet: AtomicBool,
    pub packet_flags: AtomicU8,
}

impl PtyPair {
    pub fn has_open_slave(&self) -> bool {
        self.slave_count.load(Ordering::Acquire) != 0
    }
}

/// Driver for slave TTY writes: puts data into master buffer and wakes master.
struct PtySlaveTtyDriver {
    pair: Weak<PtyPair>,
}

impl TtyDriver for PtySlaveTtyDriver {
    fn write_output(&self, data: &[u8]) -> Result<()> {
        let Some(pair) = self.pair.upgrade() else {
            return Ok(());
        };

        let mut mbuf = pair.master_buf.lock();
        for &b in data {
            if mbuf.len() < MASTER_BUF_SIZE {
                mbuf.push_back(b);
            }
        }
        drop(mbuf);

        pair.master_wq.wake_all();
        Ok(())
    }

    fn get_winsize(&self) -> winsize {
        winsize {
            ws_row: 25,
            ws_col: 80,
            ws_xpixel: 0,
            ws_ypixel: 0,
        }
    }
}

/// Allocates a new pseudo-terminal pair (/dev/ptmx master and /dev/pts/N slave).
pub fn alloc_pty() -> Result<Arc<PtyPair>> {
    let index = PTY_INDEX.fetch_add(1, Ordering::Relaxed);
    let name = format!("pts/{}", index);

    let pair = Arc::new_cyclic(|weak: &Weak<PtyPair>| {
        let driver = Arc::new(PtySlaveTtyDriver { pair: weak.clone() });
        let tty = Tty::new(name, driver);
        PtyPair {
            index,
            tty,
            master_buf: Mutex::new(VecDeque::with_capacity(MASTER_BUF_SIZE)),
            master_wq: WaitQueue::new(),
            unlocked: AtomicBool::new(false),
            slave_count: AtomicUsize::new(0),
            packet: AtomicBool::new(false),
            packet_flags: AtomicU8::new(0),
        }
    });

    PTY_TABLE.lock().insert(index, Arc::clone(&pair));
    Ok(pair)
}

pub fn get_pty_pair(index: u32) -> Option<Arc<PtyPair>> {
    PTY_TABLE.lock().get(&index).cloned()
}

/// Master side file operations for /dev/ptmx.
pub struct PtyMasterFileOps {
    pub pair: Arc<PtyPair>,
}

impl PtyMasterFileOps {
    pub fn new(pair: Arc<PtyPair>) -> Self {
        Self { pair }
    }
}

impl FileOps for PtyMasterFileOps {
    fn read(&self, _offset: u64, buf: &mut [u8]) -> Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }

        let try_read = || -> Option<Result<usize>> {
            // Packet mode control byte has priority
            if self.pair.packet.load(Ordering::Relaxed) {
                let pending = self.pair.packet_flags.swap(0, Ordering::AcqRel);
                if pending != 0 {
                    buf[0] = pending;
                    return Some(Ok(1));
                }
            }

            let mut mbuf = self.pair.master_buf.lock();
            if !mbuf.is_empty() {
                if self.pair.packet.load(Ordering::Relaxed) {
                    buf[0] = TIOCPKT_DATA;
                    let to_read = (buf.len() - 1).min(mbuf.len());
                    for i in 0..to_read {
                        buf[1 + i] = mbuf.pop_front().unwrap();
                    }
                    return Some(Ok(1 + to_read));
                } else {
                    let to_read = buf.len().min(mbuf.len());
                    for i in 0..to_read {
                        buf[i] = mbuf.pop_front().unwrap();
                    }
                    return Some(Ok(to_read));
                }
            }

            if !self.pair.has_open_slave() {
                return Some(Err(Errno::EIO));
            }

            None
        };

        self.pair.master_wq.wait_until(try_read)
    }

    fn write(&self, _offset: u64, buf: &[u8]) -> Result<usize> {
        for &byte in buf {
            let echo = self.pair.tty.input_byte_internal(byte);
            if !echo.is_empty() {
                let mut mbuf = self.pair.master_buf.lock();
                for b in echo {
                    if mbuf.len() < MASTER_BUF_SIZE {
                        mbuf.push_back(b);
                    }
                }
                drop(mbuf);
                self.pair.master_wq.wake_all();
            }
        }
        Ok(buf.len())
    }

    fn ioctl(&self, cmd: u32, arg: usize) -> Result<usize> {
        let proc = crate::proc::thread::Thread::proc_or_esrch()?;
        let vmar = proc.vmspace();

        match cmd {
            TIOCGPTN => {
                let index = self.pair.index as i32;
                UserPtr::new(arg).write_val(&vmar, index)?;
                Ok(0)
            }
            TIOCSPTLCK => {
                let lock: i32 = UserPtr::new(arg).read_val(&vmar)?;
                self.pair.unlocked.store(lock == 0, Ordering::Release);
                Ok(0)
            }
            TIOCGPTLCK => {
                let locked: i32 = if self.pair.unlocked.load(Ordering::Acquire) {
                    0
                } else {
                    1
                };
                UserPtr::new(arg).write_val(&vmar, locked)?;
                Ok(0)
            }
            TIOCPKT => {
                let enable: i32 = UserPtr::new(arg).read_val(&vmar)?;
                self.pair.packet.store(enable != 0, Ordering::Release);
                if enable == 0 {
                    self.pair.packet_flags.store(0, Ordering::Release);
                }
                Ok(0)
            }
            TIOCGPKT => {
                let val: i32 = self.pair.packet.load(Ordering::Acquire) as i32;
                UserPtr::new(arg).write_val(&vmar, val)?;
                Ok(0)
            }
            TIOCSIG => {
                let signum: i32 = UserPtr::new(arg).read_val(&vmar)?;
                if let Some(sig) = Signal::from_u32(signum as u32) {
                    self.pair.tty.signal_foreground(sig);
                }
                Ok(0)
            }
            ioctl::FIONREAD => {
                let avail = self.pair.master_buf.lock().len() as i32;
                UserPtr::new(arg).write_val(&vmar, avail)?;
                Ok(0)
            }
            _ => self.pair.tty.ioctl(cmd, arg),
        }
    }

    fn poll(&self) -> PollEvents {
        let mut revents = PollEvents::empty();
        let has_data = !self.pair.master_buf.lock().is_empty();
        let has_packet = self.pair.packet.load(Ordering::Relaxed)
            && self.pair.packet_flags.load(Ordering::Relaxed) != 0;

        if has_data || has_packet {
            revents |= PollEvents::IN | PollEvents::RDNORM;
        }
        if self.pair.has_open_slave() {
            revents |= PollEvents::OUT | PollEvents::WRNORM;
        } else {
            revents |= PollEvents::HUP;
        }
        revents
    }
}

impl Drop for PtyMasterFileOps {
    fn drop(&mut self) {
        self.pair.tty.hangup();
        self.pair.master_wq.wake_all();
        PTY_TABLE.lock().remove(&self.pair.index);
    }
}

/// Node operations for /dev/ptmx.
pub struct PtmxNodeOps;

impl NodeOps for PtmxNodeOps {
    fn open(&self, _flags: OpenFlags) -> Result<Box<dyn FileOps>> {
        let pair = alloc_pty()?;
        Ok(Box::new(PtyMasterFileOps::new(pair)))
    }
}

/// Character device for /dev/ptmx.
pub struct PtmxDevice;

impl CharacterDevice for PtmxDevice {
    fn name(&self) -> String {
        String::from("ptmx")
    }

    fn read(&self, _buf: &mut [u8]) -> Result<usize> {
        crate::return_errno!(EPERM, "open /dev/ptmx to allocate a pty master")
    }

    fn write(&self, _buf: &[u8]) -> Result<usize> {
        crate::return_errno!(EPERM, "open /dev/ptmx to allocate a pty master")
    }
}

/// Slave file operations for /dev/pts/N.
pub struct PtySlaveFileOps {
    pair: Arc<PtyPair>,
}

impl Drop for PtySlaveFileOps {
    fn drop(&mut self) {
        self.pair.slave_count.fetch_sub(1, Ordering::AcqRel);
        self.pair.master_wq.wake_all();
    }
}

impl FileOps for PtySlaveFileOps {
    fn read(&self, _offset: u64, buf: &mut [u8]) -> Result<usize> {
        self.pair.tty.read(buf)
    }

    fn write(&self, _offset: u64, buf: &[u8]) -> Result<usize> {
        self.pair.tty.write(buf)
    }

    fn ioctl(&self, cmd: u32, arg: usize) -> Result<usize> {
        self.pair.tty.ioctl(cmd, arg)
    }

    fn poll(&self) -> PollEvents {
        match self.pair.tty.poll() {
            Ok(mask) => PollEvents::from_bits_truncate(mask as u16),
            Err(_) => PollEvents::empty(),
        }
    }
}

/// Slave node operations for /dev/pts/N.
pub struct PtySlaveNodeOps {
    pair: Arc<PtyPair>,
}

impl NodeOps for PtySlaveNodeOps {
    fn open(&self, _flags: OpenFlags) -> Result<Box<dyn FileOps>> {
        if !self.pair.unlocked.load(Ordering::Acquire) {
            return Err(Errno::EIO);
        }
        self.pair.slave_count.fetch_add(1, Ordering::AcqRel);
        Ok(Box::new(PtySlaveFileOps {
            pair: Arc::clone(&self.pair),
        }))
    }
}

/// Directory node operations for /dev/pts.
pub struct PtsDirNodeOps;

impl NodeOps for PtsDirNodeOps {
    fn lookup(&self, name: &str) -> Result<Arc<INode>> {
        let index = name.parse::<u32>().map_err(|_| Errno::ENOENT)?;
        let pair = get_pty_pair(index).ok_or(Errno::ENOENT)?;

        Ok(Arc::new(INode::new(
            Box::new(PtySlaveNodeOps { pair }),
            Mode::CHAR
                | Mode::RUSR
                | Mode::WUSR
                | Mode::RGRP
                | Mode::WGRP
                | Mode::ROTH
                | Mode::WOTH,
            1000 + index as usize,
        )))
    }

    fn open(&self, _flags: OpenFlags) -> Result<Box<dyn FileOps>> {
        Ok(Box::new(PtsDirFileOps))
    }
}

pub struct PtsDirFileOps;

impl FileOps for PtsDirFileOps {
    fn read(&self, _offset: u64, _buf: &mut [u8]) -> Result<usize> {
        crate::return_errno!(EISDIR, "cannot read from directory")
    }

    fn write(&self, _offset: u64, _buf: &[u8]) -> Result<usize> {
        crate::return_errno!(EISDIR, "cannot write to directory")
    }

    fn readdir(&self) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        out.extend_from_slice(b".\n..\n");
        let table = PTY_TABLE.lock();
        for &index in table.keys() {
            let entry = format!("{}\n", index);
            out.extend_from_slice(entry.as_bytes());
        }
        Ok(out)
    }
}

/// Registers the /dev/ptmx character device.
pub fn init() -> Result<()> {
    register_chrdev(Arc::new(PtmxDevice));
    Ok(())
}
