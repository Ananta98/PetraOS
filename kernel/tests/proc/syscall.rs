extern crate alloc;

// ── Module shims: expose crate::sched, crate::sync, crate::arch, crate::mm, crate::drivers ──

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

pub mod drivers {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum DriverError {
        InitFailed,
        ReadFailed,
        WriteFailed,
    }

    pub trait DeviceDriver {
        fn name(&self) -> &'static str;
        fn init(&mut self) -> Result<(), DriverError>;
    }

    pub trait CharDevice: DeviceDriver {
        fn read_byte(&mut self) -> Result<u8, DriverError>;
        fn write_byte(&mut self, byte: u8) -> Result<(), DriverError>;
    }

    pub mod serial {
        use super::{DeviceDriver, CharDevice, DriverError};

        pub struct PortIoBackend {
            _port: u16,
        }
        impl PortIoBackend {
            pub fn new(port: u16) -> Self { Self { _port: port } }
        }

        pub struct SerialPort<B> {
            _backend: B,
        }

        impl<B> SerialPort<B> {
            pub fn new(backend: B) -> Self { Self { _backend: backend } }
            pub fn init(&mut self) -> Result<(), DriverError> { Ok(()) }
            pub fn write_byte(&mut self, _byte: u8) -> Result<(), DriverError> { Ok(()) }
        }

        impl<B> DeviceDriver for SerialPort<B> {
            fn name(&self) -> &'static str { "Serial" }
            fn init(&mut self) -> Result<(), DriverError> { Ok(()) }
        }

        impl<B> CharDevice for SerialPort<B> {
            fn read_byte(&mut self) -> Result<u8, DriverError> { Err(DriverError::ReadFailed) }
            fn write_byte(&mut self, _byte: u8) -> Result<(), DriverError> { Ok(()) }
        }
    }
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
        fn disable_interrupts() -> bool { true }
        fn enable_interrupts() {}
        fn halt() {}
        fn init_hardware() {}
        fn cpu_id() -> u32 { 0 }
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

    #[repr(C)]
    #[derive(Debug, Clone, Copy)]
    pub struct SyscallFrame {
        pub r11: u64,
        pub r10: u64,
        pub r9: u64,
        pub r8: u64,
        pub rcx: u64,
        pub rdx: u64,
        pub rsi: u64,
        pub rdi: u64,
        pub rbp: u64,
        pub rax: u64,
        pub rip: u64,
        pub cs: u64,
        pub rflags: u64,
        pub rsp: u64,
        pub ss: u64,
    }

    impl SyscallFrame {
        pub fn syscall_num(&self) -> u64 { self.rax }
        pub fn arg1(&self) -> u64 { self.rdi }
        pub fn arg2(&self) -> u64 { self.rsi }
        pub fn arg3(&self) -> u64 { self.rdx }
        pub fn arg4(&self) -> u64 { self.rcx }
        pub fn set_return_value(&mut self, val: u64) { self.rax = val; }
        pub fn setup_user_entry(&mut self, entry_point: u64, stack_pointer: u64) {
            self.rip = entry_point;
            self.rsp = stack_pointer;
            self.cs = 0x1B;
            self.ss = 0x23;
            self.rflags = 0x202;
        }
    }

