//! NVMe MMIO Register Definitions and Doorbell Offset Helpers

#[repr(C)]
pub struct NvmeRegs {
    pub cap: u64,   // Controller Capabilities (0x00)
    pub vs: u32,    // Version (0x08)
    pub intms: u32, // Interrupt Mask Set (0x0C)
    pub intmc: u32, // Interrupt Mask Clear (0x10)
    pub cc: u32,    // Controller Configuration (0x14)
    pub rsvd0: u32,
    pub csts: u32,    // Controller Status (0x1C)
    pub nssr: u32,    // NVM Subsystem Reset (0x20)
    pub aqa: u32,     // Admin Queue Attributes (0x24)
    pub asq: u64,     // Admin Submission Queue Base Address (0x28)
    pub acq: u64,     // Admin Completion Queue Base Address (0x30)
    pub cmbloc: u32,  // Controller Memory Buffer Location (0x38)
    pub cmbsz: u32,   // Controller Memory Buffer Size (0x3C)
    pub bpinfo: u32,  // Boot Partition Information (0x40)
    pub bprsel: u32,  // Boot Partition Read Select (0x44)
    pub bpmbl: u64,   // Boot Partition Memory Buffer Location (0x48)
    pub cmbmsc: u64,  // Controller Memory Buffer Memory Space Control (0x50)
    pub cmbats: u32,  // Controller Memory Buffer Attributes (0x58)
    pub pmrcap: u32,  // Persistent Memory Region Capabilities (0x5C)
    pub pmrctl: u32,  // Persistent Memory Region Control (0x60)
    pub pmrsts: u32,  // Persistent Memory Region Status (0x64)
    pub pmrebs: u32,  // Persistent Memory Region Elastic Buffer Size (0x68)
    pub pmrswtp: u32, // Persistent Memory Region Sustained Write Throughput (0x6C)
    pub pmrmsc: u64,  // Persistent Memory Region Memory Space Control (0x70)
}

// Controller Configuration (CC) bit masks
pub const NVME_CC_EN: u32 = 1 << 0;
pub const NVME_CC_CSS_NVM: u32 = 0 << 4;
pub const NVME_CC_MPS_4K: u32 = 0 << 7;
pub const NVME_CC_AMS_RR: u32 = 0 << 11;
pub const NVME_CC_IOSQES_64: u32 = 6 << 16;
pub const NVME_CC_IOCQES_16: u32 = 4 << 20;

// Controller Status (CSTS) bit masks
pub const NVME_CSTS_RDY: u32 = 1 << 0;
pub const NVME_CSTS_CFS: u32 = 1 << 1;

impl NvmeRegs {
    /// Calculate pointer to the doorbell register for a given queue ID and direction.
    /// `dstrd`: Doorbell Stride index from `cap` register (bits 35:32).
    /// `qid`: Queue Identifier.
    /// `is_cq`: `false` for Submission Queue, `true` for Completion Queue.
    pub fn doorbell_ptr(regs_ptr: *mut NvmeRegs, qid: u32, is_cq: bool, dstrd: u32) -> *mut u32 {
        let db_base = 0x1000usize;
        let index = (qid * 2) + if is_cq { 1 } else { 0 };
        let stride = 4usize << dstrd;
        let offset = db_base + (index as usize * stride);

        // SAFETY: Offset calculation stays within the mapped MMIO window of the NVMe controller.
        unsafe { (regs_ptr as *mut u8).add(offset) as *mut u32 }
    }
}
