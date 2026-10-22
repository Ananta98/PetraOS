// SPDX-License-Identifier: GPL-2.0

//! Userspace pointer utilities.

use core::ops::Range;

use ostd::mm::{FallibleVmWrite, MAX_USERSPACE_VADDR, PAGE_SIZE, io::FallibleVmRead};
use zerocopy::{FromBytes, Immutable, IntoBytes};

use super::user_cstr::UserCStr;
use crate::{
    api::errno::{Errno, Result},
    vm::{
        perms::VmPerms,
        vmar::{Vmar, page_fault::PageFaultInfo},
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

    /// Converts this pointer into a `UserCStr`.
    pub const fn as_cstr(self) -> UserCStr {
        UserCStr::new(self.addr)
    }

    /// Safely copies a null-terminated string from userspace memory up to `max_len` bytes.
    pub fn read_string(&self, vmar: &Vmar, max_len: usize) -> Result<alloc::string::String> {
        self.as_cstr().read_string(vmar, max_len)
    }

    /// Helper to perform a memory access, falling back to activating `vmar`
    /// and resolving page faults across the requested range if the access fails.
    fn access_with_fault_handling<F>(
        &self,
        vmar: &Vmar,
        range: Range<usize>,
        perms: VmPerms,
        mut op: F,
    ) -> Result<()>
    where
        F: FnMut() -> Result<()>,
    {
        if op().is_ok() {
            return Ok(());
        }

        // On failure, activate the address space and handle page faults for the range.
        vmar.activate();
        let page_start = range.start & !(PAGE_SIZE - 1);
        let page_end =
            range.end.checked_add(PAGE_SIZE - 1).ok_or(Errno::EFAULT)? & !(PAGE_SIZE - 1);

        for page in (page_start..page_end).step_by(PAGE_SIZE) {
            let _ = vmar.handle_page_fault(&PageFaultInfo::new(page, perms));
        }

        op()
    }

    /// Safely writes bytes to userspace memory at this pointer.
    pub fn write_bytes(&self, vmar: &Vmar, data: &[u8]) -> Result<()> {
        if self.is_null() {
            return Err(Errno::EFAULT);
        }
        if data.is_empty() {
            return Ok(());
        }
        let end_addr = self.addr.checked_add(data.len()).ok_or(Errno::EFAULT)?;
        if end_addr > MAX_USERSPACE_VADDR {
            return Err(Errno::EFAULT);
        }

        self.access_with_fault_handling(vmar, self.addr..end_addr, VmPerms::WRITE, || {
            let mut writer = vmar
                .vm_space()
                .writer(self.addr, data.len())
                .map_err(|_| Errno::EFAULT)?;
            let mut reader = ostd::mm::VmReader::from(data);
            writer
                .write_fallible(&mut reader)
                .map_err(|_| Errno::EFAULT)?;
            Ok(())
        })
    }

    /// Safely reads bytes from userspace memory at this pointer into `buf`.
    pub fn read_bytes(&self, vmar: &Vmar, buf: &mut [u8]) -> Result<()> {
        if self.is_null() {
            return Err(Errno::EFAULT);
        }
        if buf.is_empty() {
            return Ok(());
        }
        let end_addr = self.addr.checked_add(buf.len()).ok_or(Errno::EFAULT)?;
        if end_addr > MAX_USERSPACE_VADDR {
            return Err(Errno::EFAULT);
        }

        self.access_with_fault_handling(vmar, self.addr..end_addr, VmPerms::READ, || {
            let mut reader = vmar
                .vm_space()
                .reader(self.addr, buf.len())
                .map_err(|_| Errno::EFAULT)?;
            let mut writer = ostd::mm::VmWriter::from(&mut buf[..]);
            reader
                .read_fallible(&mut writer)
                .map_err(|_| Errno::EFAULT)?;
            Ok(())
        })
    }

    /// Copies a plain-data value from userspace memory.
    ///
    /// Type bounds are zerocopy's so there are no reads of invalid bit
    /// patterns and the kernel never holds a reference into user memory.
    pub fn read_val<T: FromBytes>(&self, vmar: &Vmar) -> Result<T> {
        let mut bytes = alloc::vec![0u8; core::mem::size_of::<T>()];
        self.read_bytes(vmar, &mut bytes)?;
        // Can only fail on a short source, which cannot happen: `bytes` is
        // exactly `size_of::<T>()`.
        T::read_from_bytes(&bytes).map_err(|_| Errno::EFAULT)
    }

    /// Copies a plain-data value to userspace memory.
    pub fn write_val<T: IntoBytes + Immutable>(&self, vmar: &Vmar, val: T) -> Result<()> {
        self.write_bytes(vmar, val.as_bytes())
    }

    /// Reads a value if the pointer is non-null; `Ok(None)` otherwise.
    ///
    /// Encodes the kernel's optional-pointer idiom (e.g. ioctl `out` pointers
    /// and optional `loff_t` arguments are only written/read when present).
    pub fn read_opt<T: FromBytes>(&self, vmar: &Vmar) -> Result<Option<T>> {
        if self.is_null() {
            return Ok(None);
        }
        self.read_val(vmar).map(Some)
    }

    /// Writes a value if the pointer is non-null; a null pointer is a no-op.
    pub fn write_opt<T: IntoBytes + Immutable>(&self, vmar: &Vmar, val: T) -> Result<()> {
        if self.is_null() {
            return Ok(());
        }
        self.write_val(vmar, val)
    }
}

impl From<usize> for UserPtr {
    fn from(addr: usize) -> Self {
        Self::new(addr)
    }
}

impl From<UserPtr> for usize {
    fn from(ptr: UserPtr) -> Self {
        ptr.addr()
    }
}
