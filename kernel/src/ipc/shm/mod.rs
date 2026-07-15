/// Shared Memory Inter-Process Communication (IPC)
///
/// Implements shared memory segments that can be created, attached, detached,
/// and controlled by different processes. Shared memory segments map the same
/// physical memory frames into the virtual address spaces of multiple processes.

use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;
use ostd::mm::{PAGE_SIZE, PageFlags, UFrame};
use ostd::sync::SpinLock;
use ostd::Error;

use crate::proc::pid_table::Pid;
use crate::proc::process::Process;

// System V IPC Shared Memory Constants
pub const IPC_CREAT: u32 = 0o1000;
pub const IPC_EXCL: u32 = 0o2000;
pub const IPC_RMID: u32 = 0;
pub const IPC_PRIVATE: usize = 0;
pub const SHM_RDONLY: u32 = 0o10000;

/// A shared memory segment descriptor.
pub struct ShmSegment {
    /// Unique identifier for this segment.
    pub id: u32,
    /// Associated key.
    pub key: usize,
    /// Size of the segment in bytes (page-aligned).
    pub size: usize,
    /// The physical memory frames allocated for this segment.
    pub frames: Vec<UFrame>,
    /// PID of the process that created the segment.
    pub creator_pid: Pid,
    /// Access permissions.
    pub mode: u32,
    /// Number of active process attachments.
    pub attach_count: usize,
    /// Whether the segment is marked for deletion.
    pub marked_for_deletion: bool,
}

/// Global registry for all shared memory segments and process attachments.
pub struct ShmRegistry {
    segments: SpinLock<BTreeMap<u32, ShmSegment>>,
    attachments: SpinLock<BTreeMap<(Pid, usize), u32>>, // (Pid, Vaddr) -> Shmid
    next_id: core::sync::atomic::AtomicU32,
}

impl ShmRegistry {
    /// Create a new empty `ShmRegistry`.
    pub const fn new() -> Self {
        Self {
            segments: SpinLock::new(BTreeMap::new()),
            attachments: SpinLock::new(BTreeMap::new()),
            next_id: core::sync::atomic::AtomicU32::new(1),
        }
    }

