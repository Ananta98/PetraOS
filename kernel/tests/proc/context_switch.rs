extern crate alloc;

// ── Module shim: expose crate::sched, crate::sync, and crate::arch ───────────

#[path = "."]
pub mod sync {
    #[path = "."]
    pub mod spinlock {
        #[path = "../../src/sync/spinlock.rs"]
        pub mod impl_spinlock;
        pub use impl_spinlock::Spinlock;
    }
}

#[path = "."]
pub mod sched {
    #[path = "../../src/sched/sched_thread.rs"]
    pub mod sched_thread;

    #[path = "../../src/sched/cfs.rs"]
    pub mod cfs;

    #[path = "../../src/sched/rt.rs"]
    pub mod rt;

    #[path = "../../src/sched/policy.rs"]
    pub mod policy;

    #[path = "../../src/sched/scheduler.rs"]
    pub mod scheduler;

    pub use scheduler::MAX_CPUS;
}

#[path = "../../src/arch/x86_64/context_switch.rs"]
pub mod x86_64_thread;

pub mod mm {
    pub mod address {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
        pub struct PhysAddr(pub u64);
        impl PhysAddr {
            pub fn as_ptr<T>(self, _hhdm: u64) -> *mut T { self.0 as *mut T }
            pub fn as_u64(self) -> u64 { self.0 }
        }
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
        pub struct VirtAddr(pub u64);
        impl VirtAddr {
            pub fn as_u64(self) -> u64 { self.0 }
        }
        impl core::ops::Sub<usize> for VirtAddr {
            type Output = Self;
            fn sub(self, rhs: usize) -> Self::Output { Self(self.0 - rhs as u64) }
        }
        impl core::ops::Sub<VirtAddr> for VirtAddr {
            type Output = u64;
            fn sub(self, rhs: VirtAddr) -> Self::Output { self.0 - rhs.0 }
        }
        impl core::ops::Add<usize> for VirtAddr {
            type Output = Self;
            fn add(self, rhs: usize) -> Self::Output { Self(self.0 + rhs as u64) }
        }
    }
    pub use address::{PhysAddr, VirtAddr};

    pub mod paging {
        use super::{PhysAddr, VirtAddr};
        bitflags::bitflags! {
            #[derive(Debug, Clone, Copy, PartialEq, Eq)]
            pub struct MapFlags: u64 {
                const READ     = 1 << 0;
                const WRITE    = 1 << 1;
                const EXECUTE  = 1 << 2;
                const USER     = 1 << 3;
                const NO_CACHE = 1 << 4;
            }
        }
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum MapError { FrameAllocationFailed }
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum UnmapError { NotMapped }
        pub trait PageTable {
            fn new() -> Result<Self, MapError> where Self: Sized;
            unsafe fn from_root(root: PhysAddr) -> Self where Self: Sized;
            fn root(&self) -> PhysAddr;
            fn map(&mut self, page: VirtAddr, frame: PhysAddr, flags: MapFlags) -> Result<(), MapError>;
            fn unmap(&mut self, page: VirtAddr) -> Result<PhysAddr, UnmapError>;
            fn translate(&self, virt: VirtAddr) -> Option<PhysAddr>;
            unsafe fn activate(&self);
        }
    }
    pub use paging::{MapFlags, MapError, UnmapError, PageTable};

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum VmAreaKind { Anonymous }
    
    pub struct AddrSpace<P: paging::PageTable> {
        page_table: P,
    }
    impl<P: paging::PageTable> AddrSpace<P> {
        pub fn new(page_table: P) -> Self { Self { page_table } }
        pub fn page_table(&self) -> &P { &self.page_table }
        pub fn map_area(&mut self, _start: VirtAddr, _size: usize, _flags: paging::MapFlags, _kind: VmAreaKind) -> Result<(), &'static str> { Ok(()) }
        pub unsafe fn activate(&self) { unsafe { self.page_table.activate(); } }
    }

    pub fn hhdm_offset() -> u64 { 0 }
}

pub mod arch {
    pub trait CpuArch {
        fn disable_interrupts() -> bool;
        fn enable_interrupts();
        fn halt();
        fn init_hardware();
        fn cpu_id() -> u32;
        fn init_stack(stack: &mut [u8], entry: extern "C" fn(*mut u8), arg: *mut u8) -> u64;
        unsafe fn switch_context(prev_rsp_ptr: *mut u64, next_rsp: u64);
        unsafe fn switch_context_to(next_rsp: u64) -> !;
    }

    pub struct ArchImpl;

