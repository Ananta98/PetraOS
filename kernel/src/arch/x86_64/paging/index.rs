use crate::mm::VirtAddr;

/// Get PML4 (level 4 page table) index for a virtual address.
#[inline]
pub fn pml4_index(virt: VirtAddr) -> usize {
    ((virt.as_u64() >> 39) & 0x1FF) as usize
}

/// Get PDPT (level 3 page directory pointer table) index for a virtual address.
#[inline]
pub fn pdpt_index(virt: VirtAddr) -> usize {
    ((virt.as_u64() >> 30) & 0x1FF) as usize
}

/// Get PD (level 2 page directory) index for a virtual address.
#[inline]
pub fn pd_index(virt: VirtAddr) -> usize {
    ((virt.as_u64() >> 21) & 0x1FF) as usize
}

/// Get PT (level 1 page table) index for a virtual address.
#[inline]
pub fn pt_index(virt: VirtAddr) -> usize {
    ((virt.as_u64() >> 12) & 0x1FF) as usize
}
