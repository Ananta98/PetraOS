// SPDX-License-Identifier: GPL-2.0

//! Resolving page faults in user space.
//!
//! A fault arrives with an address and the access that caused it. The handler
//! finds the mapping that covers the address, checks that the access is allowed,
//! and either resolves it from the shared Zero Page, demand-pages from a backing object,
//! or handles Copy-on-Write (CoW).

use ostd::mm::{
    PAGE_SIZE, PageFlags, UFrame, Vaddr, VmSpace, io::util::HasVmReaderWriter, tlb::TlbFlushOp,
    vm_space::VmQueriedItem,
};
use ostd::arch::cpu::context::CpuException;
use ostd::task::disable_preempt;
use ostd::arch::cpu::context::PageFaultErrorCode;
use crate::{
    api::errno::Result,
    vm::{
        backing::{alloc_zeroed_frame, zero::{get_zero_frame, is_zero_frame}},
        perms::VmPerms,
    },
};

use super::{Vmar, mappings::VmMapping};

/// A page fault in user space.
#[derive(Clone, Copy, Debug)]
pub struct PageFaultInfo {
    /// The address that faulted.
    address: Vaddr,
    /// The permission the faulting access needed.
    required: VmPerms,
    /// Whether the fault was provoked rather than caused by an access.
    provoked: bool,
}

impl PageFaultInfo {
    /// Creates the information for a fault caused by an access.
    pub fn new(address: Vaddr, required: VmPerms) -> Self {
        Self {
            address,
            required,
            provoked: false,
        }
    }

    /// Marks this fault as provoked rather than caused by an access.
    pub fn provoked(mut self) -> Self {
        self.provoked = true;
        self
    }

    /// Returns the address that faulted.
    pub fn address(&self) -> Vaddr {
        self.address
    }

    /// Returns the permission the faulting access needed.
    pub fn required_perms(&self) -> VmPerms {
        self.required
    }

    /// Returns whether the fault was provoked.
    pub fn is_provoked(&self) -> bool {
        self.provoked
    }

    /// Builds the fault information for a CPU page-fault error code.
    #[cfg(target_arch = "x86_64")]
    pub fn from_fault(
        addr: Vaddr,
        error_code: PageFaultErrorCode,
    ) -> Option<Self> {
        if !error_code.contains(PageFaultErrorCode::USER) {
            return None;
        }
        let required = if error_code.contains(PageFaultErrorCode::WRITE) {
            VmPerms::WRITE
        } else if error_code.contains(PageFaultErrorCode::INSTRUCTION) {
            VmPerms::EXEC
        } else {
            VmPerms::READ
        };
        Some(Self::new(addr, required))
    }

    /// Builds the fault information for a CPU exception.
    #[cfg(target_arch = "x86_64")]
    pub fn from_cpu_exception(exception: &CpuException) -> Option<Self> {
        let CpuException::PageFault(info) = exception else {
            return None;
        };
        Self::from_fault(info.addr, info.error_code)
    }
}

impl Vmar {
    /// Resolves a page fault for this address space.
    pub fn handle_page_fault(&self, info: &PageFaultInfo) -> Result<()> {
        {
            let inner = self.inner.read();
            if let Some(mapping) = inner.mappings.get(info.address) {
                return handle_mapping_fault(mapping, &self.vm_space, info);
            }
        }

        let Some(mapping) = self.grow_for_fault(info.address) else {
            crate::return_errno!(
                EACCES,
                "no mapping contains the faulting address {:#x}",
                info.address
            );
        };
        handle_mapping_fault(&mapping, &self.vm_space, info)
    }
}

