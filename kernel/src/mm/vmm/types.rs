use alloc::sync::Arc;
use crate::mm::vmm::paging::PhysAddr;

/// Backing store kind for a Virtual Memory Area (VMA).
#[derive(Clone)]
pub enum VmAreaKind {
    Anonymous,
    Device {
        phys_start: PhysAddr,
    },
    File {
        file: Arc<dyn crate::fs::FileOps>,
        offset: usize,
        file_size: usize,
    },
    Shared {
        shmid: i32,
    },
}

impl PartialEq for VmAreaKind {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (VmAreaKind::Anonymous, VmAreaKind::Anonymous) => true,
            (VmAreaKind::Device { phys_start: p1 }, VmAreaKind::Device { phys_start: p2 }) => {
                p1 == p2
            }
            (
                VmAreaKind::File {
                    file: f1,
                    offset: o1,
                    file_size: s1,
                },
                VmAreaKind::File {
                    file: f2,
                    offset: o2,
                    file_size: s2,
                },
            ) => o1 == o2 && s1 == s2 && Arc::ptr_eq(f1, f2),
            (VmAreaKind::Shared { shmid: s1 }, VmAreaKind::Shared { shmid: s2 }) => s1 == s2,
            _ => false,
        }
    }
}

impl Eq for VmAreaKind {}

impl core::fmt::Debug for VmAreaKind {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            VmAreaKind::Anonymous => write!(f, "Anonymous"),
            VmAreaKind::Device { phys_start } => write!(f, "Device({:?})", phys_start),
            VmAreaKind::File {
                offset, file_size, ..
            } => write!(f, "File(offset={}, size={})", offset, file_size),
            VmAreaKind::Shared { shmid } => write!(f, "Shared(shmid={})", shmid),
        }
    }
}