    /// Find or create a shared memory segment.
    pub fn shm_get(&self, key: usize, size: usize, flags: u32) -> Result<u32, Error> {
        if size == 0 {
            return Err(Error::InvalidArgs);
        }

        let size_aligned = (size + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        let current_pid = Process::current().pid;
        let mut segments = self.segments.lock();

        // If key is IPC_PRIVATE, always create a new segment
        if key == IPC_PRIVATE {
            let id = self.next_id.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
            let mut frames = Vec::new();
            let num_pages = size_aligned / PAGE_SIZE;
            for _ in 0..num_pages {
                let frame = ostd::mm::FrameAllocOptions::new()
                    .zeroed(true)
                    .alloc_frame()
                    .map_err(|_| Error::NoMemory)?;
                frames.push(frame.into());
            }

            let segment = ShmSegment {
                id,
                key,
                size: size_aligned,
                frames,
                creator_pid: current_pid,
                mode: flags & 0o777,
                attach_count: 0,
                marked_for_deletion: false,
            };
            segments.insert(id, segment);
            return Ok(id);
        }

        // Search for existing segment with key
        let mut found_id = None;
        for (&id, seg) in segments.iter() {
            if seg.key == key {
                found_id = Some(id);
                break;
            }
        }

        if let Some(id) = found_id {
            // Segment exists
            if (flags & IPC_CREAT) != 0 && (flags & IPC_EXCL) != 0 {
                return Err(Error::InvalidArgs);
            }
            let seg = segments.get(&id).unwrap();
            if size_aligned > seg.size {
                return Err(Error::InvalidArgs);
            }
            return Ok(id);
        }

        // Segment does not exist
        if (flags & IPC_CREAT) == 0 {
            return Err(Error::InvalidArgs);
        }

        // Create new segment
        let id = self.next_id.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        let mut frames = Vec::new();
        let num_pages = size_aligned / PAGE_SIZE;
        for _ in 0..num_pages {
            let frame = ostd::mm::FrameAllocOptions::new()
                .zeroed(true)
                .alloc_frame()
                .map_err(|_| Error::NoMemory)?;
            frames.push(frame.into());
        }

        let segment = ShmSegment {
            id,
            key,
            size: size_aligned,
            frames,
            creator_pid: current_pid,
            mode: flags & 0o777,
            attach_count: 0,
            marked_for_deletion: false,
        };
        segments.insert(id, segment);
        Ok(id)
    }

    /// Attach a shared memory segment to the calling process's address space.
    pub fn shm_at(&self, shmid: u32, shmaddr: usize, flags: u32) -> Result<usize, Error> {
        let current_process = Process::current();
        let current_pid = current_process.pid;
        let vma_manager = &current_process.vm;

        let mut segments = self.segments.lock();
        let segment = segments.get_mut(&shmid).ok_or(Error::InvalidArgs)?;

        if segment.marked_for_deletion && segment.attach_count == 0 {
            return Err(Error::InvalidArgs);
        }

        let size = segment.size;
        let frames = segment.frames.clone();

        // Determine address to attach
        let addr = if shmaddr == 0 {
            vma_manager.find_free_region(size).ok_or(Error::NoMemory)?
        } else {
            if shmaddr % PAGE_SIZE != 0 {
                return Err(Error::InvalidArgs);
            }
            if vma_manager.find_vma(shmaddr).is_some() {
                return Err(Error::InvalidArgs);
            }
            shmaddr
        };

        // Determine page flags
        let page_flags = if (flags & SHM_RDONLY) != 0 {
            PageFlags::R
        } else {
            PageFlags::RW
        };

        // Map frames into current process's address space
        vma_manager.map_shared_frames(addr, &frames, page_flags)?;

        // Record attachment
        let mut attachments = self.attachments.lock();
        attachments.insert((current_pid, addr), shmid);

        segment.attach_count += 1;

        Ok(addr)
    }

    /// Detach a shared memory segment from the calling process's address space.
    pub fn shm_dt(&self, shmaddr: usize) -> Result<(), Error> {
        let current_process = Process::current();
        let current_pid = current_process.pid;
        let vma_manager = &current_process.vm;

        let mut attachments = self.attachments.lock();
        let shmid = attachments
            .remove(&(current_pid, shmaddr))
            .ok_or(Error::InvalidArgs)?;

        let mut segments = self.segments.lock();
        let segment = segments.get_mut(&shmid).ok_or(Error::InvalidArgs)?;

        let vma = vma_manager.find_vma(shmaddr).ok_or(Error::InvalidArgs)?;
        let size = vma.1.size;

        vma_manager.munmap(shmaddr, size)?;

        segment.attach_count = segment.attach_count.saturating_sub(1);

        if segment.marked_for_deletion && segment.attach_count == 0 {
            segments.remove(&shmid);
        }

        Ok(())
    }

    /// Control shared memory options (e.g. deletion).
    pub fn shm_ctl(&self, shmid: u32, cmd: u32) -> Result<(), Error> {
        let mut segments = self.segments.lock();
        if cmd == IPC_RMID {
            let segment = segments.get_mut(&shmid).ok_or(Error::InvalidArgs)?;
            segment.marked_for_deletion = true;
            if segment.attach_count == 0 {
                segments.remove(&shmid);
            }
            Ok(())
        } else {
            Err(Error::InvalidArgs)
        }
    }

    /// Detach all attachments for a terminating process.
    pub fn detach_all_for_process(&self, pid: Pid) {
        let mut attachments = self.attachments.lock();
        let mut segments = self.segments.lock();

        let mut keys_to_remove = Vec::new();
        for (&(att_pid, addr), &shmid) in attachments.iter() {
            if att_pid == pid {
                keys_to_remove.push((att_pid, addr, shmid));
            }
        }

        for (att_pid, addr, shmid) in keys_to_remove {
            attachments.remove(&(att_pid, addr));
            if let Some(segment) = segments.get_mut(&shmid) {
                segment.attach_count = segment.attach_count.saturating_sub(1);
                if segment.marked_for_deletion && segment.attach_count == 0 {
                    segments.remove(&shmid);
                }
            }
        }
    }
}

pub static SHM_REGISTRY: ShmRegistry = ShmRegistry::new();

pub fn shm_get(key: usize, size: usize, flags: u32) -> Result<u32, Error> {
    SHM_REGISTRY.shm_get(key, size, flags)
}

pub fn shm_at(shmid: u32, shmaddr: usize, flags: u32) -> Result<usize, Error> {
    SHM_REGISTRY.shm_at(shmid, shmaddr, flags)
}

pub fn shm_dt(shmaddr: usize) -> Result<(), Error> {
    SHM_REGISTRY.shm_dt(shmaddr)
}

pub fn shm_ctl(shmid: u32, cmd: u32) -> Result<(), Error> {
    SHM_REGISTRY.shm_ctl(shmid, cmd)
}

#[cfg(ktest)]
mod tests {
    use super::*;
    use crate::vm::vma::VmaManager;
    use ostd::prelude::ktest;
    use ostd::mm::PAGE_SIZE;