    impl CpuArch for ArchImpl {
        fn disable_interrupts() -> bool {
            true
        }
        fn enable_interrupts() {}
        fn halt() {}
        fn init_hardware() {}
        fn cpu_id() -> u32 {
            0
        }
        fn init_stack(stack: &mut [u8], entry: extern "C" fn(*mut u8), arg: *mut u8) -> u64 {
            super::x86_64_thread::init_stack(stack, entry, arg)
        }
        unsafe fn switch_context(prev_rsp_ptr: *mut u64, next_rsp: u64) {
            unsafe {
                super::x86_64_thread::switch_context(prev_rsp_ptr, next_rsp);
            }
        }
        unsafe fn switch_context_to(next_rsp: u64) -> ! {
            unsafe {
                super::x86_64_thread::switch_context_to(next_rsp);
            }
        }
    }

    pub mod x86_64 {
        pub mod lapic {
            pub struct MockLapic;
            impl MockLapic {
                pub fn id(&self) -> u32 {
                    0
                }
            }
            pub unsafe fn get_lapic() -> MockLapic {
                MockLapic
            }
        }
        pub mod paging {
            use crate::mm::paging::{PageTable, MapError, MapFlags, UnmapError};
            use crate::mm::{PhysAddr, VirtAddr};
            pub struct X86_64PageTable;
            impl PageTable for X86_64PageTable {
                fn new() -> Result<Self, MapError> { Ok(Self) }
                unsafe fn from_root(_root: PhysAddr) -> Self { Self }
                fn root(&self) -> PhysAddr { PhysAddr(0) }
                fn map(&mut self, _page: VirtAddr, _frame: PhysAddr, _flags: MapFlags) -> Result<(), MapError> { Ok(()) }
                fn unmap(&mut self, _page: VirtAddr) -> Result<PhysAddr, UnmapError> { Ok(PhysAddr(0)) }
                fn translate(&self, _virt: VirtAddr) -> Option<PhysAddr> { None }
                unsafe fn activate(&self) {}
            }
        }
    }
}

#[path = "../../src/proc/mod.rs"]
pub mod proc;

#[repr(C)]
pub struct SyscallFrame;

#[unsafe(no_mangle)]
pub extern "C" fn syscall_rust_handler(_frame: &mut SyscallFrame) {}

use sched::sched_thread::{SchedThread, ThreadId};
use core::sync::atomic::{AtomicBool, Ordering};

// ── Integration Tests ────────────────────────────────────────────────────────

static THREAD_1_RAN: AtomicBool = AtomicBool::new(false);
static THREAD_2_RAN: AtomicBool = AtomicBool::new(false);

extern "C" fn test_thread_1(_arg: *mut u8) {
    THREAD_1_RAN.store(true, Ordering::SeqCst);
    // Yield to let thread 2 run
    proc::yield_now();
}

extern "C" fn test_thread_2(_arg: *mut u8) {
    THREAD_2_RAN.store(true, Ordering::SeqCst);
}

#[test]
fn test_thread_creation() {
    let tid = ThreadId(99);
    let thread = proc::thread::Thread::new(tid, proc::ProcessId(1), test_thread_2, core::ptr::null_mut());

    assert_eq!(thread.id, tid);
    assert_eq!(thread.state, proc::ThreadState::Ready);
    assert!(thread.stack.is_some());
    assert!(thread.rsp != 0);
}

#[test]
fn test_context_switch_integration() {
    // Reset flags in case test runs multiple times
    THREAD_1_RAN.store(false, Ordering::SeqCst);
    THREAD_2_RAN.store(false, Ordering::SeqCst);

    // Register CPU 0 in the global scheduler
    {
        let mut sched = sched::scheduler::GLOBAL_SCHEDULER.lock();
        sched.register_cpu(0);
    }

    // Initialize threads (main + idle)
    proc::init_threads(0);

    // Spawn thread 1
    let tid1 = ThreadId(1);
    proc::spawn_thread(tid1, proc::ProcessId(1), test_thread_1, core::ptr::null_mut());
    {
        let mut sched = sched::scheduler::GLOBAL_SCHEDULER.lock();
        sched.spawn_thread(SchedThread::new_normal(tid1), Some(0));
    }

    // Spawn thread 2
    let tid2 = ThreadId(2);
    proc::spawn_thread(tid2, proc::ProcessId(1), test_thread_2, core::ptr::null_mut());
    {
        let mut sched = sched::scheduler::GLOBAL_SCHEDULER.lock();
        sched.spawn_thread(SchedThread::new_normal(tid2), Some(0));
    }

    // Yield main thread to run thread 1 and thread 2
    proc::yield_now();
    proc::yield_now();

    // Verify both threads ran and returned to main thread successfully
    assert!(THREAD_1_RAN.load(Ordering::SeqCst), "Thread 1 did not run");
    assert!(THREAD_2_RAN.load(Ordering::SeqCst), "Thread 2 did not run");
}
