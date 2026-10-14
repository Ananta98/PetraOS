// SPDX-License-Identifier: GPL-2.0

//! Userspace pointer utilities.

use ostd::mm::{io::FallibleVmRead, FallibleVmWrite, PAGE_SIZE};

use crate::{
    api::errno::{Errno, Result},
    vm::{
        perms::VmPerms,
        vmar::{page_fault::PageFaultInfo, Vmar},
    },
};

/// A safe wrapper representing a pointer to userspace memory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UserPtr {
    addr: usize,
}

impl UserPtr {
    /// Creates a new `UserPtr` wrapping `addr`.
    pub const fn new(addr: usize) -> Self {
        Self { addr }
    }

    /// Returns the raw userspace virtual address.
    pub const fn addr(&self) -> usize {
        self.addr
    }

    /// Returns `true` if the pointer is null (`0`).
    pub const fn is_null(&self) -> bool {
        self.addr == 0
    }

    /// Safely writes bytes to userspace memory at this pointer.
    pub fn write_bytes(&self, vmar: &Vmar, data: &[u8]) -> Result<()> {
        if self.is_null() {
            return Err(Errno::EFAULT);
        }
        if data.is_empty() {
            return Ok(());
        }
        vmar.activate();
        let end_addr = self.addr.checked_add(data.len()).ok_or(Errno::EFAULT)?;
        let page_start = self.addr & !(PAGE_SIZE - 1);
        let page_end = end_addr
            .checked_add(PAGE_SIZE - 1)
            .ok_or(Errno::EFAULT)?
            & !(PAGE_SIZE - 1);

        let res = (|| -> Result<()> {
            let mut writer = vmar
                .vm_space()
                .writer(self.addr, data.len())
                .map_err(|_| Errno::EFAULT)?;
            let mut reader = ostd::mm::VmReader::from(data);
            writer.write_fallible(&mut reader).map_err(|_| Errno::EFAULT)?;
            Ok(())
        })();

        if res.is_err() {
            for page in (page_start..page_end).step_by(PAGE_SIZE) {
                let _ = vmar.handle_page_fault(&PageFaultInfo::new(page, VmPerms::WRITE));
            }
            vmar.activate();
            let mut writer = vmar
                .vm_space()
                .writer(self.addr, data.len())
                .map_err(|_| Errno::EFAULT)?;
            let mut reader = ostd::mm::VmReader::from(data);
            writer.write_fallible(&mut reader).map_err(|_| Errno::EFAULT)?;
        }
        Ok(())
    }

    /// Safely reads bytes from userspace memory at this pointer into `buf`.
    pub fn read_bytes(&self, vmar: &Vmar, buf: &mut [u8]) -> Result<()> {
        if self.is_null() {
            return Err(Errno::EFAULT);
        }
        if buf.is_empty() {
            return Ok(());
        }
        vmar.activate();
        let end_addr = self.addr.checked_add(buf.len()).ok_or(Errno::EFAULT)?;
        let page_start = self.addr & !(PAGE_SIZE - 1);
        let page_end = end_addr
            .checked_add(PAGE_SIZE - 1)
            .ok_or(Errno::EFAULT)?
            & !(PAGE_SIZE - 1);

        let res = (|| -> Result<()> {
            let mut reader = vmar
                .vm_space()
                .reader(self.addr, buf.len())
                .map_err(|_| Errno::EFAULT)?;
            let mut writer = ostd::mm::VmWriter::from(&mut buf[..]);
            reader.read_fallible(&mut writer).map_err(|_| Errno::EFAULT)?;
            Ok(())
        })();

        if res.is_err() {
            for page in (page_start..page_end).step_by(PAGE_SIZE) {
                let _ = vmar.handle_page_fault(&PageFaultInfo::new(page, VmPerms::READ));
            }
            vmar.activate();
            let mut reader = vmar
                .vm_space()
                .reader(self.addr, buf.len())
                .map_err(|_| Errno::EFAULT)?;
            let mut writer = ostd::mm::VmWriter::from(&mut buf[..]);
            reader.read_fallible(&mut writer).map_err(|_| Errno::EFAULT)?;
        }
        Ok(())
    }

}

/// Safely writes bytes to userspace memory.
pub fn write_user_bytes(vmar: &Vmar, user_ptr: usize, data: &[u8]) -> Result<()> {
    UserPtr::new(user_ptr).write_bytes(vmar, data)
}

/// Safely reads bytes from userspace memory.
pub fn read_user_bytes(vmar: &Vmar, user_ptr: usize, buf: &mut [u8]) -> Result<()> {
    UserPtr::new(user_ptr).read_bytes(vmar, buf)
}
