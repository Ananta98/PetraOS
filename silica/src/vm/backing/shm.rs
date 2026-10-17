// SPDX-License-Identifier: GPL-2.0

//! Shared memory objects.
//!
//! A [`SharedPages`] object owns the physical pages that back it, keyed by
//! offset. Two mappings of the same object that cover the same offset install
//! the same frame, so a write through one is immediately visible through the
//! other, and the object survives for exactly as long as it is reachable —
//! which is what `fork(2)` inheritance and `MAP_SHARED` need.

use alloc::{
    collections::BTreeMap,
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};
use core::fmt::{self, Debug, Formatter};

use ostd::mm::{UFrame, io::util::HasVmReaderWriter};
use ostd::sync::SpinLock;

use crate::api::errno::Result;
use super::{Backing, alloc_zeroed_frame};

/// The longest name a shared memory object may have, matching `NAME_MAX`.
const NAME_MAX: usize = 255;

/// A fixed-size region of shared physical memory.
pub struct SharedPages {
    size: usize,
    /// Materialized pages, keyed by their offset in the object.
    pages: SpinLock<BTreeMap<usize, UFrame>>,
}

impl SharedPages {
    /// Creates an object `size` bytes long.
    pub fn new(size: usize) -> Result<Arc<Self>> {
        if size == 0 {
            crate::return_errno!(EINVAL, "a shared memory object cannot be empty");
        }
        Ok(Arc::new(Self {
            size,
            pages: SpinLock::new(BTreeMap::new()),
        }))
    }

    /// Returns the size of the object in bytes.
    pub fn size(&self) -> usize {
        self.size
    }

    /// Returns the number of pages of the object that have been materialized.
    pub fn resident_pages(&self) -> usize {
        self.pages.lock().len()
    }

    /// Returns the `u64` stored at `offset` in the object.
    pub fn peek_u64(&self, offset: usize) -> Result<u64> {
        let frame = self.frame_at(offset)?;
        let mut reader = frame.reader();
        Ok(reader.read_val()?)
    }

    /// Returns the page at `offset`, materializing it if this is its first use.
    fn frame_at(&self, offset: usize) -> Result<UFrame> {
        if offset >= self.size {
            crate::return_errno!(EINVAL, "offset {offset:#x} is past the end of the object");
        }

        let mut pages = self.pages.lock();
        if let Some(frame) = pages.get(&offset) {
            return Ok(frame.clone());
        }

        let frame = alloc_zeroed_frame()?;
        pages.insert(offset, frame.clone());
        Ok(frame)
    }
}

impl Backing for SharedPages {
    fn size(&self) -> usize {
        self.size
    }

    fn frame(&self, offset: usize) -> Result<UFrame> {
        self.frame_at(offset)
    }
}

impl Debug for SharedPages {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let resident = self.pages.lock().len();
        f.debug_struct("SharedPages")
            .field("size", &self.size)
            .field("resident_pages", &resident)
            .finish()
    }
}

/// Creates the unnamed object that backs `MAP_SHARED | MAP_ANONYMOUS`.
pub fn new_anonymous(size: usize) -> Result<Arc<SharedPages>> {
    SharedPages::new(size)
}

/// A named shared memory object.
type Registry = SpinLock<Vec<(String, Arc<SharedPages>)>>;

static REGISTRY: Registry = SpinLock::new(Vec::new());

/// Checks that `name` is usable as a shared memory object name.
fn check_name(name: &str) -> Result<()> {
    if name.len() > NAME_MAX {
        crate::return_errno!(
            ENAMETOOLONG,
            "shared memory name is longer than {NAME_MAX} bytes"
        );
    }
    if name.is_empty() || !name.starts_with('/') {
        crate::return_errno!(EINVAL, "a shared memory name must start with '/'");
    }
    if name[1..].contains('/') {
        crate::return_errno!(
            EINVAL,
            "a shared memory name must not contain a '/' after the first"
        );
    }
    Ok(())
}

/// Creates a new named object of `size` bytes.
pub fn create(name: &str, size: usize) -> Result<Arc<SharedPages>> {
    check_name(name)?;
    let object = SharedPages::new(size)?;

    let mut registry = REGISTRY.lock();
    if registry.iter().any(|(existing, _)| existing == name) {
        crate::return_errno!(EEXIST, "shared memory object {name} already exists");
    }
    registry.push((name.to_string(), object.clone()));
    Ok(object)
}

/// Opens an existing named object.
pub fn open(name: &str) -> Result<Arc<SharedPages>> {
    check_name(name)?;

    let registry = REGISTRY.lock();
    let object = registry
        .iter()
        .find(|(existing, _)| existing == name)
        .map(|(_, object)| object.clone());
    match object {
        Some(object) => Ok(object),
        None => crate::return_errno!(ENOENT, "no shared memory object named {name}"),
    }
}

/// Removes a name from the shared memory namespace.
pub fn unlink(name: &str) -> Result<()> {
    check_name(name)?;

    let mut registry = REGISTRY.lock();
    match registry.iter().position(|(existing, _)| existing == name) {
        Some(index) => {
            registry.remove(index);
            Ok(())
        }
        None => crate::return_errno!(ENOENT, "no shared memory object named {name}"),
    }
}

/// Returns the names of all registered shared memory objects.
pub fn names() -> Vec<String> {
    REGISTRY
        .lock()
        .iter()
        .map(|(name, _)| name.clone())
        .collect()
}
