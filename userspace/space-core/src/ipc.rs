//! Zero-copy shared memory surfaces (`mmap`) and lock-free atomic ring buffers.
//!
//! Architecture:
//! - Clients connect to `/run/space-compositor.sock` (AF_UNIX) only for initial
//!   handshake and registration of their shared-memory surface & ring IDs.
//! - All frame pixel data is written directly into `mmap`'d `/dev/shm` pages
//!   (zero-copy between client and compositor).
//! - All high-frequency messages (damage commits, input events, resize notices)
//!   flow over cache-line-aligned, lock-free SPSC atomic ring buffers.

use crate::geometry::Rect;
use crate::sys;
use std::ffi::CString;
use std::io;
use std::os::raw::{c_int, c_void};
use std::ptr;
use std::sync::atomic::{AtomicU32, Ordering};

pub const COMPOSITOR_SOCKET: &str = "/run/space-compositor.sock";
pub const RING_CAPACITY: usize = 256;

/// Lock-free message types exchanged over `ShmRing`.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MsgKind {
    Nop = 0,
    /// Client -> Compositor: damage region committed in `ShmSurface`.
    CommitDamage = 1,
    /// Client -> Compositor: request window close.
    CloseRequest = 2,
    /// Client -> Compositor: request window move/resize.
    SetTitleHint = 3,
    /// Compositor -> Client: key press/release (`a` = keycode, `b` = pressed, `c` = char/mods).
    KeyEvent = 10,
    /// Compositor -> Client: pointer move/button (`a` = x, `b` = y, `c` = button_mask, `d` = scroll).
    PointerEvent = 11,
    /// Compositor -> Client: window resized (`a` = width, `b` = height).
    ResizeEvent = 12,
    /// Compositor -> Client: focus changed (`a` = focused 1/0).
    FocusEvent = 13,
    /// Compositor -> Client: compositor requests graceful exit.
    CloseEvent = 14,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct RingMsg {
    pub kind: u32,
    pub a: i32,
    pub b: i32,
    pub c: i32,
    pub d: i32,
}

impl RingMsg {
    pub const fn new(kind: MsgKind, a: i32, b: i32, c: i32, d: i32) -> Self {
        Self {
            kind: kind as u32,
            a,
            b,
            c,
            d,
        }
    }
}

/// Cache-line-aligned lock-free SPSC ring buffer header in `mmap`'d shared memory.
#[repr(C, align(64))]
pub struct RingHeader {
    pub head: AtomicU32,
    _pad1: [u8; 60],
    pub tail: AtomicU32,
    _pad2: [u8; 60],
    pub capacity: u32,
    _pad3: [u8; 60],
}

#[repr(C)]
pub struct RingMemory {
    pub header: RingHeader,
    pub slots: [RingMsg; RING_CAPACITY],
}

/// Abstraction for IPC message transport (Dependency Inversion Principle).
pub trait IpcTransport {
    fn try_push(&self, msg: RingMsg) -> bool;
    fn try_pop(&self) -> Option<RingMsg>;
}

/// Mapped lock-free SPSC ring buffer over `/dev/shm`.
pub struct ShmRing {
    ptr: *mut RingMemory,
    size: usize,
    fd: c_int,
    path: String,
    owner: bool,
}

unsafe impl Send for ShmRing {}
unsafe impl Sync for ShmRing {}

impl ShmRing {
    pub fn create(name: &str) -> io::Result<Self> {
        let path = format!("/dev/shm/{}", name);
        let size = std::mem::size_of::<RingMemory>();
        let fd = sys::open_path(&path, sys::O_RDWR | sys::O_CREAT | sys::O_CLOEXEC, 0o600)?;
        if unsafe { sys::ftruncate(fd, size as i64) } < 0 {
            let err = io::Error::last_os_error();
            unsafe { sys::close(fd) };
            return Err(err);
        }
        let ptr = unsafe {
            sys::mmap(
                ptr::null_mut(),
                size,
                sys::PROT_READ | sys::PROT_WRITE,
                sys::MAP_SHARED,
                fd,
                0,
            )
        };
        if ptr == sys::MAP_FAILED {
            let err = io::Error::last_os_error();
            unsafe { sys::close(fd) };
            return Err(err);
        }
        let mem = ptr as *mut RingMemory;
        unsafe {
            ptr::write_bytes(mem, 0, 1);
            (*mem).header.capacity = RING_CAPACITY as u32;
        }
        Ok(Self {
            ptr: mem,
            size,
            fd,
            path,
            owner: true,
        })
    }

