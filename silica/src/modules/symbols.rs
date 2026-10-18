// SPDX-License-Identifier: GPL-2.0

//! Kernel symbol table used to link loadable modules against the kernel.

use alloc::{borrow::ToOwned, collections::btree_map::BTreeMap, string::String};
use spin::Mutex;

/// Global symbol table for resolving module symbols.
pub static SYMBOL_TABLE: Mutex<BTreeMap<String, usize>> = Mutex::new(BTreeMap::new());

/// Registers a kernel symbol so loadable modules can resolve against it.
pub fn register_symbol(name: &str, address: usize) {
    let mut table = SYMBOL_TABLE.lock();
    table.insert(name.to_owned(), address);
}

/// Resolves a symbol name to its runtime kernel virtual address.
pub fn lookup_symbol(name: &str) -> Option<usize> {
    let table = SYMBOL_TABLE.lock();
    table.get(name).copied()
}

/// Registers the curated set of kernel symbols that loadable modules may
/// link against (the analogue of Linux `EXPORT_SYMBOL`).
///
/// Extend this list as more subsystems become linkable from modules.
pub(crate) fn register_core_symbols() {
    use crate::drivers::block::register_block_device;
    use crate::drivers::{register_blkdev, register_chrdev, register_netdev};

    crate::export_symbol!(register_chrdev);
    crate::export_symbol!(register_blkdev);
    crate::export_symbol!(register_netdev);
    crate::export_symbol!(register_block_device);
    crate::export_symbol!(lookup_symbol);
}
