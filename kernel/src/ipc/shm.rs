//! System V Shared Memory IPC Subsystem
//!
//! Implements POSIX/Linux System V shared memory segment lifecycle and management:
//! - `shmget`: creates or retrieves a shared memory segment by `key_t` or `IPC_PRIVATE`.
//! - `shmat`: attaches a shared memory segment into the calling process address space.
//! - `shmdt`: detaches a shared memory segment from the calling process address space.
//! - `shmctl`: queries, updates, or removes (`IPC_RMID`) shared memory segments.
//!
//! Shared memory segments are backed by physical frames allocated from the Physical Memory
//! Manager (`PMM`). When attached to processes, these frames are mapped into process page tables
//! without Copy-On-Write (COW), enabling direct zero-copy shared inter-process communication.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicI32, Ordering};

use crate::arch::timer::hpet;
use crate::drivers::time::cmos_rtc;
use crate::ipc::semaphore::IpcPerm;
use crate::mm::ArchPageTable;
use crate::mm::vmm::paging::{PageTableFlags, PhysAddr, VirtAddr};
use crate::mm::vmm::vma::AddrSpace;
use crate::sync::spinlock::Spinlock;

// ── IPC flags / commands ─────────────────────────────────────────────────────

pub const IPC_PRIVATE: i32 = 0;
pub const IPC_CREAT: i32 = 0o1000;
pub const IPC_EXCL: i32 = 0o2000;
pub const IPC_RMID: i32 = 0;
pub const IPC_SET: i32 = 1;
pub const IPC_STAT: i32 = 2;
pub const IPC_INFO: i32 = 3;

pub const SHM_RDONLY: i32 = 0o10000;
pub const SHM_RND: i32 = 0o20000;
pub const SHM_REMAP: i32 = 0o40000;
pub const SHM_EXEC: i32 = 0o100000;

pub const SHM_LOCK: i32 = 11;
pub const SHM_UNLOCK: i32 = 12;
pub const SHM_STAT: i32 = 13;
pub const SHM_INFO: i32 = 14;

/// Lower boundary address alignment for shared memory attaches (4KB on x86_64).
pub const SHMLBA: u64 = 4096;

/// Minimum shared memory segment size in bytes.
pub const SHMMIN: usize = 1;
/// Maximum shared memory segment size in bytes (1 GB).
pub const SHMMAX: usize = 1024 * 1024 * 1024;
/// Maximum number of shared memory segments system-wide.
pub const SHMMNI: usize = 4096;
/// Maximum total shared memory in pages (1 million pages = 4 GB).
pub const SHMALL: usize = 1_048_576;

/// Global shared memory manager singleton.
pub static SHM_MANAGER: Spinlock<SharedMemoryManager> = Spinlock::new(SharedMemoryManager::new());

// ── Error type ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShmError {
    /// Invalid argument (e.g. unaligned address, invalid size or flags)
    InvalidArg,
    /// Shared memory identifier or key not found
    NotFound,
    /// Permission denied
    PermDenied,
    /// Shared memory segment already exists (IPC_CREAT | IPC_EXCL)
    AlreadyExists,
    /// Out of shared memory identifiers or system limit reached
    OutOfIds,
    /// Out of physical memory
    NoMem,
    /// Resource is busy or already attached
    InUse,
}

// ── ABI-compatible structures ─────────────────────────────────────────────────

/// Mirrors `struct shmid_ds` from userspace ABI (Linux x86_64).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct ShmidDs {
    pub shm_perm: IpcPerm,
    pub shm_segsz: usize,
    pub shm_atime: i64,
    pub shm_dtime: i64,
    pub shm_ctime: i64,
    pub shm_cpid: u32,
    pub shm_lpid: u32,
    pub shm_nattch: u64,
    _pad: [u64; 2],
}

