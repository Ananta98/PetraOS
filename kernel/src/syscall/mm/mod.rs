pub mod brk;
pub mod madvise;
pub mod memfd;
pub mod mmap;
pub mod mprotect;
pub mod mremap;
pub mod msync;
pub mod munmap;
pub mod shm;

pub use brk::syscall_brk;
pub use madvise::syscall_madvise;
pub use memfd::syscall_memfd_create;
pub use mmap::syscall_mmap;
pub use mprotect::syscall_mprotect;
pub use mremap::syscall_mremap;
pub use msync::syscall_msync;
pub use munmap::syscall_munmap;
pub use shm::{syscall_shmat, syscall_shmctl, syscall_shmdt, syscall_shmget};
