//! Minimal POSIX/Linux syscall & C-ABI FFI wrappers compatible with both glibc and musl.

use std::ffi::{CStr, CString};
use std::io;
use std::os::raw::{c_char, c_int, c_ulong, c_void};

pub const O_RDONLY: c_int = 0;
pub const O_WRONLY: c_int = 1;
pub const O_RDWR: c_int = 2;
pub const O_CREAT: c_int = 0o100;
pub const O_NOCTTY: c_int = 0o400;
pub const O_NONBLOCK: c_int = 0o4000;
pub const O_CLOEXEC: c_int = 0o2000000;

pub const PROT_READ: c_int = 1;
pub const PROT_WRITE: c_int = 2;
pub const MAP_SHARED: c_int = 1;
pub const MAP_FAILED: *mut c_void = !0usize as *mut c_void;

pub const F_GETFL: c_int = 3;
pub const F_SETFL: c_int = 4;

pub const AF_UNIX: c_int = 1;
pub const SOCK_STREAM: c_int = 1;
pub const SOCK_NONBLOCK: c_int = 0o4000;
pub const SOCK_CLOEXEC: c_int = 0o2000000;

pub const POLLIN: i16 = 0x001;
pub const TIOCSCTTY: c_ulong = 0x540E;
pub const TIOCSWINSZ: c_ulong = 0x5414;

pub const WNOHANG: c_int = 1;

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct PollFd {
    pub fd: c_int,
    pub events: i16,
    pub revents: i16,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SockAddrUn {
    pub sun_family: u16,
    pub sun_path: [c_char; 108],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct WinSize {
    pub ws_row: u16,
    pub ws_col: u16,
    pub ws_xpixel: u16,
    pub ws_ypixel: u16,
}

extern "C" {
    pub fn open(pathname: *const c_char, flags: c_int, ...) -> c_int;
    pub fn close(fd: c_int) -> c_int;
    pub fn read(fd: c_int, buf: *mut c_void, count: usize) -> isize;
    pub fn write(fd: c_int, buf: *const c_void, count: usize) -> isize;
    pub fn ioctl(fd: c_int, request: c_ulong, ...) -> c_int;
    pub fn mmap(
        addr: *mut c_void,
        length: usize,
        prot: c_int,
        flags: c_int,
        fd: c_int,
        offset: i64,
    ) -> *mut c_void;
    pub fn munmap(addr: *mut c_void, length: usize) -> c_int;
    pub fn ftruncate(fd: c_int, length: i64) -> c_int;
    pub fn unlink(pathname: *const c_char) -> c_int;
    pub fn poll(fds: *mut PollFd, nfds: c_ulong, timeout: c_int) -> c_int;
    pub fn socket(domain: c_int, ty: c_int, protocol: c_int) -> c_int;
    pub fn bind(sockfd: c_int, addr: *const SockAddrUn, addrlen: u32) -> c_int;
    pub fn listen(sockfd: c_int, backlog: c_int) -> c_int;
    pub fn accept(sockfd: c_int, addr: *mut c_void, addrlen: *mut u32) -> c_int;
    pub fn connect(sockfd: c_int, addr: *const SockAddrUn, addrlen: u32) -> c_int;
    pub fn fcntl(fd: c_int, cmd: c_int, ...) -> c_int;
    pub fn kill(pid: c_int, sig: c_int) -> c_int;
    pub fn getpid() -> c_int;
    pub fn fork() -> c_int;
    pub fn setsid() -> c_int;
    pub fn dup2(oldfd: c_int, newfd: c_int) -> c_int;
    pub fn execvp(file: *const c_char, argv: *const *const c_char) -> c_int;
    pub fn _exit(status: c_int) -> !;
    pub fn waitpid(pid: c_int, status: *mut c_int, options: c_int) -> c_int;
    pub fn posix_openpt(flags: c_int) -> c_int;
    pub fn grantpt(fd: c_int) -> c_int;
    pub fn unlockpt(fd: c_int) -> c_int;
    pub fn ptsname(fd: c_int) -> *const c_char;
}

pub fn open_path(path: &str, flags: c_int, mode: u32) -> io::Result<c_int> {
    let c_path = CString::new(path).map_err(|_| io::Error::from_raw_os_error(22))?;
    let fd = unsafe { open(c_path.as_ptr(), flags, mode) };
    if fd < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(fd)
    }
}

pub fn set_nonblocking(fd: c_int) -> io::Result<()> {
    let flags = unsafe { fcntl(fd, F_GETFL) };
    if flags < 0 {
        return Err(io::Error::last_os_error());
    }
    if unsafe { fcntl(fd, F_SETFL, flags | O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

pub fn make_sockaddr_un(path: &str) -> io::Result<(SockAddrUn, u32)> {
    let bytes = path.as_bytes();
    if bytes.len() >= 107 {
        return Err(io::Error::from_raw_os_error(36));
    }
    let mut addr = SockAddrUn {
        sun_family: AF_UNIX as u16,
        sun_path: [0; 108],
    };
    for (i, &b) in bytes.iter().enumerate() {
        addr.sun_path[i] = b as c_char;
    }
    let len = (2 + bytes.len() + 1) as u32;
    Ok((addr, len))
}

pub fn cstr_to_string(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    unsafe { Some(CStr::from_ptr(ptr).to_string_lossy().into_owned()) }
}

pub fn send_signal(pid: i32, sig: i32) -> io::Result<()> {
    if unsafe { kill(pid, sig) } < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