/// Mirrors `struct shm_info` / `shminfo` for `IPC_INFO` / `SHM_INFO`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct ShmInfo {
    pub shmmax: usize,
    pub shmmin: usize,
    pub shmmni: usize,
    pub shmseg: usize,
    pub shmall: usize,
    _pad: [usize; 4],
}

// ── Shared memory segment ─────────────────────────────────────────────────────

/// State and backing physical memory of a shared memory segment.
pub struct ShmSegment {
    /// Unique shared memory identifier (`shmid`)
    pub id: i32,
    /// IPC key
    pub key: i32,
    /// Requested size in bytes
    pub size: usize,
    /// Permissions and creator metadata
    pub perm: IpcPerm,
    /// Pre-allocated physical frames backing this segment
    pub frames: Vec<PhysAddr>,
    /// Creator PID
    pub cpid: u32,
    /// Last attached/detached PID
    pub lpid: u32,
    /// Time of last attach
    pub atime: i64,
    /// Time of last detach
    pub dtime: i64,
    /// Time of last control status change
    pub ctime: i64,
    /// Number of active process attachments
    pub nattch: u64,
    /// Marked for destruction via `IPC_RMID`; freed once `nattch` reaches 0
    pub marked_for_destruction: bool,
}

impl ShmSegment {
    /// Return the `ShmidDs` snapshot of this segment.
    pub fn ds(&self) -> ShmidDs {
        ShmidDs {
            shm_perm: self.perm,
            shm_segsz: self.size,
            shm_atime: self.atime,
            shm_dtime: self.dtime,
            shm_ctime: self.ctime,
            shm_cpid: self.cpid,
            shm_lpid: self.lpid,
            shm_nattch: self.nattch,
            _pad: [0; 2],
        }
    }

    /// Check if the caller with `(uid, gid)` has required read / write permissions.
    pub fn check_perm(&self, uid: u32, gid: u32, req_write: bool) -> bool {
        if uid == 0 {
            return true; // root has full access
        }

        let mode = self.perm.mode;
        let mut granted_read = false;
        let mut granted_write = false;

        if uid == self.perm.uid || uid == self.perm.cuid {
            if (mode & 0o400) != 0 {
                granted_read = true;
            }
            if (mode & 0o200) != 0 {
                granted_write = true;
            }
        } else if gid == self.perm.gid || gid == self.perm.cgid {
            if (mode & 0o040) != 0 {
                granted_read = true;
            }
            if (mode & 0o020) != 0 {
                granted_write = true;
            }
        } else {
            if (mode & 0o004) != 0 {
                granted_read = true;
            }
            if (mode & 0o002) != 0 {
                granted_write = true;
            }
        }

        if !granted_read {
            return false;
        }
        if req_write && !granted_write {
            return false;
        }

        true
    }
}

/// Tracks an active process attachment to a shared memory segment.
#[derive(Debug, Clone, Copy)]
pub struct ShmAttachment {
    pub pid: u32,
    pub shmid: i32,
    pub vaddr: u64,
    pub size: usize,
    pub read_only: bool,
}

// ── Global Shared Memory Manager ──────────────────────────────────────────────

static NEXT_SHMID: AtomicI32 = AtomicI32::new(1);

/// Singleton manager coordinating all active shared memory segments and process attachments.
pub struct SharedMemoryManager {
    /// Map from `shmid` to `ShmSegment`
    pub(crate) segments: BTreeMap<i32, ShmSegment>,
    /// Map from `key` to `shmid` (excludes `IPC_PRIVATE`)
    pub(crate) key_map: BTreeMap<i32, i32>,
    /// Active process attachments
    pub(crate) attachments: Vec<ShmAttachment>,
}

impl SharedMemoryManager {
    pub const fn new() -> Self {
        Self {
            segments: BTreeMap::new(),
            key_map: BTreeMap::new(),
            attachments: Vec::new(),
        }
    }