/// Resolves a page fault on a known mapping.
pub(crate) fn handle_mapping_fault(
    mapping: &VmMapping,
    vm_space: &VmSpace,
    info: &PageFaultInfo,
) -> Result<()> {
    let allowed = if info.provoked {
        mapping.perms().may()
    } else {
        mapping.perms().granted()
    };
    if !allowed.contains(info.required) {
        crate::return_errno!(
            EACCES,
            "{:?} at {:#x} is not permitted by {:?}",
            info.required,
            info.address,
            mapping.perms()
        );
    }

    let page = info.address & !(PAGE_SIZE - 1);
    let range = page..page + PAGE_SIZE;
    let is_write = info.required.contains(VmPerms::WRITE);

    let guard = disable_preempt();
    let mut cursor = vm_space
        .cursor_mut(&guard, &range)
        .expect("a mapped address always lies inside the VM space");

    let (_, item) = cursor.query().expect("the range is page-aligned");
    match item {
        Some(VmQueriedItem::MappedRam { frame, mut prop }) => {
            if VmPerms::from(prop.flags).contains(info.required) {
                TlbFlushOp::for_range(range.clone()).perform_on_current();
                return Ok(());
            }

            let is_shared_zero = is_zero_frame(&frame);
            if mapping.is_cow() && (frame.reference_count() > 1 || is_shared_zero) {
                // Break CoW / Zero-Page: allocate private frame
                let copy = if is_shared_zero {
                    alloc_zeroed_frame()?
                } else {
                    copy_frame(&frame)?
                };
                prop.flags |= PageFlags::W | PageFlags::ACCESSED | PageFlags::DIRTY;
                cursor.unmap(PAGE_SIZE);
                cursor.jump(page).expect("page-aligned");
                cursor.map(copy, prop);
            } else {
                prop.flags |= PageFlags::W | PageFlags::ACCESSED | PageFlags::DIRTY;
                cursor.protect_next(PAGE_SIZE, |flags, _| *flags = prop.flags);
            }
            cursor
                .flusher()
                .issue_tlb_flush(TlbFlushOp::for_range(range.clone()));
            cursor.flusher().dispatch_tlb_flush();
            cursor.flusher().sync_tlb_flush();
        }
        Some(VmQueriedItem::MappedIoMem { .. }) => {
            unreachable!("device mappings are not supported")
        }
        None => {
            let offset = page - mapping.start();
            let frame = obtain_frame(mapping, offset, is_write)?;
            let is_shared_zero = is_zero_frame(&frame);
            let must_copy = mapping.is_cow() && (frame.reference_count() > 1 || is_shared_zero);
            let property = mapping.fault_property(!must_copy, is_write);
            cursor.map(frame, property);
            cursor
                .flusher()
                .issue_tlb_flush(TlbFlushOp::for_range(range.clone()));
            cursor.flusher().dispatch_tlb_flush();
            cursor.flusher().sync_tlb_flush();
        }
    }
    Ok(())
}

/// Returns the page that backs `offset` bytes into `mapping`.
fn obtain_frame(mapping: &VmMapping, offset: usize, is_write: bool) -> Result<UFrame> {
    match mapping.backing() {
        Some(backing) if mapping.is_backed(offset) => {
            backing.frame(mapping.backing_offset_of(offset))
        }
        _ => {
            // Anonymous private memory: use shared Zero Page for read faults
            if !is_write && !mapping.is_shared() {
                get_zero_frame()
            } else {
                alloc_zeroed_frame()
            }
        }
    }
}

/// Returns a private copy of `frame`.
fn copy_frame(frame: &UFrame) -> Result<UFrame> {
    let copy = alloc_zeroed_frame()?;
    copy.writer().write(&mut frame.reader());
    Ok(copy)
}

#[cfg(ktest)]
mod tests {
    use ostd::mm::{PAGE_SIZE, Vaddr};
    use ostd::prelude::ktest;

    use super::*;
    use crate::vm::{
        backing::is_zero_frame_paddr,
        flags::MmapFlags,
        perms::VmPerms,
        tests::{not_resident, record_at, touch},
    };

    const STACK_TOP: Vaddr = 0x4000_0000;

