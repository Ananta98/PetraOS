use crate::mm::types::{PhysAddr, VirtAddr};

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct MapFlags: u64 {
        const READ     = 1 << 0;
        const WRITE    = 1 << 1;
        const EXECUTE  = 1 << 2;
        const USER     = 1 << 3;
        const NO_CACHE = 1 << 4;
    }
}

bitflags::bitflags! {
    /// Architecture-agnostic representation of memory access type during a page fault.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct PageFaultAccess: u32 {
        const PRESENT = 1 << 0;  // Fault caused by protection violation (page is present)
        const WRITE   = 1 << 1;  // Fault caused by a write access
        const USER    = 1 << 2;  // Fault caused by user-mode instruction/access
        const EXECUTE = 1 << 3;  // Fault caused by instruction fetch
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageFaultError {
    UnmappedAccess,        // Virtual address is not within any registered VMA
    ProtectionViolation,   // VMA flags disallow the requested access mode
    FrameAllocationFailed, // Physical memory allocator ran out of pages
    PagingError(MapError), // Failure while updating page tables
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapError {
    FrameAllocationFailed,
    AlreadyMapped,
    NotMapped,
    InvalidAddress,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnmapError {
    NotMapped,
    InvalidAddress,
}

pub trait PageTable: Send + Sync {
    /// Create a new page table by allocating a root directory and copying kernel-space mappings.
    fn new() -> Result<Self, MapError>
    where
        Self: Sized;

    /// Recreate a page table interface wrapper around an existing hardware page table root.
    ///
    /// # Safety
    /// The caller must ensure that `root` points to a valid page directory root (e.g., PML4).
    unsafe fn from_root(root: PhysAddr) -> Self
    where
        Self: Sized;

    /// Get the physical address of the page table root directory.
    fn root(&self) -> PhysAddr;

    /// Map a virtual page to a physical frame.
    fn map(&mut self, page: VirtAddr, frame: PhysAddr, flags: MapFlags) -> Result<(), MapError>;

    /// Map a contiguous range of virtual pages to physical frames.
    fn map_range(
        &mut self,
        page: VirtAddr,
        frame: PhysAddr,
        size: usize,
        flags: MapFlags,
    ) -> Result<(), MapError>;

    /// Unmap a virtual page.
    fn unmap(&mut self, page: VirtAddr) -> Result<PhysAddr, UnmapError>;

    /// Unmap a contiguous range of virtual pages.
    fn unmap_range(&mut self, page: VirtAddr, size: usize) -> Result<(), UnmapError>;

    /// Remap a virtual page with new flags.
    fn remap(&mut self, page: VirtAddr, flags: MapFlags) -> Result<(), MapError>;

    /// Remap a contiguous range of virtual pages with new flags.
    fn remap_range(&mut self, page: VirtAddr, size: usize, flags: MapFlags)
    -> Result<(), MapError>;

    /// Translate a virtual address to its corresponding physical address.
    fn translate(&self, virt: VirtAddr) -> Option<PhysAddr>;

    /// Activate this page table by loading it into the MMU.
    ///
    /// # Safety
    /// Activating a page table switches the active address space and can cause undefined behavior
    /// if kernel mappings are not correctly set up.
    unsafe fn activate(&self);
}