    #[ktest]
    fn test_shm_get_create_and_get() {
        let key = 0x12345;
        
        let shmid = shm_get(key, PAGE_SIZE, IPC_CREAT).unwrap();
        assert!(shmid > 0);

        let shmid2 = shm_get(key, PAGE_SIZE, 0).unwrap();
        assert_eq!(shmid, shmid2);

        let res = shm_get(key, PAGE_SIZE, IPC_CREAT | IPC_EXCL);
        assert!(res.is_err());

        shm_ctl(shmid, IPC_RMID).unwrap();
    }

    #[ktest]
    fn test_shm_at_and_dt() {
        let vm = Arc::new(VmaManager::new());
        let test_proc = Process::new(vm, "shm_test_proc");
        
        let thread = test_proc.spawn_thread("shm_thread", move || {
            let key = 0x23456;
            let shmid = shm_get(key, PAGE_SIZE, IPC_CREAT).unwrap();
            
            let addr = shm_at(shmid, 0, 0).unwrap();
            assert!(addr > 0);
            
            let current_process = Process::current();
            let test_data = b"Shared Memory content!";
            current_process.vm.copy_to_user(addr, test_data).unwrap();
            
            let mut buf = [0u8; 22];
            current_process.vm.copy_from_user(addr, &mut buf).unwrap();
            assert_eq!(&buf, test_data);
            
            shm_dt(addr).unwrap();
            shm_ctl(shmid, IPC_RMID).unwrap();
        }).unwrap();
        
        test_proc.join_thread(thread.tid);
    }

    #[ktest]
    fn test_shm_sharing() {
        let key = 0x34567;
        let shmid = shm_get(key, PAGE_SIZE, IPC_CREAT).unwrap();

        let vm1 = Arc::new(VmaManager::new());
        let proc1 = Process::new(vm1, "shm_proc1");
        
        let thread1 = proc1.spawn_thread("thread1", move || {
            let addr1 = shm_at(shmid, 0, 0).unwrap();
            let test_data = b"Sharing is caring!";
            Process::current().vm.copy_to_user(addr1, test_data).unwrap();
            shm_dt(addr1).unwrap();
        }).unwrap();
        proc1.join_thread(thread1.tid);

        let vm2 = Arc::new(VmaManager::new());
        let proc2 = Process::new(vm2, "shm_proc2");
        
        let thread2 = proc2.spawn_thread("thread2", move || {
            let addr2 = shm_at(shmid, 0, 0).unwrap();
            let mut buf = [0u8; 18];
            Process::current().vm.copy_from_user(addr2, &mut buf).unwrap();
            assert_eq!(&buf, b"Sharing is caring!");
            shm_dt(addr2).unwrap();
        }).unwrap();
        proc2.join_thread(thread2.tid);

        shm_ctl(shmid, IPC_RMID).unwrap();
    }
}