    fn growdown_stack(vmar: &Vmar, flags: MmapFlags) -> Vaddr {
        vmar.mmap_anonymous(
            STACK_TOP,
            2 * PAGE_SIZE,
            VmPerms::READ | VmPerms::WRITE,
            flags,
        )
        .expect("the stack mapping is created at the requested address")
    }

    fn fault(vmar: &Vmar, addr: Vaddr) -> Result<()> {
        vmar.handle_page_fault(&PageFaultInfo::new(addr, VmPerms::WRITE))
    }

    #[ktest]
    fn a_fault_below_a_growdown_mapping_extends_it() {
        let vmar = Vmar::new();
        let start = growdown_stack(&vmar, MmapFlags::PRIVATE | MmapFlags::GROWSDOWN);
        assert_eq!(start, STACK_TOP);
        assert_eq!(vmar.total_mapped_size(), 2 * PAGE_SIZE);

        let below = start - PAGE_SIZE;
        fault(&vmar, below).expect("a stack with room grows downwards");

        let grown = record_at(&vmar, below);
        assert_eq!(grown.start(), below, "the record now reaches the faulting page");
        assert_eq!(grown.len(), 3 * PAGE_SIZE);
        assert_eq!(
            vmar.total_mapped_size(),
            3 * PAGE_SIZE,
            "the address space grew by the page that was added"
        );
    }

    #[ktest]
    fn growth_faults_the_page_in_and_it_reads_as_zero() {
        let vmar = Vmar::new();
        let start = growdown_stack(&vmar, MmapFlags::PRIVATE | MmapFlags::GROWSDOWN);

        let below = start - PAGE_SIZE;
        assert!(not_resident(&vmar, below), "the page is not in the page table yet");
        fault(&vmar, below).expect("the stack grows");
        assert!(!not_resident(&vmar, below), "the faulting page is now resident");

        touch(&vmar, below..below + PAGE_SIZE).expect("the grown page faults in");
        assert_eq!(crate::vm::tests::peek(&vmar, below), 0);
    }

    #[ktest]
    fn growth_reaches_several_pages_down_at_once() {
        let vmar = Vmar::new();
        let start = growdown_stack(&vmar, MmapFlags::PRIVATE | MmapFlags::GROWSDOWN);

        let target = start - 16 * PAGE_SIZE;
        fault(&vmar, target).expect("the stack grows to reach the page");

        let grown = record_at(&vmar, target);
        assert_eq!(grown.start(), target);
        assert_eq!(grown.len(), 18 * PAGE_SIZE);
    }

    #[ktest]
    fn zero_page_deduplication_and_cow_break() {
        let vmar = Vmar::new();
        let addr = 0x5000_0000;
        vmar.mmap_anonymous(
            addr,
            PAGE_SIZE,
            VmPerms::READ | VmPerms::WRITE,
            MmapFlags::PRIVATE | MmapFlags::FIXED | MmapFlags::ANONYMOUS,
        ).unwrap();

        // 1. Read fault should install the shared zero page
        vmar.handle_page_fault(&PageFaultInfo::new(addr, VmPerms::READ)).unwrap();
        let (paddr1, flags1) = crate::vm::tests::page_at(&vmar, addr).unwrap();
        assert!(is_zero_frame_paddr(paddr1), "read fault must install shared Zero Page");
        assert!(!flags1.contains(ostd::mm::PageFlags::W), "Zero Page must be read-only");

        // 2. Write fault should break CoW and allocate a private writable frame
        vmar.handle_page_fault(&PageFaultInfo::new(addr, VmPerms::WRITE)).unwrap();
        let (paddr2, flags2) = crate::vm::tests::page_at(&vmar, addr).unwrap();
        assert!(!is_zero_frame_paddr(paddr2), "write fault must allocate private frame");
        assert!(flags2.contains(ostd::mm::PageFlags::W), "private frame must be writable");
    }
}
