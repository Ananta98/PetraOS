pub(crate) mod brk;
pub(crate) mod mmap;
pub(crate) mod munmap;

pub(crate) use brk::syscall_brk;
pub(crate) use mmap::syscall_mmap;
pub(crate) use munmap::syscall_munmap;