    /// Retrieve current time in seconds since epoch.
    fn current_timestamp() -> i64 {
        let (sec, _) = cmos_rtc::get_wall_time();
        if sec > 0 {
            sec as i64
        } else {
            (hpet::elapsed_ns() / 1_000_000_000) as i64
        }
    }

    // ── shmget ────────────────────────────────────────────────────────────────

    /// Implements `shmget(key, size, shmflg)`.
    pub fn shmget(
        &mut self,
        key: i32,
        size: usize,
        shmflg: i32,
        uid: u32,
        gid: u32,
        pid: u32,
    ) -> Result<i32, ShmError> {
        if key != IPC_PRIVATE {
            if let Some(&existing_id) = self.key_map.get(&key) {
                let seg = self.segments.get(&existing_id).ok_or(ShmError::NotFound)?;
                if seg.marked_for_destruction {
                    return Err(ShmError::NotFound);
                }
                if (shmflg & IPC_CREAT) != 0 && (shmflg & IPC_EXCL) != 0 {
                    return Err(ShmError::AlreadyExists);
                }
                if size > 0 && size > seg.size {
                    return Err(ShmError::InvalidArg);
                }
                if !seg.check_perm(uid, gid, false) {
                    return Err(ShmError::PermDenied);
                }
                return Ok(existing_id);
            }

            if (shmflg & IPC_CREAT) == 0 {
                return Err(ShmError::NotFound);
            }
        }

        // Create a new shared memory segment
        if size < SHMMIN || size > SHMMAX {
            return Err(ShmError::InvalidArg);
        }
        if self.segments.len() >= SHMMNI {
            return Err(ShmError::OutOfIds);
        }

        let num_pages = (size + 4095) / 4096;
        let mut frames = Vec::with_capacity(num_pages);
        let hhdm = crate::mm::hhdm_offset();

        for _ in 0..num_pages {
            let frame = match crate::mm::PMM.alloc_page() {
                Some(f) => f,
                None => {
                    for allocated in frames {
                        crate::mm::PMM.free_page(allocated);
                    }
                    return Err(ShmError::NoMem);
                }
            };

            // SAFETY: Zeroing the newly allocated shared physical frame.
            unsafe {
                let dest_ptr = (frame.as_u64() + hhdm) as *mut u8;
                core::ptr::write_bytes(dest_ptr, 0, 4096);
            }
            frames.push(frame);
        }

        let id = NEXT_SHMID.fetch_add(1, Ordering::Relaxed);
        let now = Self::current_timestamp();
        let mode = (shmflg & 0o777) as u32;

        let segment = ShmSegment {
            id,
            key,
            size,
            perm: IpcPerm {
                key,
                uid,
                gid,
                cuid: uid,
                cgid: gid,
                mode,
                seq: 0,
                _pad: [0; 2],
            },
            frames,
            cpid: pid,
            lpid: 0,
            atime: 0,
            dtime: 0,
            ctime: now,
            nattch: 0,
            marked_for_destruction: false,
        };

        self.segments.insert(id, segment);
        if key != IPC_PRIVATE {
            self.key_map.insert(key, id);
        }

        Ok(id)
    }

    // ── shmat ─────────────────────────────────────────────────────────────────

