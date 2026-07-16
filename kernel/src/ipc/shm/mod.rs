/// Shared Memory Inter-Process Communication (IPC)
///
/// Implements shared memory segments that can be created, attached, detached,
/// and controlled by different processes. Shared memory segments map the same
/// physical memory frames into the virtual address spaces of multiple processes.
use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;
use ostd::Error;
use ostd::mm::{PAGE_SIZE, PageFlags, UFrame};
use ostd::sync::SpinLock;

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
            let id = self
                .next_id
                .fetch_add(1, core::sync::atomic::Ordering::Relaxed);
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
        let id = self
            .next_id
            .fetch_add(1, core::sync::atomic::Ordering::Relaxed);
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

        {
            let attachments = self.attachments.lock();
            if !attachments.contains_key(&(current_pid, shmaddr)) {
                return Err(Error::InvalidArgs);
            }
        }

        let vma = vma_manager.find_vma(shmaddr).ok_or(Error::InvalidArgs)?;
        let size = vma.1.size;

        vma_manager.munmap(shmaddr, size)?;

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

    /// Clone attachments from parent process to child process during fork.
    pub fn clone_attachments_for_fork(&self, parent_pid: Pid, child_pid: Pid) {
        let mut attachments = self.attachments.lock();
        let mut segments = self.segments.lock();

        let mut child_atts = Vec::new();
        for (&(pid, addr), &shmid) in attachments.iter() {
            if pid == parent_pid {
                child_atts.push((addr, shmid));
            }
        }

        for (addr, shmid) in child_atts {
            attachments.insert((child_pid, addr), shmid);
            if let Some(segment) = segments.get_mut(&shmid) {
                segment.attach_count += 1;
            }
        }
    }

    /// Detach a shared memory segment if it is currently attached to the process at the given address.
    pub fn shm_dt_if_attached(&self, pid: Pid, shmaddr: usize) {
        let mut attachments = self.attachments.lock();
        if let Some(shmid) = attachments.remove(&(pid, shmaddr)) {
            let mut segments = self.segments.lock();
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

pub fn clone_attachments_for_fork(parent_pid: Pid, child_pid: Pid) {
    SHM_REGISTRY.clone_attachments_for_fork(parent_pid, child_pid);
}

pub fn shm_dt_if_attached(pid: Pid, shmaddr: usize) {
    SHM_REGISTRY.shm_dt_if_attached(pid, shmaddr);
}

#[cfg(ktest)]
mod tests {
    use super::*;
    use crate::vm::vma::VmaManager;
    use ostd::mm::PAGE_SIZE;
    use ostd::prelude::ktest;

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

        let thread = test_proc
            .spawn_thread("shm_thread", move || {
                let current_process = Process::current();
                current_process.vm.activate();

                let key = 0x23456;
                let shmid = shm_get(key, PAGE_SIZE, IPC_CREAT).unwrap();

                let addr = shm_at(shmid, 0, 0).unwrap();
                assert!(addr > 0);

                let test_data = b"Shared Memory content!";
                current_process.vm.copy_to_user(addr, test_data).unwrap();

                let mut buf = [0u8; 22];
                current_process.vm.copy_from_user(addr, &mut buf).unwrap();
                assert_eq!(&buf, test_data);

                shm_dt(addr).unwrap();
                shm_ctl(shmid, IPC_RMID).unwrap();
            })
            .unwrap();

        test_proc.join_thread(thread.tid);
    }

    #[ktest]
    fn test_shm_sharing() {
        use ostd::sync::SpinLock as OstdSpinLock;

        // `shm_get` internally calls `Process::current()` to record the
        // creator PID.  Calling it outside a task context causes a panic when
        // no init process (PID 1) is registered.  Run all registry operations
        // inside spawned threads so that a valid process context is always
        // available.
        let shared_shmid: Arc<OstdSpinLock<u32>> = Arc::new(OstdSpinLock::new(0));

        // ── Setup: create the segment inside a thread ─────────────────────
        let vm_setup = Arc::new(VmaManager::new());
        let proc_setup = Process::new(vm_setup, "shm_setup");
        let shmid_ref = shared_shmid.clone();
        let setup_thread = proc_setup
            .spawn_thread("setup_thread", move || {
                let key = 0x34567;
                let id = shm_get(key, PAGE_SIZE, IPC_CREAT).unwrap();
                *shmid_ref.lock() = id;
            })
            .unwrap();
        proc_setup.join_thread(setup_thread.tid);

        let shmid = *shared_shmid.lock();

        // ── Writer: proc1 maps, writes data, then detaches ────────────────
        let vm1 = Arc::new(VmaManager::new());
        let proc1 = Process::new(vm1, "shm_proc1");

        let thread1 = proc1
            .spawn_thread("thread1", move || {
                let current_process = Process::current();
                current_process.vm.activate();

                let addr1 = shm_at(shmid, 0, 0).unwrap();
                let test_data = b"Sharing is caring!";
                current_process.vm.copy_to_user(addr1, test_data).unwrap();
                shm_dt(addr1).unwrap();
            })
            .unwrap();
        proc1.join_thread(thread1.tid);

        // ── Reader: proc2 maps, reads data, verifies, then detaches ───────
        let vm2 = Arc::new(VmaManager::new());
        let proc2 = Process::new(vm2, "shm_proc2");

        let thread2 = proc2
            .spawn_thread("thread2", move || {
                let current_process = Process::current();
                current_process.vm.activate();

                let addr2 = shm_at(shmid, 0, 0).unwrap();
                let mut buf = [0u8; 18];
                current_process.vm.copy_from_user(addr2, &mut buf).unwrap();
                assert_eq!(&buf, b"Sharing is caring!");
                shm_dt(addr2).unwrap();
            })
            .unwrap();
        proc2.join_thread(thread2.tid);

        // ── Teardown: mark the segment for deletion ────────────────────────
        shm_ctl(shmid, IPC_RMID).unwrap();
    }

    #[ktest]
    fn test_shm_fork() {
        let parent_vm = Arc::new(VmaManager::new());
        let parent_proc = Process::new(parent_vm, "shm_parent");

        let thread = parent_proc
            .spawn_thread("parent_thread", move || {
                let current_process = Process::current();
                current_process.vm.activate();

                let key = 0x45678;
                let shmid = shm_get(key, PAGE_SIZE, IPC_CREAT).unwrap();
                let addr = shm_at(shmid, 0, 0).unwrap();

                let initial_data = b"Hello Fork!";
                current_process.vm.copy_to_user(addr, initial_data).unwrap();

                // Fork the child process
                let child_proc = current_process.fork().unwrap();

                // Run child-specific logic in a thread
                let child_thread = child_proc
                    .spawn_thread("child_thread", move || {
                        let child_process = Process::current();
                        child_process.vm.activate();

                        // Verify that the child can read the parent's data at the same address
                        let mut buf = [0u8; 11];
                        child_process.vm.copy_from_user(addr, &mut buf).unwrap();
                        assert_eq!(&buf, b"Hello Fork!");

                        // Modify the data in the child
                        let modified_data = b"Hello Child";
                        child_process.vm.copy_to_user(addr, modified_data).unwrap();
                    })
                    .unwrap();

                child_proc.join_thread(child_thread.tid);

                // Reactivate parent's VM space since the child thread activated its own
                current_process.vm.activate();

                // Now parent reads the modified data, verifying it is shared (not CoW)
                let mut buf = [0u8; 11];
                current_process.vm.copy_from_user(addr, &mut buf).unwrap();
                assert_eq!(&buf, b"Hello Child");

                shm_dt(addr).unwrap();
                shm_ctl(shmid, IPC_RMID).unwrap();
            })
            .unwrap();

        parent_proc.join_thread(thread.tid);
    }
}