    pub fn open(name: &str) -> io::Result<Self> {
        let path = format!("/dev/shm/{}", name);
        let size = std::mem::size_of::<RingMemory>();
        let fd = sys::open_path(&path, sys::O_RDWR | sys::O_CLOEXEC, 0)?;
        let ptr = unsafe {
            sys::mmap(
                ptr::null_mut(),
                size,
                sys::PROT_READ | sys::PROT_WRITE,
                sys::MAP_SHARED,
                fd,
                0,
            )
        };
        if ptr == sys::MAP_FAILED {
            let err = io::Error::last_os_error();
            unsafe { sys::close(fd) };
            return Err(err);
        }
        Ok(Self {
            ptr: ptr as *mut RingMemory,
            size,
            fd,
            path,
            owner: false,
        })
    }

    #[inline]
    fn mem(&self) -> &RingMemory {
        unsafe { &*self.ptr }
    }

    #[inline]
    fn mem_mut(&self) -> &mut RingMemory {
        unsafe { &mut *self.ptr }
    }
}

impl IpcTransport for ShmRing {
    #[inline]
    fn try_push(&self, msg: RingMsg) -> bool {
        let hdr = &self.mem().header;
        let cap = hdr.capacity.max(1) as usize;
        let head = hdr.head.load(Ordering::Relaxed) as usize;
        let tail = hdr.tail.load(Ordering::Acquire) as usize;
        let next = (head + 1) % cap;
        if next == tail {
            return false;
        }
        self.mem_mut().slots[head] = msg;
        hdr.head.store(next as u32, Ordering::Release);
        true
    }

    #[inline]
    fn try_pop(&self) -> Option<RingMsg> {
        let hdr = &self.mem().header;
        let cap = hdr.capacity.max(1) as usize;
        let tail = hdr.tail.load(Ordering::Relaxed) as usize;
        let head = hdr.head.load(Ordering::Acquire) as usize;
        if tail == head {
            return None;
        }
        let msg = self.mem().slots[tail];
        let next = (tail + 1) % cap;
        hdr.tail.store(next as u32, Ordering::Release);
        Some(msg)
    }
}

impl Drop for ShmRing {
    fn drop(&mut self) {
        unsafe {
            sys::munmap(self.ptr as *mut c_void, self.size);
            sys::close(self.fd);
            if self.owner {
                if let Ok(c) = CString::new(self.path.as_str()) {
                    sys::unlink(c.as_ptr());
                }
            }
        }
    }
}

/// Zero-copy shared-memory pixel surface header.
#[repr(C, align(64))]
pub struct SurfaceHeader {
    pub width: AtomicU32,
    pub height: AtomicU32,
    pub stride: AtomicU32,
    pub seq: AtomicU32,
    pub damage_x: AtomicU32,
    pub damage_y: AtomicU32,
    pub damage_w: AtomicU32,
    pub damage_h: AtomicU32,
}

/// Zero-copy `mmap`'d ARGB8888 surface shared between client and compositor.
pub struct ShmSurface {
    ptr: *mut u8,
    map_size: usize,
    fd: c_int,
    path: String,
    owner: bool,
}

unsafe impl Send for ShmSurface {}
unsafe impl Sync for ShmSurface {}

impl ShmSurface {
    pub fn create(name: &str, width: u32, height: u32) -> io::Result<Self> {
        let path = format!("/dev/shm/{}", name);
        let hdr_size = std::mem::size_of::<SurfaceHeader>();
        let px_bytes = width as usize * height as usize * 4;
        let map_size = hdr_size + px_bytes;
        let fd = sys::open_path(&path, sys::O_RDWR | sys::O_CREAT | sys::O_CLOEXEC, 0o600)?;
        if unsafe { sys::ftruncate(fd, map_size as i64) } < 0 {
            let err = io::Error::last_os_error();
            unsafe { sys::close(fd) };
            return Err(err);
        }
        let ptr = unsafe {
            sys::mmap(
                ptr::null_mut(),
                map_size,
                sys::PROT_READ | sys::PROT_WRITE,
                sys::MAP_SHARED,
                fd,
                0,
            )
        };
        if ptr == sys::MAP_FAILED {
            let err = io::Error::last_os_error();
            unsafe { sys::close(fd) };
            return Err(err);
        }
        let surf = Self {
            ptr: ptr as *mut u8,
            map_size,
            fd,
            path,
            owner: true,
        };
        let hdr = surf.header();
        hdr.width.store(width, Ordering::Relaxed);
        hdr.height.store(height, Ordering::Relaxed);
        hdr.stride.store(width, Ordering::Relaxed);
        hdr.seq.store(0, Ordering::Release);
        Ok(surf)
    }