    /// Implements `shmat(shmid, shmaddr, shmflg)`.
    pub fn shmat(
        &mut self,
        shmid: i32,
        shmaddr: u64,
        shmflg: i32,
        uid: u32,
        gid: u32,
        pid: u32,
        addr_space: &mut AddrSpace<ArchPageTable>,
        mmap_bump: &mut u64,
    ) -> Result<u64, ShmError> {
        let seg = self.segments.get_mut(&shmid).ok_or(ShmError::NotFound)?;
        if seg.marked_for_destruction && seg.nattch == 0 {
            return Err(ShmError::NotFound);
        }

        let read_only = (shmflg & SHM_RDONLY) != 0;
        if !seg.check_perm(uid, gid, !read_only) {
            return Err(ShmError::PermDenied);
        }

        let num_pages = seg.frames.len();
        let aligned_size = num_pages * 4096;

        let attach_vaddr = if shmaddr == 0 {
            let vaddr = *mmap_bump;
            *mmap_bump = mmap_bump
                .checked_add(aligned_size as u64)
                .ok_or(ShmError::NoMem)?;
            vaddr
        } else {
            let mut vaddr = shmaddr;
            if (shmflg & SHM_RND) != 0 {
                vaddr &= !(SHMLBA - 1);
            } else if vaddr % SHMLBA != 0 {
                return Err(ShmError::InvalidArg);
            }

            if (shmflg & SHM_REMAP) != 0 {
                let end = vaddr
                    .checked_add(aligned_size as u64)
                    .ok_or(ShmError::InvalidArg)?;
                let _ = addr_space.unmap_range(VirtAddr::new(vaddr), VirtAddr::new(end));
            }
            vaddr
        };

        let mut flags = PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE;
        if !read_only {
            flags |= PageTableFlags::WRITABLE;
        }
        if (shmflg & SHM_EXEC) == 0 {
            flags |= PageTableFlags::NO_EXECUTE;
        }

        // Map pre-allocated physical frames into the process address space
        addr_space
            .map_shared_area(VirtAddr::new(attach_vaddr), flags, &seg.frames, shmid)
            .map_err(|_| ShmError::NoMem)?;

        let now = Self::current_timestamp();
        seg.nattch += 1;
        seg.lpid = pid;
        seg.atime = now;

        self.attachments.push(ShmAttachment {
            pid,
            shmid,
            vaddr: attach_vaddr,
            size: aligned_size,
            read_only,
        });

        Ok(attach_vaddr)
    }

    // ── shmdt ─────────────────────────────────────────────────────────────────

    /// Implements `shmdt(shmaddr)`.
    pub fn shmdt(
        &mut self,
        shmaddr: u64,
        pid: u32,
        addr_space: &mut AddrSpace<ArchPageTable>,
    ) -> Result<(), ShmError> {
        if shmaddr % SHMLBA != 0 {
            return Err(ShmError::InvalidArg);
        }

        let attach_idx = self
            .attachments
            .iter()
            .position(|att| att.pid == pid && att.vaddr == shmaddr)
            .ok_or(ShmError::InvalidArg)?;

        let attachment = self.attachments.remove(attach_idx);

        // Unmap the shared memory virtual address range
        let end_vaddr = shmaddr
            .checked_add(attachment.size as u64)
            .ok_or(ShmError::InvalidArg)?;

        let _ = addr_space.unmap_range(VirtAddr::new(shmaddr), VirtAddr::new(end_vaddr));

        let mut remove_segment = false;
        if let Some(seg) = self.segments.get_mut(&attachment.shmid) {
            seg.nattch = seg.nattch.saturating_sub(1);
            seg.lpid = pid;
            seg.dtime = Self::current_timestamp();

            if seg.marked_for_destruction && seg.nattch == 0 {
                remove_segment = true;
            }
        }

        if remove_segment {
            if let Some(seg) = self.segments.remove(&attachment.shmid) {
                if seg.key != IPC_PRIVATE {
                    self.key_map.remove(&seg.key);
                }
                for frame in seg.frames {
                    crate::mm::PMM.free_page(frame);
                }
            }
        }

        Ok(())
    }

    // ── shmctl ────────────────────────────────────────────────────────────────

