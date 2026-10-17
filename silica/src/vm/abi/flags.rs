// SPDX-License-Identifier: GPL-2.0

//! User-space arguments of the virtual memory system calls.
//!
//! Each type here corresponds to one system call's flag word. Parsing is strict:
//! an unknown bit is [`Errno::EINVAL`], which is what Linux does for the flag
//! words that these types mirror.

use bitflags::bitflags;

use crate::api::errno::Result;

bitflags! {
    /// The `flags` argument of `mmap(2)`.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub struct MmapFlags: u32 {
        /// `MAP_SHARED`: writes are carried through to the backing object and
        /// are visible to every other mapping of it.
        const SHARED          = 0x0000_0001;
        /// `MAP_PRIVATE`: writes are private to this mapping.
        const PRIVATE         = 0x0000_0002;
        /// `MAP_FIXED`: place the mapping at the given address, replacing
        /// whatever was there.
        const FIXED           = 0x0000_0010;
        /// `MAP_ANONYMOUS`: the mapping is not backed by a file.
        const ANONYMOUS       = 0x0000_0020;
        /// `MAP_GROWSDOWN`: let a fault below the mapping extend it downwards.
        const GROWSDOWN       = 0x0000_0100;
        /// `MAP_LOCKED`: lock the mapping into RAM, preventing paging/reclaim.
        const LOCKED          = 0x0000_2000;
        /// `MAP_POPULATE`: fault the whole mapping in before returning.
        const POPULATE        = 0x0000_8000;
        /// `MAP_FIXED_NOREPLACE`: like `MAP_FIXED`, but fail rather than
        /// replace an existing mapping.
        const FIXED_NOREPLACE = 0x0010_0000;
    }
}

impl MmapFlags {
    /// `MAP_32BIT`: place the mapping in the first 2 GiB of the address space.
    #[cfg(target_arch = "x86_64")]
    pub const MAP_32BIT: Self = Self::from_bits_retain(0x0000_0040);

    const KNOWN_BASE: Self = Self::SHARED
        .union(Self::PRIVATE)
        .union(Self::FIXED)
        .union(Self::ANONYMOUS)
        .union(Self::GROWSDOWN)
        .union(Self::LOCKED)
        .union(Self::POPULATE)
        .union(Self::FIXED_NOREPLACE);

    /// Every bit this architecture assigns, which is what `from_raw` checks the
    /// argument against.
    #[cfg(target_arch = "x86_64")]
    const KNOWN: Self = Self::KNOWN_BASE.union(Self::MAP_32BIT);
    #[cfg(not(target_arch = "x86_64"))]
    const KNOWN: Self = Self::KNOWN_BASE;

    /// Parses the `flags` argument of `mmap(2)`.
    pub fn from_raw(raw: u32) -> Result<Self> {
        if raw & !Self::KNOWN.bits() != 0 {
            crate::return_errno!(
                EINVAL,
                "the mmap flags {raw:#x} have bits outside {:#x}",
                Self::KNOWN.bits()
            );
        }
        let flags = Self::from_bits_retain(raw);
        flags.validate()?;
        Ok(flags)
    }

    /// Rejects flag combinations that Linux also rejects.
    pub(crate) fn validate(self) -> Result<()> {
        if self.contains(Self::SHARED) && self.contains(Self::PRIVATE) {
            crate::return_errno!(EINVAL, "MAP_SHARED and MAP_PRIVATE are mutually exclusive");
        }
        if self.contains(Self::FIXED) && self.contains(Self::FIXED_NOREPLACE) {
            crate::return_errno!(
                EINVAL,
                "MAP_FIXED and MAP_FIXED_NOREPLACE are mutually exclusive"
            );
        }
        Ok(())
    }

    /// Returns whether the mapping is shared with other mappings of the same
    /// backing object.
    pub fn is_shared(self) -> bool {
        self.contains(Self::SHARED)
    }

    /// Returns whether the mapping was requested as anonymous.
    pub fn is_anonymous(self) -> bool {
        self.contains(Self::ANONYMOUS)
    }

    /// Returns whether a fault just below the mapping should extend it downwards.
    pub fn grows_down(self) -> bool {
        self.contains(Self::GROWSDOWN)
    }

    /// Returns whether the mapping requested pages to be locked in memory.
    pub fn is_locked(self) -> bool {
        self.contains(Self::LOCKED)
    }
}

/// The `advice` argument of `madvise(2)`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u32)]
pub enum MadviseAdvice {
    /// `MADV_NORMAL`: no special treatment.
    Normal = 0,
    /// `MADV_RANDOM`: expect page-readahead-disabled access.
    Random = 1,
    /// `MADV_SEQUENTIAL`: expect aggressive read-ahead.
    Sequential = 2,
    /// `MADV_WILLNEED`: fault the range in ahead of time.
    WillNeed = 3,
    /// `MADV_DONTNEED`: drop the pages; a later access gets a fresh page.
    DontNeed = 4,
    /// `MADV_FREE`: drop pages when under memory pressure.
    Free = 8,
    /// `MADV_DONTFORK`: do not copy this range on fork(2).
    DontFork = 10,
    /// `MADV_DOFORK`: restore copying this range on fork(2).
    DoFork = 11,
    /// `MADV_MERGEABLE`: KSM hint (transparently supported).
    Mergeable = 12,
    /// `MADV_UNMERGEABLE`: KSM unmerge hint.
    Unmergeable = 13,
    /// `MADV_HUGEPAGE`: THP hint.
    HugePage = 14,
    /// `MADV_NOHUGEPAGE`: disable THP hint.
    NoHugePage = 15,
    /// `MADV_DONTNEED_COLD`: like `MADV_DONTNEED`, but eager to reclaim.
    DontNeedCold = 16,
    /// `MADV_WIPEONFORK`: zero out this mapping in child on fork(2).
    WipeOnFork = 18,
    /// `MADV_KEEPONFORK`: restore standard fork behavior.
    KeepOnFork = 19,
}

