// SPDX-License-Identifier: GPL-2.0

//! Userspace C-style string utilities.

use alloc::{string::String, vec::Vec};

use super::user_ptr::UserPtr;
use crate::{
    api::errno::{Errno, Result},
    vm::vmar::Vmar,
};

/// A safe wrapper representing a pointer to a null-terminated C string in userspace memory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UserCStr {
    addr: usize,
}

impl UserCStr {
    /// Creates a new `UserCStr` wrapping `addr`.
    pub const fn new(addr: usize) -> Self {
        Self { addr }
    }

    /// Safely copies a null-terminated string from userspace memory up to `max_len` bytes.
    pub fn read_string(&self, vmar: &Vmar, max_len: usize) -> Result<String> {
        if self.addr == 0 {
            return Err(Errno::EFAULT);
        }
        let mut bytes = Vec::new();
        let mut buf = [0u8; 64];
        let mut cur_addr = self.addr;

        while bytes.len() < max_len {
            // Never cross a page boundary: the next page may be unmapped even
            // though the string terminates before it.
            let to_page_end = 4096 - (cur_addr & 4095);
            let chunk_size = 64.min(max_len - bytes.len()).min(to_page_end);
            UserPtr::new(cur_addr).read_bytes(vmar, &mut buf[..chunk_size])?;

            for &byte in buf.iter().take(chunk_size) {
                if byte == 0 {
                    let s = core::str::from_utf8(&bytes).map_err(|_| Errno::EINVAL)?;
                    return Ok(String::from(s));
                }
                bytes.push(byte);
            }
            cur_addr = cur_addr.checked_add(chunk_size).ok_or(Errno::EFAULT)?;
        }

        Err(Errno::ENAMETOOLONG)
    }

    /// Reads a null-terminated array of string pointers from userspace (e.g. `argv`, `envp`).
    pub fn read_string_array(
        vmar: &Vmar,
        mut arr_ptr: usize,
        max_count: usize,
    ) -> Result<Vec<String>> {
        if arr_ptr == 0 {
            return Ok(Vec::new());
        }
        let mut strings = Vec::new();
        let ptr_size = core::mem::size_of::<usize>();

        for _ in 0..max_count {
            let mut ptr_bytes = [0u8; core::mem::size_of::<usize>()];
            UserPtr::new(arr_ptr).read_bytes(vmar, &mut ptr_bytes)?;

            let str_ptr = usize::from_ne_bytes(ptr_bytes);
            if str_ptr == 0 {
                break;
            }
            let s = UserCStr::new(str_ptr).read_string(vmar, 4096)?;
            strings.push(s);
            arr_ptr = arr_ptr.checked_add(ptr_size).ok_or(Errno::EFAULT)?;
        }
        Ok(strings)
    }
}

/// Safely copies a null-terminated string from userspace memory.
pub fn read_user_string(vmar: &Vmar, user_ptr: usize, max_len: usize) -> Result<String> {
    UserCStr::new(user_ptr).read_string(vmar, max_len)
}
