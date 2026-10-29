// SPDX-License-Identifier: GPL-2.0

use alloc::sync::Arc;
use ostd::arch::cpu::context::UserContext;

use crate::{
    uapi::errno::{Errno, Result},
    fs::vfs::OpenFlags,
    proc::thread::Thread,
    vm::{
        backing::{Backing, FileBacking},
        flags::MmapFlags,
        perms::VmPerms,
    },
};

pub fn sys_mmap(
    addr: usize,
    len: usize,
    prot_val: u32,
    flags_val: u32,
    fd: i32,
    offset: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;
    let vmar = proc.vmspace();

    let mut perms = VmPerms::empty();
    if (prot_val & 1) != 0 {
        perms |= VmPerms::READ;
    }
    if (prot_val & 2) != 0 {
        perms |= VmPerms::WRITE;
    }
    if (prot_val & 4) != 0 {
        perms |= VmPerms::EXEC;
    }

    let mut flags = MmapFlags::empty();
    if (flags_val & 0x01) != 0 {
        flags |= MmapFlags::SHARED;
    }
    if (flags_val & 0x02) != 0 {
        flags |= MmapFlags::PRIVATE;
    }
    if (flags_val & 0x10) != 0 {
        flags |= MmapFlags::FIXED;
    }
    if (flags_val & 0x20) != 0 {
        flags |= MmapFlags::ANONYMOUS;
    }
    if (flags_val & 0x0100) != 0 {
        flags |= MmapFlags::GROWSDOWN;
    }
    if (flags_val & 0x8000) != 0 {
        flags |= MmapFlags::POPULATE;
    }
    if (flags_val & 0x0010_0000) != 0 {
        flags |= MmapFlags::FIXED_NOREPLACE;
    }
    #[cfg(target_arch = "x86_64")]
    if (flags_val & 0x40) != 0 {
        flags |= MmapFlags::MAP_32BIT;
    }

    // Default to MAP_PRIVATE if neither is specified
    if !flags.contains(MmapFlags::SHARED) && !flags.contains(MmapFlags::PRIVATE) {
        flags |= MmapFlags::PRIVATE;
    }

    if flags.contains(MmapFlags::ANONYMOUS) || fd < 0 {
        let res = vmar.mmap_anonymous(addr, len, perms, flags)?;
        Ok(res)
    } else {
        let file = proc.file(fd)?;

        // Linux rule: MAP_SHARED with PROT_WRITE requires the file to be opened for writing
        if flags.contains(MmapFlags::SHARED) && perms.contains(VmPerms::WRITE) {
            let open_flags = file.flags.lock();
            if !open_flags.contains(OpenFlags::WRITE) {
                return Err(Errno::EACCES);
            }
        }

        // Validate memory mapping request with the file description
        file.mmap(offset as u64, len)?;

        // Devices with special mapping semantics (DRM cards resolving GEM
        // grants) override the default file page cache backing.
        let backing: Arc<dyn Backing> = if let Some(b) = file.mmap_backing(offset as u64, len)? {
            b
        } else {
            let min_size = offset.saturating_add(len);
            Arc::new(FileBacking::new_with_min_size(file, min_size)?)
        };
        let res = vmar.mmap_backed(&backing, addr, len, perms, flags, offset)?;
        Ok(res)
    }
}
