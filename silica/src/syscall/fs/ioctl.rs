// SPDX-License-Identifier: GPL-2.0

use ostd::arch::cpu::context::UserContext;
use crate::{
    api::errno::{Errno, Result},
    proc::thread::Thread,
};

pub const FIONBIO: u32 = 0x5421;

pub fn sys_ioctl(fd: i32, cmd: u32, arg: usize, _ctx: &mut UserContext) -> Result<usize> {
    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let file = proc.fd_table.lock().get(fd)?;

    if cmd == FIONBIO {
        let vmar = proc.vmspace();
        let mut val_bytes = [0u8; 4];
        let mut reader = vmar.vm_space().reader(arg, 4).map_err(|_| Errno::EFAULT)?;
        let mut writer = ostd::mm::VmWriter::from(&mut val_bytes[..]);
        use ostd::mm::io::FallibleVmRead;
        reader.read_fallible(&mut writer).map_err(|_| Errno::EFAULT)?;
        let on = i32::from_ne_bytes(val_bytes) != 0;
        let mut flags = file.flags.lock();
        if on {
            *flags |= crate::fs::vfs::OpenFlags::NONBLOCK;
        } else {
            *flags &= !crate::fs::vfs::OpenFlags::NONBLOCK;
        }
        if let Some(sock) = file.as_socket() {
            *sock.nonblocking.lock() = on;
        }
        return Ok(0);
    }

    file.ioctl(cmd, arg)
}