impl MadviseAdvice {
    /// Parses the `advice` argument of `madvise(2)`.
    pub fn from_raw(raw: u32) -> Result<Self> {
        match raw {
            0 => Ok(Self::Normal),
            1 => Ok(Self::Random),
            2 => Ok(Self::Sequential),
            3 => Ok(Self::WillNeed),
            4 => Ok(Self::DontNeed),
            8 => Ok(Self::Free),
            10 => Ok(Self::DontFork),
            11 => Ok(Self::DoFork),
            12 => Ok(Self::Mergeable),
            13 => Ok(Self::Unmergeable),
            14 => Ok(Self::HugePage),
            15 => Ok(Self::NoHugePage),
            16 => Ok(Self::DontNeedCold),
            18 => Ok(Self::WipeOnFork),
            19 => Ok(Self::KeepOnFork),
            _ => crate::return_errno!(EINVAL, "unsupported madvise advice {raw}"),
        }
    }

    /// Returns whether the advice asks for the pages to be dropped.
    pub fn drops_pages(self) -> bool {
        matches!(self, Self::DontNeed | Self::Free | Self::DontNeedCold)
    }
}

bitflags! {
    /// The `flags` argument of `mremap(2)`.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub struct MremapFlags: u32 {
        /// `MREMAP_MAYMOVE`: the kernel may move the mapping elsewhere.
        const MAYMOVE   = 0x01;
        /// `MREMAP_FIXED`: move the mapping to the address passed as `new_address`.
        const FIXED     = 0x02;
        /// `MREMAP_DONTUNMAP`: keep the old address range mapped, sharing the
        /// same physical pages as the new range.
        const DONTUNMAP = 0x04;
    }
}

impl MremapFlags {
    const KNOWN: Self = Self::MAYMOVE.union(Self::FIXED).union(Self::DONTUNMAP);

    /// Parses the `flags` argument of `mremap(2)`.
    pub fn from_raw(raw: u32) -> Result<Self> {
        if raw & !Self::KNOWN.bits() != 0 {
            crate::return_errno!(EINVAL, "the mremap flags {raw:#x} are not all known");
        }
        let flags = Self::from_bits_retain(raw);
        if flags.contains(Self::FIXED) && flags.contains(Self::DONTUNMAP) {
            crate::return_errno!(EINVAL, "MREMAP_FIXED and MREMAP_DONTUNMAP conflict");
        }
        Ok(flags)
    }
}

bitflags! {
    /// The `flags` argument of `msync(2)`.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub struct MsyncFlags: u32 {
        /// `MS_ASYNC`: start the writeback and return without waiting for it.
        const ASYNC      = 0x01;
        /// `MS_INVALIDATE`: drop the mapping's cached view, so that a later
        /// access re-reads the object.
        const INVALIDATE = 0x02;
        /// `MS_SYNC`: wait for the writeback to finish.
        const SYNC       = 0x04;
    }
}

impl MsyncFlags {
    const KNOWN: Self = Self::ASYNC.union(Self::INVALIDATE).union(Self::SYNC);

    /// Parses the `flags` argument of `msync(2)`.
    pub fn from_raw(raw: u32) -> Result<Self> {
        if raw & !Self::KNOWN.bits() != 0 {
            crate::return_errno!(EINVAL, "unknown msync flags {raw:#x}");
        }
        let flags = Self::from_bits_retain(raw);
        flags.validate()?;
        Ok(flags)
    }

    /// Rejects invalid flag combinations.
    ///
    /// Per POSIX:
    /// - At least one flag must be given.
    /// - `MS_ASYNC` and `MS_SYNC` are mutually exclusive.
    /// - `MS_INVALIDATE` may be combined with `MS_ASYNC` or `MS_SYNC`.
    pub(crate) fn validate(self) -> Result<()> {
        if self.is_empty() {
            crate::return_errno!(
                EINVAL,
                "msync needs one of MS_ASYNC, MS_SYNC or MS_INVALIDATE"
            );
        }
        if self.contains(Self::ASYNC) && self.contains(Self::SYNC) {
            crate::return_errno!(
                EINVAL,
                "MS_ASYNC and MS_SYNC are mutually exclusive, got {:#x}",
                self.bits()
            );
        }
        let recognized = Self::ASYNC | Self::SYNC | Self::INVALIDATE;
        if !recognized.contains(self) {
            crate::return_errno!(
                EINVAL,
                "unknown msync flags {:#x}",
                self.bits()
            );
        }
        Ok(())
    }
}
