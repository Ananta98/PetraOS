// SPDX-License-Identifier: GPL-2.0

//! `brk(2)` system call.

use ostd::{arch::cpu::context::UserContext, mm::PAGE_SIZE};

use crate::{
    api::{
        errno::{Result},
        limit::{RLIM_INFINITY, Resource},
    },
    proc::thread::Thread,
    vm::{VMAR_CAP_ADDR, flags::MmapFlags, perms::VmPerms},
};

/// System call `brk(addr)` (Syscall #12 on x86_64).
///
/// Sets the end of the data segment to the value specified by `new_brk`.
/// On success, Linux returns the new program break.
/// On failure, Linux returns the current break.
/// Calling `brk(0)` queries the current break.
pub fn sys_brk(new_brk: usize, _ctx: &mut UserContext) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;
    let vmar = proc.vmspace();

    let (start_brk, current_brk) = {
        let s = vmar.start_brk();
        let c = vmar.current_brk();
        if s == 0 && c == 0 && new_brk != 0 {
            vmar.init_brk(new_brk);
            (new_brk, new_brk)
        } else {
            (s, c)
        }
    };

    // If new_brk is 0 or less than the start break, return the current break.
    if new_brk == 0 || new_brk < start_brk {
        return Ok(current_brk);
    }

    if new_brk == current_brk {
        return Ok(current_brk);
    }

    // Check RLIMIT_DATA resource limit
    let rlim_data = proc.limit().get(Resource::Data).rlim_cur;
    if rlim_data != RLIM_INFINITY {
        let data_size = new_brk.saturating_sub(start_brk);
        if data_size as u64 > rlim_data {
            return Ok(current_brk);
        }
    }

    // Must not exceed the maximum userspace virtual address
    if new_brk > VMAR_CAP_ADDR {
        return Ok(current_brk);
    }

    if new_brk > current_brk {
        let old_page_end = match current_brk.checked_add(PAGE_SIZE - 1) {
            Some(val) => val & !(PAGE_SIZE - 1),
            None => return Ok(current_brk),
        };
        let new_page_end = match new_brk.checked_add(PAGE_SIZE - 1) {
            Some(val) => val & !(PAGE_SIZE - 1),
            None => return Ok(current_brk),
        };

        if new_page_end > old_page_end {
            let grow_range = old_page_end..new_page_end;
            if vmar.has_overlap(&grow_range) {
                return Ok(current_brk);
            }

            if vmar
                .mmap_anonymous(
                    old_page_end,
                    new_page_end - old_page_end,
                    VmPerms::READ | VmPerms::WRITE,
                    MmapFlags::PRIVATE | MmapFlags::FIXED | MmapFlags::ANONYMOUS,
                )
                .is_err()
            {
                return Ok(current_brk);
            }
        }

        vmar.set_current_brk(new_brk);
        Ok(new_brk)
    } else {
        // Shrinking the break
        let old_page_end = match current_brk.checked_add(PAGE_SIZE - 1) {
            Some(val) => val & !(PAGE_SIZE - 1),
            None => return Ok(current_brk),
        };
        let new_page_end = match new_brk.checked_add(PAGE_SIZE - 1) {
            Some(val) => val & !(PAGE_SIZE - 1),
            None => return Ok(current_brk),
        };

        if new_page_end < old_page_end {
            let shrink_range = new_page_end..old_page_end;
            let _ = vmar.munmap(shrink_range);
        }

        vmar.set_current_brk(new_brk);
        Ok(new_brk)
    }
}