    pub fn open(name: &str, width: u32, height: u32) -> io::Result<Self> {
        let path = format!("/dev/shm/{}", name);
        let hdr_size = std::mem::size_of::<SurfaceHeader>();
        let map_size = hdr_size + width as usize * height as usize * 4;
        let fd = sys::open_path(&path, sys::O_RDWR | sys::O_CLOEXEC, 0)?;
        let ptr = unsafe {
            sys::mmap(
                ptr::null_mut(),
                map_size,
                sys::PROT_READ | sys::PROT_WRITE,
                sys::MAP_SHARED,
                fd,
                0,
            )
        };
        if ptr == sys::MAP_FAILED {
            let err = io::Error::last_os_error();
            unsafe { sys::close(fd) };
            return Err(err);
        }
        Ok(Self {
            ptr: ptr as *mut u8,
            map_size,
            fd,
            path,
            owner: false,
        })
    }

    #[inline]
    pub fn header(&self) -> &SurfaceHeader {
        unsafe { &*(self.ptr as *const SurfaceHeader) }
    }

    #[inline]
    pub fn pixels(&self) -> &[u32] {
        let hdr = self.header();
        let w = hdr.width.load(Ordering::Acquire) as usize;
        let h = hdr.height.load(Ordering::Acquire) as usize;
        let max_px = (self.map_size.saturating_sub(std::mem::size_of::<SurfaceHeader>())) / 4;
        let len = (w * h).min(max_px);
        unsafe {
            let px_ptr = self.ptr.add(std::mem::size_of::<SurfaceHeader>()) as *const u32;
            std::slice::from_raw_parts(px_ptr, len)
        }
    }

    #[inline]
    pub fn pixels_mut(&mut self) -> &mut [u32] {
        let hdr = self.header();
        let w = hdr.width.load(Ordering::Acquire) as usize;
        let h = hdr.height.load(Ordering::Acquire) as usize;
        let max_px = (self.map_size.saturating_sub(std::mem::size_of::<SurfaceHeader>())) / 4;
        let len = (w * h).min(max_px);
        unsafe {
            let px_ptr = self.ptr.add(std::mem::size_of::<SurfaceHeader>()) as *mut u32;
            std::slice::from_raw_parts_mut(px_ptr, len)
        }
    }

    pub fn mark_damaged(&self, rect: Rect) {
        let hdr = self.header();
        hdr.damage_x.store(rect.x.max(0) as u32, Ordering::Relaxed);
        hdr.damage_y.store(rect.y.max(0) as u32, Ordering::Relaxed);
        hdr.damage_w.store(rect.w.max(0) as u32, Ordering::Relaxed);
        hdr.damage_h.store(rect.h.max(0) as u32, Ordering::Relaxed);
        hdr.seq.fetch_add(1, Ordering::Release);
    }
}

impl Drop for ShmSurface {
    fn drop(&mut self) {
        unsafe {
            sys::munmap(self.ptr as *mut c_void, self.map_size);
            sys::close(self.fd);
            if self.owner {
                if let Ok(c) = CString::new(self.path.as_str()) {
                    sys::unlink(c.as_ptr());
                }
            }
        }
    }
}

/// Handshake packet sent over `/run/space-compositor.sock` on client connect.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct HandshakePacket {
    pub magic: u32,
    pub pid: i32,
    pub width: u32,
    pub height: u32,
    pub title: [u8; 64],
    pub shm_prefix: [u8; 48],
}

impl HandshakePacket {
    pub const MAGIC: u32 = 0x5350_4143; // "SPAC"

    pub fn new(pid: i32, width: u32, height: u32, title_str: &str, prefix: &str) -> Self {
        let mut title = [0u8; 64];
        let tb = title_str.as_bytes();
        let tlen = tb.len().min(63);
        title[..tlen].copy_from_slice(&tb[..tlen]);

        let mut shm_prefix = [0u8; 48];
        let pb = prefix.as_bytes();
        let plen = pb.len().min(47);
        shm_prefix[..plen].copy_from_slice(&pb[..plen]);

        Self {
            magic: Self::MAGIC,
            pid,
            width,
            height,
            title,
            shm_prefix,
        }
    }

    pub fn title_str(&self) -> String {
        let end = self.title.iter().position(|&b| b == 0).unwrap_or(self.title.len());
        String::from_utf8_lossy(&self.title[..end]).into_owned()
    }

    pub fn prefix_str(&self) -> String {
        let end = self
            .shm_prefix
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(self.shm_prefix.len());
        String::from_utf8_lossy(&self.shm_prefix[..end]).into_owned()
    }
}
