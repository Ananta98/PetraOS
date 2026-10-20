// SPDX-License-Identifier: GPL-2.0

//! Resource limits.
//!
//! Manages per-process resource limits (`struct plimit` and `struct rlimit`).
//! Resource limits are shared across threads within a process and are cloned
//! across `fork(2)`. Modifications follow Copy-on-Write semantics.

use crate::api::errno::Result;

/// Unbounded resource limit value (POSIX `RLIM_INFINITY`).
pub const RLIM_INFINITY: u64 = !0;

/// Resource limit identifiers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum Resource {
    /// CPU time per process in seconds.
    Cpu = 0,
    /// Maximum file size in bytes.
    Fsize = 1,
    /// Maximum data segment size in bytes.
    Data = 2,
    /// Maximum stack size in bytes.
    Stack = 3,
    /// Maximum core file size in bytes.
    Core = 4,
    /// Maximum resident set size in bytes.
    Rss = 5,
    /// Maximum locked memory in bytes.
    Memlock = 6,
    /// Maximum number of processes per UID.
    Nproc = 7,
    /// Maximum number of open file descriptors.
    Nofile = 8,
    /// Maximum address space size in bytes.
    As = 9,
}

impl Resource {
    pub const COUNT: usize = 10;

    pub const fn from_usize(val: usize) -> Option<Self> {
        match val {
            0 => Some(Self::Cpu),
            1 => Some(Self::Fsize),
            2 => Some(Self::Data),
            3 => Some(Self::Stack),
            4 => Some(Self::Core),
            5 => Some(Self::Rss),
            6 => Some(Self::Memlock),
            7 => Some(Self::Nproc),
            8 => Some(Self::Nofile),
            9 => Some(Self::As),
            _ => None,
        }
    }
}

/// Resource limit pair (current soft limit and maximum hard limit).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RLimit {
    /// Soft limit (current effective limit).
    pub rlim_cur: u64,
    /// Hard limit (ceiling that non-root processes cannot raise).
    pub rlim_max: u64,
}

impl RLimit {
    pub const fn infinity() -> Self {
        Self {
            rlim_cur: RLIM_INFINITY,
            rlim_max: RLIM_INFINITY,
        }
    }

    pub const fn new(cur: u64, max: u64) -> Self {
        Self {
            rlim_cur: cur,
            rlim_max: max,
        }
    }
}

/// Process resource limits container.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plimit {
    pub limits: [RLimit; Resource::COUNT],
}

impl Plimit {
    /// Creates a default set of resource limits.
    pub fn default_limits() -> Self {
        let mut limits = [RLimit::infinity(); Resource::COUNT];
        // Stack limit default: 8 MiB soft, 64 MiB hard
        limits[Resource::Stack as usize] = RLimit::new(8 * 1024 * 1024, 64 * 1024 * 1024);
        // File descriptors default: 1024 soft, 4096 hard
        limits[Resource::Nofile as usize] = RLimit::new(1024, 4096);
        // Max processes default: 512 soft, 1024 hard
        limits[Resource::Nproc as usize] = RLimit::new(512, 1024);
        Self { limits }
    }

    /// Gets the current limit for a resource.
    pub fn get(&self, res: Resource) -> RLimit {
        self.limits[res as usize]
    }

    /// Sets the limit for a resource with permission validation.
    pub fn set(&mut self, res: Resource, new_lim: RLimit, is_root: bool) -> Result<()> {
        if new_lim.rlim_cur > new_lim.rlim_max {
            crate::return_errno!(EINVAL, "soft limit exceeds hard limit");
        }
        let old_lim = self.limits[res as usize];
        if new_lim.rlim_max > old_lim.rlim_max && !is_root {
            crate::return_errno!(EPERM, "cannot raise hard limit without superuser privilege");
        }
        self.limits[res as usize] = new_lim;
        Ok(())
    }
}

impl Default for Plimit {
    fn default() -> Self {
        Self::default_limits()
    }
}

/// Linux x86_64 resource usage statistics (POSIX `struct rusage`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(C)]
pub struct Rusage {
    /// User CPU time used (seconds)
    pub ru_utime_sec: i64,
    /// User CPU time used (microseconds)
    pub ru_utime_usec: i64,
    /// System CPU time used (seconds)
    pub ru_stime_sec: i64,
    /// System CPU time used (microseconds)
    pub ru_stime_usec: i64,
    /// Maximum resident set size in kilobytes
    pub ru_maxrss: i64,
    /// Integral shared memory size
    pub ru_ixrss: i64,
    /// Integral unshared data size
    pub ru_idrss: i64,
    /// Integral unshared stack size
    pub ru_isrss: i64,
    /// Page reclaims (soft page faults)
    pub ru_minflt: i64,
    /// Page faults (hard page faults)
    pub ru_majflt: i64,
    /// Swaps
    pub ru_nswap: i64,
    /// Block input operations
    pub ru_inblock: i64,
    /// Block output operations
    pub ru_oublock: i64,
    /// IPC messages sent
    pub ru_msgsnd: i64,
    /// IPC messages received
    pub ru_msgrcv: i64,
    /// Signals received
    pub ru_nsignals: i64,
    /// Voluntary context switches
    pub ru_nvcsw: i64,
    /// Involuntary context switches
    pub ru_nivcsw: i64,
}

impl Rusage {
    /// Accumulates metrics from another `Rusage` into `self`.
    pub fn accumulate(&mut self, other: &Self) {
        self.ru_utime_sec += other.ru_utime_sec;
        self.ru_utime_usec += other.ru_utime_usec;
        if self.ru_utime_usec >= 1_000_000 {
            self.ru_utime_sec += self.ru_utime_usec / 1_000_000;
            self.ru_utime_usec %= 1_000_000;
        }

        self.ru_stime_sec += other.ru_stime_sec;
        self.ru_stime_usec += other.ru_stime_usec;
        if self.ru_stime_usec >= 1_000_000 {
            self.ru_stime_sec += self.ru_stime_usec / 1_000_000;
            self.ru_stime_usec %= 1_000_000;
        }

        if other.ru_maxrss > self.ru_maxrss {
            self.ru_maxrss = other.ru_maxrss;
        }
        self.ru_minflt += other.ru_minflt;
        self.ru_majflt += other.ru_majflt;
        self.ru_inblock += other.ru_inblock;
        self.ru_oublock += other.ru_oublock;
        self.ru_nvcsw += other.ru_nvcsw;
        self.ru_nivcsw += other.ru_nivcsw;
    }

    /// Converts into raw byte representation for userspace copying.
    pub fn to_bytes(&self) -> [u8; core::mem::size_of::<Self>()] {
        let mut bytes = [0u8; core::mem::size_of::<Self>()];
        let fields = [
            self.ru_utime_sec,
            self.ru_utime_usec,
            self.ru_stime_sec,
            self.ru_stime_usec,
            self.ru_maxrss,
            self.ru_ixrss,
            self.ru_idrss,
            self.ru_isrss,
            self.ru_minflt,
            self.ru_majflt,
            self.ru_nswap,
            self.ru_inblock,
            self.ru_oublock,
            self.ru_msgsnd,
            self.ru_msgrcv,
            self.ru_nsignals,
            self.ru_nvcsw,
            self.ru_nivcsw,
        ];
        for (i, val) in fields.iter().enumerate() {
            bytes[i * 8..(i + 1) * 8].copy_from_slice(&val.to_ne_bytes());
        }
        bytes
    }
}
