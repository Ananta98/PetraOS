pub mod brk;
pub mod mmap;
pub mod munmap;
pub mod shm;

pub use brk::syscall_brk;
pub use mmap::syscall_mmap;
pub use munmap::syscall_munmap;
pub use shm::{syscall_shmat, syscall_shmctl, syscall_shmdt, syscall_shmget};