    pub mod x86_64 {
        pub mod lapic {
            pub struct MockLapic;
            impl MockLapic {
                pub fn id(&self) -> u32 { 0 }
            }
            pub unsafe fn get_lapic() -> MockLapic { MockLapic }
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

#[path = "../../src/syscalls/mod.rs"]
pub mod syscalls;

#[path = "../../src/logger.rs"]
pub mod logger;

#[unsafe(no_mangle)]
pub extern "C" fn syscall_rust_handler(frame: &mut arch::SyscallFrame) {
    syscalls::handle_syscall(frame);
}

use sched::sched_thread::{SchedThread, ThreadId};
use core::sync::atomic::{AtomicBool, Ordering};

static CHILD_RAN: AtomicBool = AtomicBool::new(false);
static CHILD_WRITE_OK: AtomicBool = AtomicBool::new(false);

extern "C" fn child_thread_entry(_arg: *mut u8) {
    // 1. Mark that child ran
    CHILD_RAN.store(true, Ordering::SeqCst);

    // 2. Perform sys_write to stdout (fd=1)
    let msg = b"Hello from child thread!\n";
    let mut frame_write = arch::SyscallFrame {
        rax: 4, // SYS_WRITE
        rdi: 1, // fd = 1
        rsi: msg.as_ptr() as u64,
        rdx: msg.len() as u64,
        r11: 0, r10: 0, r9: 0, r8: 0, rcx: 0, rbp: 0, rip: 0, cs: 0, rflags: 0, rsp: 0, ss: 0,
    };
    syscalls::handle_syscall(&mut frame_write);

    if frame_write.rax == msg.len() as u64 {
        CHILD_WRITE_OK.store(true, Ordering::SeqCst);
    }

    // 3. Perform sys_write to stderr (fd=2)
    let err_msg = b"Error!\n";
    let mut frame_write_err = arch::SyscallFrame {
        rax: 4, // SYS_WRITE
        rdi: 2, // fd = 2
        rsi: err_msg.as_ptr() as u64,
        rdx: err_msg.len() as u64,
        r11: 0, r10: 0, r9: 0, r8: 0, rcx: 0, rbp: 0, rip: 0, cs: 0, rflags: 0, rsp: 0, ss: 0,
    };
    syscalls::handle_syscall(&mut frame_write_err);

    // 4. Exit with status code 42
    let mut frame_exit = arch::SyscallFrame {
        rax: 1, // SYS_EXIT
        rdi: 42, // exit code
        rsi: 0, rdx: 0, r11: 0, r10: 0, r9: 0, r8: 0, rcx: 0, rbp: 0, rip: 0, cs: 0, rflags: 0, rsp: 0, ss: 0,
    };
    syscalls::handle_syscall(&mut frame_exit);
}

#[test]
fn test_syscall_write_invalid_fd() {
    logger::init();
    
    // Attempt sys_write with invalid fd (e.g. 3)
    let msg = b"test";
    let mut frame = arch::SyscallFrame {
        rax: 4, // SYS_WRITE
        rdi: 3, // fd = 3 (invalid)
        rsi: msg.as_ptr() as u64,
        rdx: msg.len() as u64,
        r11: 0, r10: 0, r9: 0, r8: 0, rcx: 0, rbp: 0, rip: 0, cs: 0, rflags: 0, rsp: 0, ss: 0,
    };
    syscalls::handle_syscall(&mut frame);
    assert_eq!(frame.rax, u64::MAX);
}

#[test]
fn test_syscall_exec_invalid() {
    let mut frame = arch::SyscallFrame {
        rax: 3, // SYS_EXEC
        rdi: 0, // invalid ptr
        rsi: 0, // invalid size
        rdx: 0, r11: 0, r10: 0, r9: 0, r8: 0, rcx: 0, rbp: 0, rip: 0, cs: 0, rflags: 0, rsp: 0, ss: 0,
    };
    syscalls::handle_syscall(&mut frame);
    assert_eq!(frame.rax, u64::MAX);
}

#[test]
fn test_syscall_fork_waitpid_exit() {
    CHILD_RAN.store(false, Ordering::SeqCst);
    CHILD_WRITE_OK.store(false, Ordering::SeqCst);
    logger::init();

    // Register CPU 0 in the global scheduler
    {
        let mut sched = sched::scheduler::GLOBAL_SCHEDULER.lock();
        sched.register_cpu(0);
    }

    // Initialize threads (main + idle)
    proc::init_threads(0);

    // Call sys_fork from the main thread
    let mut frame_fork = arch::SyscallFrame {
        rax: 2, // SYS_FORK
        rdi: 0, rsi: 0, rdx: 0, r11: 0, r10: 0, r9: 0, r8: 0, rcx: 0, rbp: 0, rip: 0, cs: 0, rflags: 0, rsp: 0, ss: 0,
    };
    syscalls::handle_syscall(&mut frame_fork);
    let child_pid = frame_fork.rax;
    assert!(child_pid > 1);

    // Spawn a thread for the child process in our manager and scheduler
    let child_tid = ThreadId(2);
    proc::spawn_thread(child_tid, proc::ProcessId(child_pid), child_thread_entry, core::ptr::null_mut());
    {
        let mut sched = sched::scheduler::GLOBAL_SCHEDULER.lock();
        sched.spawn_thread(SchedThread::new_normal(child_tid), Some(0));
    }

    // Call sys_waitpid for the child process
    let mut frame_wait = arch::SyscallFrame {
        rax: 5, // SYS_WAITPID
        rdi: child_pid,
        rsi: 0, rdx: 0, r11: 0, r10: 0, r9: 0, r8: 0, rcx: 0, rbp: 0, rip: 0, cs: 0, rflags: 0, rsp: 0, ss: 0,
    };
    syscalls::handle_syscall(&mut frame_wait);

    // Verify child process ran and exit status matches
    assert!(CHILD_RAN.load(Ordering::SeqCst), "Child thread did not run");
    assert!(CHILD_WRITE_OK.load(Ordering::SeqCst), "Child sys_write failed or returned incorrect length");
    assert_eq!(frame_wait.rax, 42, "waitpid did not return child's exit code");
}
