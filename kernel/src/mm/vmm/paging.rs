use x86_64::structures::paging::mapper::{FlagUpdateError, MapToError, UnmapError};
use x86_64::structures::paging::{PageTableFlags, Size4KiB};
use x86_64::{PhysAddr, VirtAddr};

pub trait PageTable: Send + Sync {
    /// Create a new page table by allocating a root directory and copying kernel-space mappings.
    fn new() -> Result<Self, MapToError<Size4KiB>>
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
    fn map(
        &mut self,
        page: VirtAddr,
        frame: PhysAddr,
        flags: PageTableFlags,
    ) -> Result<(), MapToError<Size4KiB>>;

    /// Map a contiguous range of virtual pages to physical frames.
    fn map_range(
        &mut self,
        page: VirtAddr,
        frame: PhysAddr,
        size: usize,
        flags: PageTableFlags,
    ) -> Result<(), MapToError<Size4KiB>>;

    /// Unmap a virtual page.
    fn unmap(&mut self, page: VirtAddr) -> Result<PhysAddr, UnmapError>;

    /// Unmap a contiguous range of virtual pages.
    fn unmap_range(&mut self, page: VirtAddr, size: usize) -> Result<(), UnmapError>;

    /// Remap a virtual page with new flags.
    fn remap(&mut self, page: VirtAddr, flags: PageTableFlags) -> Result<(), FlagUpdateError>;

    /// Remap a contiguous range of virtual pages with new flags.
    fn remap_range(
        &mut self,
        page: VirtAddr,
        size: usize,
        flags: PageTableFlags,
    ) -> Result<(), FlagUpdateError>;

    /// Translate a virtual address to its corresponding physical address.
    fn translate(&self, virt: VirtAddr) -> Option<PhysAddr>;

    /// Retrieve physical frame address and raw page entry flags for a virtual address.
    fn get_entry(&self, virt: VirtAddr) -> Option<(PhysAddr, PageTableFlags)>;

    /// Activate this page table by loading it into the MMU.
    ///
    /// # Safety
    /// Activating a page table switches the active address space and can cause undefined behavior
    /// if kernel mappings are not correctly set up.
    unsafe fn activate(&self);
}