    /// Implements `shmctl(shmid, cmd, buf)`.
    pub fn shmctl(
        &mut self,
        shmid: i32,
        cmd: i32,
        out_ds: Option<&mut ShmidDs>,
        in_ds: Option<&ShmidDs>,
        out_info: Option<&mut ShmInfo>,
        uid: u32,
        gid: u32,
    ) -> Result<i32, ShmError> {
        match cmd {
            IPC_RMID => {
                let seg = self.segments.get_mut(&shmid).ok_or(ShmError::NotFound)?;
                if uid != 0 && uid != seg.perm.uid && uid != seg.perm.cuid {
                    return Err(ShmError::PermDenied);
                }

                seg.marked_for_destruction = true;
                if seg.nattch == 0 {
                    if let Some(seg_removed) = self.segments.remove(&shmid) {
                        if seg_removed.key != IPC_PRIVATE {
                            self.key_map.remove(&seg_removed.key);
                        }
                        for frame in seg_removed.frames {
                            crate::mm::PMM.free_page(frame);
                        }
                    }
                }
                Ok(0)
            }

            IPC_STAT | SHM_STAT => {
                let seg = self.segments.get(&shmid).ok_or(ShmError::NotFound)?;
                if !seg.check_perm(uid, gid, false) {
                    return Err(ShmError::PermDenied);
                }
                if let Some(out) = out_ds {
                    *out = seg.ds();
                }
                Ok(if cmd == SHM_STAT { shmid } else { 0 })
            }

            IPC_SET => {
                let seg = self.segments.get_mut(&shmid).ok_or(ShmError::NotFound)?;
                if uid != 0 && uid != seg.perm.uid && uid != seg.perm.cuid {
                    return Err(ShmError::PermDenied);
                }
                let in_val = in_ds.ok_or(ShmError::InvalidArg)?;
                seg.perm.uid = in_val.shm_perm.uid;
                seg.perm.gid = in_val.shm_perm.gid;
                seg.perm.mode = (in_val.shm_perm.mode & 0o777) as u32;
                seg.ctime = Self::current_timestamp();
                Ok(0)
            }

            IPC_INFO | SHM_INFO => {
                if let Some(info) = out_info {
                    *info = ShmInfo {
                        shmmax: SHMMAX,
                        shmmin: SHMMIN,
                        shmmni: SHMMNI,
                        shmseg: SHMMNI,
                        shmall: SHMALL,
                        _pad: [0; 4],
                    };
                }
                Ok(self.segments.keys().next_back().copied().unwrap_or(0))
            }

            SHM_LOCK | SHM_UNLOCK => {
                let seg = self.segments.get(&shmid).ok_or(ShmError::NotFound)?;
                if uid != 0 && uid != seg.perm.uid && uid != seg.perm.cuid {
                    return Err(ShmError::PermDenied);
                }
                // Memory is locked in RAM by default in PetraOS
                Ok(0)
            }

            _ => Err(ShmError::InvalidArg),
        }
    }

    // ── Process lifecycle support ─────────────────────────────────────────────

    /// Handle process exit: detach all shared memory attachments for the terminating process.
    pub fn on_process_exit(&mut self, pid: u32) {
        let mut i = 0;
        let mut segments_to_free = Vec::new();

        while i < self.attachments.len() {
            if self.attachments[i].pid == pid {
                let att = self.attachments.remove(i);
                if let Some(seg) = self.segments.get_mut(&att.shmid) {
                    seg.nattch = seg.nattch.saturating_sub(1);
                    seg.lpid = pid;
                    seg.dtime = Self::current_timestamp();

                    if seg.marked_for_destruction && seg.nattch == 0 {
                        segments_to_free.push(att.shmid);
                    }
                }
            } else {
                i += 1;
            }
        }

        for shmid in segments_to_free {
            if let Some(seg) = self.segments.remove(&shmid) {
                if seg.key != IPC_PRIVATE {
                    self.key_map.remove(&seg.key);
                }
                for frame in seg.frames {
                    crate::mm::PMM.free_page(frame);
                }
            }
        }
    }

    /// Increment attachment count when a process with shared memory VMAs forks.
    pub fn inc_attch(&mut self, shmid: i32) {
        if let Some(seg) = self.segments.get_mut(&shmid) {
            seg.nattch = seg.nattch.saturating_add(1);
        }
    }
}
