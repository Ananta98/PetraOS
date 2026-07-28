use crate::proc::process::Process;
use crate::syscall::{SyscallResult, to_continue_i32};
use crate::vm::vma::VmaManager;
use ostd::Error;
use ostd::arch::cpu::context::UserContext;

#[derive(Default, Copy, Clone)]
pub struct Msghdr {
    pub msg_name: usize,
    pub msg_namelen: u32,
    pub msg_iov: usize,
    pub msg_iovlen: usize,
    pub msg_control: usize,
    pub msg_controllen: usize,
    pub msg_flags: i32,
}

impl Msghdr {
    pub fn read_from_user(vm: &VmaManager, user_ptr: usize) -> Result<Self, Error> {
        let mut buf = [0u8; 56];
        vm.copy_from_user(user_ptr, &mut buf)?;
        let mut ptr_bytes = [0u8; 8];
        let mut len_bytes = [0u8; 8];
        let mut u32_bytes = [0u8; 4];
        let mut i32_bytes = [0u8; 4];

        ptr_bytes.copy_from_slice(&buf[0..8]);
        let msg_name = usize::from_ne_bytes(ptr_bytes);

        u32_bytes.copy_from_slice(&buf[8..12]);
        let msg_namelen = u32::from_ne_bytes(u32_bytes);

        ptr_bytes.copy_from_slice(&buf[16..24]);
        let msg_iov = usize::from_ne_bytes(ptr_bytes);

        len_bytes.copy_from_slice(&buf[24..32]);
        let msg_iovlen = usize::from_ne_bytes(len_bytes);

        ptr_bytes.copy_from_slice(&buf[32..40]);
        let msg_control = usize::from_ne_bytes(ptr_bytes);

        len_bytes.copy_from_slice(&buf[40..48]);
        let msg_controllen = usize::from_ne_bytes(len_bytes);

        i32_bytes.copy_from_slice(&buf[48..52]);
        let msg_flags = i32::from_ne_bytes(i32_bytes);

        Ok(Self {
            msg_name,
            msg_namelen,
            msg_iov,
            msg_iovlen,
            msg_control,
            msg_controllen,
            msg_flags,
        })
    }
}

#[derive(Default, Copy, Clone)]
pub struct Iovec {
    pub iov_base: usize,
    pub iov_len: usize,
}

impl Iovec {
    pub fn read_from_user(vm: &VmaManager, user_ptr: usize) -> Result<Self, Error> {
        let mut buf = [0u8; 16];
        vm.copy_from_user(user_ptr, &mut buf)?;
        let mut ptr_bytes = [0u8; 8];
        let mut len_bytes = [0u8; 8];
        ptr_bytes.copy_from_slice(&buf[0..8]);
        len_bytes.copy_from_slice(&buf[8..16]);
        Ok(Self {
            iov_base: usize::from_ne_bytes(ptr_bytes),
            iov_len: usize::from_ne_bytes(len_bytes),
        })
    }
}

/// `sendmsg()` — SYS_sendmsg = 46
pub fn syscall_sendmsg(
    arg0: usize,
    arg1: usize,
    _flags: usize,
    _: usize,
    _: usize,
    _: usize,
    vm: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    let fd = arg0 as i32;
    let msg_ptr = arg1;
    if msg_ptr == 0 {
        return to_continue_i32(Err(Error::InvalidArgs));
    }

    let proc = Process::current();
    let fd_table = proc.fd_table.lock();
    let fd_entry = match fd_table.get_fd(fd) {
        Ok(f) => f,
        Err(e) => return to_continue_i32(Err(e)),
    };

    let msg = match Msghdr::read_from_user(vm, msg_ptr) {
        Ok(m) => m,
        Err(e) => return to_continue_i32(Err(e)),
    };

    let mut total_sent = 0usize;
    let iov_len = msg.msg_iovlen;
    let iov_ptr = msg.msg_iov;
    let mut open_file = fd_entry.open_file.lock();

    for i in 0..iov_len {
        let entry_ptr = iov_ptr + i * 16;
        let iov = match Iovec::read_from_user(vm, entry_ptr) {
            Ok(v) => v,
            Err(e) => return to_continue_i32(Err(e)),
        };

        if iov.iov_base != 0 && iov.iov_len > 0 {
            let mut buf = alloc::vec![0u8; iov.iov_len];
            if vm.copy_from_user(iov.iov_base, &mut buf).is_err() {
                return to_continue_i32(Err(Error::InvalidArgs));
            }
            let mut offset = 0;
            match open_file.file_ops.write(&buf, &mut offset) {
                Ok(written) => total_sent += written,
                Err(e) => return to_continue_i32(Err(e)),
            }
        }
    }

    to_continue_i32(Ok(total_sent as i32))
}

/// `recvmsg()` — SYS_recvmsg = 47
pub fn syscall_recvmsg(
    arg0: usize,
    arg1: usize,
    _flags: usize,
    _: usize,
    _: usize,
    _: usize,
    vm: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    let fd = arg0 as i32;
    let msg_ptr = arg1;
    if msg_ptr == 0 {
        return to_continue_i32(Err(Error::InvalidArgs));
    }

    let proc = Process::current();
    let fd_table = proc.fd_table.lock();
    let fd_entry = match fd_table.get_fd(fd) {
        Ok(f) => f,
        Err(e) => return to_continue_i32(Err(e)),
    };

    let msg = match Msghdr::read_from_user(vm, msg_ptr) {
        Ok(m) => m,
        Err(e) => return to_continue_i32(Err(e)),
    };

    let mut total_recv = 0usize;
    let iov_len = msg.msg_iovlen;
    let iov_ptr = msg.msg_iov;
    let mut open_file = fd_entry.open_file.lock();

    for i in 0..iov_len {
        let entry_ptr = iov_ptr + i * 16;
        let iov = match Iovec::read_from_user(vm, entry_ptr) {
            Ok(v) => v,
            Err(e) => return to_continue_i32(Err(e)),
        };

        if iov.iov_base != 0 && iov.iov_len > 0 {
            let mut buf = alloc::vec![0u8; iov.iov_len];
            let mut offset = 0;
            match open_file.file_ops.read(&mut buf, &mut offset) {
                Ok(nread) => {
                    if vm.copy_to_user(iov.iov_base, &buf[..nread]).is_err() {
                        return to_continue_i32(Err(Error::InvalidArgs));
                    }
                    total_recv += nread;
                }
                Err(e) => return to_continue_i32(Err(e)),
            }
        }
    }

    to_continue_i32(Ok(total_recv as i32))
}
