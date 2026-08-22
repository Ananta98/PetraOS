//! NVMe Queue Pair (Submission & Completion Queue) Implementation

use crate::device::DriverError;
use crate::mm::dma::DmaCoherent;

#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default)]
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
#[derive(Clone, Copy, Debug, Default)]
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
    pub sq: DmaCoherent,
    pub cq: DmaCoherent,
    pub sq_tail: u16,
    pub cq_head: u16,
    pub cq_phase: bool,
    pub sq_db: *mut u32,
    pub cq_db: *mut u32,
}

unsafe impl Send for NvmeQueue {}
unsafe impl Sync for NvmeQueue {}

impl NvmeQueue {
    pub fn new(
        qid: u32,
        size: u16,
        sq: DmaCoherent,
        cq: DmaCoherent,
        sq_db: *mut u32,
        cq_db: *mut u32,
    ) -> Self {
        Self {
            qid,
            size,
            sq,
            cq,
            sq_tail: 0,
            cq_head: 0,
            cq_phase: true,
            sq_db,
            cq_db,
        }
    }

    /// Submit a command into the submission queue ring, advance tail, and write doorbell.
    pub fn submit(&mut self, cmd: NvmeCmd) {
        unsafe {
            // SAFETY: `sq` is a valid, mapped DMA buffer with capacity for `size` entries.
            let slot = self.sq.as_mut_ptr().add(self.sq_tail as usize) as *mut NvmeCmd;
            core::ptr::write_volatile(slot, cmd);
        }

        self.sq_tail = (self.sq_tail + 1) % self.size;

        unsafe {
            // SAFETY: sq_db is the MMIO doorbell pointer for this submission queue.
            core::ptr::write_volatile(self.sq_db, self.sq_tail as u32);
        }
    }

    /// Poll the completion queue for an entry matching the active phase tag.
    pub fn poll_completion(&mut self) -> Option<NvmeCqe> {
        let cqe = unsafe {
            // SAFETY: `cq` is a valid, mapped DMA buffer with capacity for `size` entries.
            let slot = self.cq.as_mut_ptr().add(self.cq_head as usize) as *mut NvmeCqe;
            core::ptr::read_volatile(slot)
        };

        let phase = (cqe.status & 1) != 0;
        if phase != self.cq_phase {
            return None; // No new completion entry yet
        }

        // Advance CQ head and update phase bit if wrapped
        self.cq_head = (self.cq_head + 1) % self.size;
        if self.cq_head == 0 {
            self.cq_phase = !self.cq_phase;
        }

        unsafe {
            // SAFETY: cq_db is the MMIO doorbell pointer for this completion queue.
            core::ptr::write_volatile(self.cq_db, self.cq_head as u32);
        }

        Some(cqe)
    }

    /// Submit command and synchronously poll until completion is received or timeout occurs.
    pub fn submit_and_wait(&mut self, cmd: NvmeCmd) -> Result<NvmeCqe, DriverError> {
        let target_cid = cmd.cid;
        self.submit(cmd);

        let mut timeout = 1_000_000usize;
        while timeout > 0 {
            if let Some(cqe) = self.poll_completion() {
                if cqe.cid == target_cid {
                    let status_code = (cqe.status >> 1) & 0x7FFF;
                    if status_code != 0 {
                        log::error!(
                            "NVMe QID {} Cmd CID {} failed with status {:#x}",
                            self.qid,
                            target_cid,
                            status_code
                        );
                        return Err(DriverError::ReadFailed);
                    }
                    return Ok(cqe);
                }
            }
            core::hint::spin_loop();
            timeout -= 1;
        }

        log::error!("NVMe QID {} Cmd CID {} timed out", self.qid, target_cid);
        Err(DriverError::ReadFailed)
    }
}
