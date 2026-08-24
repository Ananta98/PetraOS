//! Architecture-Independent Paging Subsystem for PetraOS.
//!
//! Defines the `PageTable` trait and hardware mapping abstractions.

pub mod address;
pub mod entry;
pub mod flags;

pub use address::{PhysAddr, VirtAddr};
pub use entry::PageTableEntry;
pub use flags::{COW_FLAG, PageFaultErrorCode, PageTableFlags};

/// Errors returned by page table manipulation operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PagingError {
    /// Physical frame allocator failed to provide a frame for page table structures.
    FrameAllocationFailed,
    /// The specified virtual or physical address is invalid or unaligned.
    InvalidAddress,
    /// Virtual page is already mapped.
    AlreadyMapped,
    /// Virtual page is not mapped.
    NotMapped,
    /// Page table flags update failed.
    FlagUpdateFailed,
    /// Huge page conflicts with requested operation.
    HugePageConflict,
}

/// Generic interface implemented by architecture-specific hardware page tables.
pub trait PageTable: Send + Sync {
    /// Create a new page table by allocating a root directory and copying kernel-space mappings.
    fn new() -> Result<Self, PagingError>
    where
        Self: Sized;

    /// Recreate a page table interface wrapper around an existing hardware page table root.
    ///
    /// # Safety
    /// The caller must ensure that `root` points to a valid page directory root (e.g., PML4 or PML5).
    unsafe fn from_root(root: PhysAddr) -> Self
    where
        Self: Sized;

    /// Get the physical address of the page table root directory.
    fn root(&self) -> PhysAddr;

    /// Map a virtual page to a physical frame.
    fn map(
        &mut self,
        page: VirtAddr,
        frame: PhysAddr,
        flags: PageTableFlags,
    ) -> Result<(), PagingError>;

    /// Map a contiguous range of virtual pages to physical frames.
    fn map_range(
        &mut self,
        page: VirtAddr,
        frame: PhysAddr,
        size: usize,
        flags: PageTableFlags,
    ) -> Result<(), PagingError>;

    /// Unmap a virtual page.
    fn unmap(&mut self, page: VirtAddr) -> Result<PhysAddr, PagingError>;

    /// Unmap a contiguous range of virtual pages.
    fn unmap_range(&mut self, page: VirtAddr, size: usize) -> Result<(), PagingError>;

    /// Remap a virtual page with new flags.
    fn remap(&mut self, page: VirtAddr, flags: PageTableFlags) -> Result<(), PagingError>;

    /// Remap a contiguous range of virtual pages with new flags.
    fn remap_range(
        &mut self,
        page: VirtAddr,
        size: usize,
        flags: PageTableFlags,
    ) -> Result<(), PagingError>;

    /// Translate a virtual address to its corresponding physical address.
    fn translate(&self, virt: VirtAddr) -> Option<PhysAddr>;

    /// Retrieve physical frame address and raw page entry flags for a virtual address.
    fn get_entry(&self, virt: VirtAddr) -> Option<(PhysAddr, PageTableFlags)>;

    /// Flush the translation lookaside buffer (TLB) for the given virtual page address.
    fn flush_tlb(&self, page: VirtAddr);

    /// Flush the entire translation lookaside buffer (TLB).
    fn flush_tlb_all(&self);

    /// Activate this page table by loading it into the MMU.
    ///
    /// # Safety
    /// Activating a page table switches the active address space and can cause undefined behavior
    /// if kernel mappings are not correctly set up.
    unsafe fn activate(&self);
}
