/// Virtqueue management for VirtIO GPU driver.
///
/// Provides descriptor ring configuration, available ring updates,
/// and used ring polling over DMA-coherent memory.
use ostd::mm::dma::DmaCoherent;
use ostd::mm::{HasDaddr, VmIo};

pub struct Virtqueue {
    dma: DmaCoherent,
    queue_size: u16,
    avail_idx: u16,
    last_used_idx: u16,
}

impl Virtqueue {
    /// Create a new Virtqueue instance allocating DMA-coherent memory.
    pub fn new(queue_size: u16) -> Result<Self, ostd::Error> {
        let size = if queue_size == 0 { 16 } else { queue_size };
        // Allocate 2 contiguous pages (8KB) to fit descriptors, avail, and used rings
        let dma = DmaCoherent::alloc(2, true)?;
        Ok(Self {
            dma,
            queue_size: size,
            avail_idx: 0,
            last_used_idx: 0,
        })
    }

    /// Retrieve physical page frame number (PFN) for legacy VirtIO queue registration.
    pub fn pfn(&self) -> u32 {
        (self.dma.daddr() >> 12) as u32
    }

    /// Set a descriptor entry in the descriptor table.
    pub fn set_desc(
        &self,
        idx: u16,
        addr: u64,
        len: u32,
        flags: u16,
        next: u16,
    ) -> Result<(), ostd::Error> {
        let mut buf = [0u8; 16];
        buf[0..8].copy_from_slice(&addr.to_le_bytes());
        buf[8..12].copy_from_slice(&len.to_le_bytes());
        buf[12..14].copy_from_slice(&flags.to_le_bytes());
        buf[14..16].copy_from_slice(&next.to_le_bytes());
        self.dma.write_bytes((idx as usize) * 16, &buf)
    }

    /// Push a descriptor index onto the available ring.
    pub fn push_avail(&mut self, desc_idx: u16) -> Result<(), ostd::Error> {
        let qsize = self.queue_size as usize;
        let ring_offset = qsize * 16 + 4 + (self.avail_idx as usize % qsize) * 2;
        self.dma.write_bytes(ring_offset, &desc_idx.to_le_bytes())?;
        self.avail_idx = self.avail_idx.wrapping_add(1);
        let idx_offset = qsize * 16 + 2;
        self.dma
            .write_bytes(idx_offset, &self.avail_idx.to_le_bytes())
    }

    /// Poll used ring for completed buffer notifications.
    /// Returns `Ok(Some((descriptor_id, length_written)))` if a buffer has been processed.
    pub fn pop_used(&mut self) -> Result<Option<(u16, u32)>, ostd::Error> {
        let mut idx_buf = [0u8; 2];
        // Used ring starts at 4096 bytes (aligned offset)
        self.dma.read_bytes(4098, &mut idx_buf)?;
        let used_idx = u16::from_le_bytes(idx_buf);

        if self.last_used_idx == used_idx {
            return Ok(None);
        }

        let slot = (self.last_used_idx as usize) % (self.queue_size as usize);
        let elem_offset = 4100 + slot * 8;
        let mut elem_buf = [0u8; 8];
        self.dma.read_bytes(elem_offset, &mut elem_buf)?;

        let id = u32::from_le_bytes([elem_buf[0], elem_buf[1], elem_buf[2], elem_buf[3]]) as u16;
        let len = u32::from_le_bytes([elem_buf[4], elem_buf[5], elem_buf[6], elem_buf[7]]);

        self.last_used_idx = self.last_used_idx.wrapping_add(1);
        Ok(Some((id, len)))
    }
}
