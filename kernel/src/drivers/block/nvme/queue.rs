#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct NvmeCmd {
    pub opcode: u8,
    pub flags: u8,
    pub cid: u16,
    pub nsid: u32,
    pub reserved0: u64,
    pub mptr: u64,
    pub dptr: [u64; 2],
    pub cdw10: u32,
    pub cdw11: u32,
    pub cdw12: u32,
    pub cdw13: u32,
    pub cdw14: u32,
    pub cdw15: u32,
}

#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct NvmeCqe {
    pub result: u32,
    pub reserved: u32,
    pub sq_head: u16,
    pub sq_id: u16,
    pub cid: u16,
    pub status: u16,
}

pub struct NvmeQueue {
    pub qid: u32,
    pub size: u16,
    pub sq_dma: *mut NvmeCmd,
    pub cq_dma: *mut NvmeCqe,
    pub sq_tail: u16,
    pub cq_head: u16,
    pub cq_phase: bool,
    pub sq_db: *mut u32,
    pub cq_db: *mut u32,
}

impl NvmeQueue {
    pub fn new(
        qid: u32,
        size: u16,
        sq_dma: *mut NvmeCmd,
        cq_dma: *mut NvmeCqe,
        sq_db: *mut u32,
        cq_db: *mut u32,
    ) -> Self {
        Self {
            qid,
            size,
            sq_dma,
            cq_dma,
            sq_tail: 0,
            cq_head: 0,
            cq_phase: true,
            sq_db,
            cq_db,
        }
    }
}
