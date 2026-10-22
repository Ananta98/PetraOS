// SPDX-License-Identifier: GPL-2.0

use crate::utils::userspace::UserPtr;
use crate::{
    api::errno::Result,
    proc::thread::Thread,
};
use ostd::arch::cpu::context::UserContext;

pub const FIONBIO: u32 = 0x5421;

pub fn sys_ioctl(fd: i32, cmd: u32, arg: usize, _ctx: &mut UserContext) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;
    let file = proc.file(fd)?;

    if cmd == FIONBIO {
        let vmar = proc.vmspace();
        let on = i32::from(UserPtr::new(arg).read_val::<i32>(&vmar)?) != 0;
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
