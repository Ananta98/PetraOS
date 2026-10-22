// SPDX-License-Identifier: GPL-2.0

//! The interface user space sees.
//!
//! Every type here is part of an ABI: either a shape user space passes to or
//! receives from a syscall, or a set of constants both sides agree on. They are
//! therefore layout-sensitive (`#[repr(C)]` where a C struct is implied), and the
//! comments about them describe what the *other* side of the boundary expects
//! rather than how the kernel uses them internally.
//!
//! | Module | ABI |
//! |---|---|
//! | [`errno`] | `errno` numbers, and the negated value a failed syscall returns |
//! | [`drm`] | `drm.h`/`drm_mode.h` UAPI: LGPL-style ioctl numbers, GEM flink
//!   structs and the KMS mode tables |
//! | [`cred`] | `uid_t`, `gid_t` and `struct ucred` |
//! | [`ipc`] | System V IPC common constants and `struct ipc_perm` |
//! | [`shm`] | System V Shared Memory `struct shmid_ds`, `struct shminfo`, and `SHM_*` constants |
//! | [`sem`] | System V Semaphore `struct sembuf`, `struct semid_ds`, and `SEM_*` commands |
//! | [`sched`] | Scheduler policies, `struct sched_param`, `struct sched_attr`, `cpu_set_t` |
//! | [`reboot`] | `reboot(2)` magic numbers and command codes |
//! | [`random`] | `getrandom(2)` flag definitions |
//! | [`limit`] | `struct rlimit` and the `RLIMIT_*` resource identifiers |
//! | [`signal`] | `sigset_t`, `struct sigaction` and the signal numbers |
//! | [`termios`] | `struct termios`, `struct winsize`, `tcflag_t` |
//! | [`ioctl`] | `ioctl(2)` request numbers |
//!
//! Nothing here acts on the data. The mechanisms that do live next to their
//! subject: signal delivery and process state in [`crate::proc`], file
//! descriptors in [`crate::fs`].

pub mod cred;
pub mod drm;
pub mod errno;
pub mod ioctl;
pub mod ipc;
pub mod limit;
pub mod msg;
pub mod random;
pub mod reboot;
pub mod sched;
pub mod sem;
pub mod shm;
pub mod signal;
pub mod termios;
