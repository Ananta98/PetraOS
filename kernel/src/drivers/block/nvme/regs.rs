#[repr(C)]
pub struct NvmeRegs {
    pub cap: u64,   // Controller Capabilities
    pub vs: u32,    // Version
    pub intms: u32, // Interrupt Mask Set
    pub intmc: u32, // Interrupt Mask Clear
    pub cc: u32,    // Controller Configuration
    pub rsvd0: u32,
    pub csts: u32,    // Controller Status
    pub nssr: u32,    // NVM Subsystem Reset
    pub aqa: u32,     // Admin Queue Attributes
    pub asq: u64,     // Admin Submission Queue Base Address
    pub acq: u64,     // Admin Completion Queue Base Address
    pub cmbloc: u32,  // Controller Memory Buffer Location
    pub cmbsz: u32,   // Controller Memory Buffer Size
    pub bpinfo: u32,  // Boot Partition Information
    pub bprsel: u32,  // Boot Partition Read Select
    pub bpmbl: u64,   // Boot Partition Memory Buffer Location
    pub cmbmsc: u64,  // Controller Memory Buffer Memory Space Control
    pub cmbats: u32,  // Controller Memory Buffer Attributes
    pub pmrcap: u32,  // Persistent Memory Region Capabilities
    pub pmrctl: u32,  // Persistent Memory Region Control
    pub pmrsts: u32,  // Persistent Memory Region Status
    pub pmrebs: u32,  // Persistent Memory Region Elastic Buffer Size
    pub pmrswtp: u32, // Persistent Memory Region Sustained Write Throughput
    pub pmrmsc: u64,  // Persistent Memory Region Memory Space Control
}

impl NvmeRegs {
    /// Get the doorbell offset for a specific queue ID and type (submission/completion).
    /// dstrd: Doorbell Stride (from cap register).
    pub fn doorbell_offset(&self, qid: u32, is_cq: bool, dstrd: u32) -> usize {
        let db_base = 0x1000;
        let index = (qid * 2) + if is_cq { 1 } else { 0 };
        db_base + (index as usize * (4 << dstrd))
    }
}
