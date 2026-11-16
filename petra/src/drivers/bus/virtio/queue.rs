// SPDX-License-Identifier: GPL-2.0

//! A single split-ring virtqueue (virtio 1.0 layout).
//!
//! Commands of this kernel run one at a time, so the completion story is
//! synchronous: submit a descriptor chain, then poll the used ring until the
//! device returns. Drivers that later need real throughput should grow IRQ
//! handling here.

use alloc::sync::{Arc, Weak};
use core::fmt::{self, Debug, Formatter};
use core::sync::atomic::{AtomicBool, Ordering};
use ostd::irq::InterruptLevel;
use ostd::mm::{HasDaddr, VmIo, dma::DmaCoherent};
use ostd::sync::WaitQueue;
use ostd::task::Task;
use spin::Mutex;

use crate::drivers::bus::virtio::transport::VirtioTransport;
use crate::uapi::errno::{Errno, Result};

/// `virtq_desc.flags`: chain-on-to-next bit.
pub const DESC_FLAG_NEXT: u16 = 1 << 0;
/// `virtq_desc.flags`: the device writes into the buffer (in-side).
pub const DESC_FLAG_WRITE: u16 = 1 << 1;

/// One descriptor handed to the device as part of a submission.
#[derive(Debug, Clone, Copy)]
pub struct SubmitBuffer {
    /// Physical (DMA) address of the buffer.
    pub addr: u64,
    pub len: u32,
    /// `true` for in-side descriptors the device fills with a response.
    pub writable: bool,
}

/// `struct virtq_desc` — descriptor table entry layout.
#[repr(C)]
#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    zerocopy::FromBytes,
    zerocopy::IntoBytes,
    zerocopy::KnownLayout,
    zerocopy::Immutable,
)]
struct Desc {
    addr: u64,
    len: u32,
    flags: u16,
    next: u16,
}

/// Round-robin descriptor allocation and ring counters.
struct QueueInner {
    /// Next free descriptor index.
    free_head: u16,
    /// Descriptors not currently committed to the device.
    free_count: u16,
    /// Local copy of the driver-side `avail.idx`.
    last_avail: u16,
    /// Number of used elements already reaped.
    last_used: u16,
}

/// A split-ring virtqueue with its notification window.
pub struct VirtQueue {
    index: u16,
    size: u16,
    /// Descriptor table in page-multiple DMA-coherent memory.
    desc_dma: DmaCoherent,
    /// Avail ring: `flags`, `idx`, `ring[]`, `avail_event`.
    avail_dma: DmaCoherent,
    /// Used ring: `flags`, `idx`, `ring[]`, `used_event`.
    used_dma: DmaCoherent,
    /// Queue notification offset within the transport's notify window.
    notify_off: usize,
    /// The transport owning this queue (weak: it owns this queue).
    transport: Weak<VirtioTransport>,
    inner: Mutex<QueueInner>,
    wait_queue: WaitQueue,
    irq_enabled: AtomicBool,
}

impl VirtQueue {
    /// `struct virtq_desc` occupies 16 bytes.
    const DESC_SIZE: usize = core::mem::size_of::<Desc>();

    const fn avail_ring_bytes(size: u16) -> usize {
        // 2 flags + 2 idx + 2*size ring entries + 2 avail_event.
        4 + (size as usize) * 2 + 2
    }

    const fn used_ring_bytes(size: u16) -> usize {
        // 2 flags + 2 idx + size*8 ring entries + 2 used_event.
        4 + (size as usize) * 8 + 2
    }

    /// Creates an unlinked queue with DMA rings of `size` entries.
    pub(crate) fn new(
        index: u16,
        size: u16,
        notify_off: usize,
        transport: &Weak<VirtioTransport>,
    ) -> Result<Arc<Self>> {
        Ok(Arc::new(Self {
            index,
            size,
            desc_dma: DmaCoherent::alloc(page_frames(size as usize * Self::DESC_SIZE), true)
                .map_err(|_| Errno::ENOMEM)?,
            avail_dma: DmaCoherent::alloc(page_frames(Self::avail_ring_bytes(size)), true)
                .map_err(|_| Errno::ENOMEM)?,
            used_dma: DmaCoherent::alloc(page_frames(Self::used_ring_bytes(size)), true)
                .map_err(|_| Errno::ENOMEM)?,
            notify_off,
            transport: transport.clone(),
            inner: Mutex::new(QueueInner {
                free_head: 0,
                free_count: size,
                last_avail: 0,
                last_used: 0,
            }),
            wait_queue: WaitQueue::new(),
            irq_enabled: AtomicBool::new(false),
        }))
    }

    /// The ring physical addresses, in the order the transport programs
    /// them (desc, avail, used).
    pub(crate) fn ring_addresses(&self) -> (u64, u64, u64) {
        (
            self.desc_dma.daddr() as u64,
            self.avail_dma.daddr() as u64,
            self.used_dma.daddr() as u64,
        )
    }

    /// Returns the number of descriptor entries in this virtqueue.
    pub fn size(&self) -> u16 {
        self.size
    }

    /// Returns `true` if the device has placed at least one unconsumed entry in the used ring.
    pub fn has_used(&self) -> bool {
        let Ok(used_idx) = self.used_dma.read_val::<u16>(2) else {
            return false;
        };
        let inner = self.inner.lock();
        used_idx != inner.last_used
    }

    /// Programs single descriptor `desc_id` with `buffer` and publishes it to the avail ring
    /// without blocking. Call [`Self::notify`] afterwards to ring the queue doorbell.
    pub fn push_avail_desc(&self, desc_id: u16, buffer: SubmitBuffer) -> Result<()> {
        if desc_id >= self.size {
            crate::return_errno!(EINVAL, "descriptor index out of range");
        }
        let mut inner = self.inner.lock();
        if inner.free_count == 0 {
            crate::return_errno!(ENOMEM, "virtqueue descriptors exhausted");
        }
        inner.free_count -= 1;

        let desc = Desc {
            addr: buffer.addr,
            len: buffer.len,
            flags: if buffer.writable { DESC_FLAG_WRITE } else { 0 },
            next: 0,
        };
        self.desc_dma
            .write_val(desc_id as usize * Self::DESC_SIZE, &desc)
            .map_err(|_| Errno::EIO)?;

        let avail_slot = (inner.last_avail % self.size) as usize;
        self.avail_dma
            .write_val(4 + avail_slot * 2, &desc_id)
            .map_err(|_| Errno::EIO)?;
        inner.last_avail = inner.last_avail.wrapping_add(1);
        self.avail_dma
            .write_val(2, &inner.last_avail)
            .map_err(|_| Errno::EIO)?;
        Ok(())
    }

    /// Reaps one completed single-descriptor entry `(desc_id, len)` from the used ring,
    /// or returns `Ok(None)` if no completed entry is available.
    pub fn pop_used_desc(&self) -> Result<Option<(u16, u32)>> {
        let used_idx: u16 = self.used_dma.read_val(2).map_err(|_| Errno::EIO)?;
        let mut inner = self.inner.lock();
        if used_idx == inner.last_used {
            return Ok(None);
        }
        let slot = (inner.last_used % self.size) as usize;
        let (id, len) = self.read_used_elem(slot)?;
        inner.last_used = inner.last_used.wrapping_add(1);
        inner.free_count = (inner.free_count + 1).min(self.size);
        Ok(Some((id, len)))
    }

    /// Rings this queue's doorbell via the transport's notification window.
    pub fn notify(&self) -> Result<()> {
        self.ring_bell()
    }

    /// Submits `buffers` to the device and waits for it to consume them.
    ///
    /// Returns the length the device reports for the writable buffers.
    pub fn submit_wait(&self, buffers: &[SubmitBuffer]) -> Result<u32> {
        if buffers.is_empty() || buffers.len() > self.size as usize {
            crate::return_errno!(EINVAL, "submission does not fit the queue");
        }
        let chain_len = buffers.len() as u16;
        let (head, local_used) = {
            let mut inner = self.inner.lock();
            if inner.free_count < chain_len {
                crate::return_errno!(ENOMEM, "virtqueue descriptors exhausted");
            }
            let head = inner.free_head;
            inner.free_head = (head + chain_len) % self.size;
            inner.free_count -= chain_len;

            // Patch the descriptor table into one linked chain.
            for (i, buffer) in buffers.iter().enumerate() {
                let slot = (head + i as u16) % self.size;
                let next = if i + 1 < buffers.len() {
                    (head + i as u16 + 1) % self.size
                } else {
                    0
                };
                let flags = if i + 1 < buffers.len() {
                    DESC_FLAG_NEXT
                } else {
                    0
                } | if buffer.writable { DESC_FLAG_WRITE } else { 0 };
                let desc = Desc {
                    addr: buffer.addr,
                    len: buffer.len,
                    flags,
                    next,
                };
                self.desc_dma
                    .write_val(slot as usize * Self::DESC_SIZE, &desc)
                    .map_err(|_| Errno::EIO)?;
            }

            // Publish the chain head to the avail ring.
            let avail_slot = (inner.last_avail % self.size) as usize;
            self.avail_dma
                .write_val(4 + avail_slot * 2, &head)
                .map_err(|_| Errno::EIO)?;
            inner.last_avail = inner.last_avail.wrapping_add(1);
            self.avail_dma
                .write_val(2, &inner.last_avail)
                .map_err(|_| Errno::EIO)?;
            (head, inner.last_used)
        };

        // Ring the doorbell with the lock released.
        self.ring_bell()?;

        let len = self.wait_for_consumed(head, local_used, chain_len)?;
        Ok(len)
    }

    /// Submits multiple descriptor chains in a single doorbell kick and waits
    /// for all of them to be consumed by the device.
    pub fn submit_batch_wait(&self, chains: &[&[SubmitBuffer]]) -> Result<()> {
        if chains.is_empty() {
            return Ok(());
        }
        let total_descs: usize = chains.iter().map(|c| c.len()).sum();
        if total_descs == 0 || total_descs > self.size as usize {
            crate::return_errno!(EINVAL, "batched submission does not fit the queue");
        }
        for chain in chains {
            if chain.is_empty() {
                crate::return_errno!(EINVAL, "empty chain in batched submission");
            }
        }

        let chain_count = chains.len() as u16;
        let total_u16 = total_descs as u16;
        let local_used = {
            let mut inner = self.inner.lock();
            if inner.free_count < total_u16 {
                crate::return_errno!(ENOMEM, "virtqueue descriptors exhausted");
            }
            let mut cursor = inner.free_head;
            inner.free_head = (cursor + total_u16) % self.size;
            inner.free_count -= total_u16;

            for chain in chains {
                let chain_head = cursor;
                for (i, buffer) in chain.iter().enumerate() {
                    let slot = (cursor + i as u16) % self.size;
                    let next = if i + 1 < chain.len() {
                        (cursor + i as u16 + 1) % self.size
                    } else {
                        0
                    };
                    let flags = if i + 1 < chain.len() {
                        DESC_FLAG_NEXT
                    } else {
                        0
                    } | if buffer.writable { DESC_FLAG_WRITE } else { 0 };
                    let desc = Desc {
                        addr: buffer.addr,
                        len: buffer.len,
                        flags,
                        next,
                    };
                    self.desc_dma
                        .write_val(slot as usize * Self::DESC_SIZE, &desc)
                        .map_err(|_| Errno::EIO)?;
                }
                cursor = (cursor + chain.len() as u16) % self.size;

                let avail_slot = (inner.last_avail % self.size) as usize;
                self.avail_dma
                    .write_val(4 + avail_slot * 2, &chain_head)
                    .map_err(|_| Errno::EIO)?;
                inner.last_avail = inner.last_avail.wrapping_add(1);
            }

            self.avail_dma
                .write_val(2, &inner.last_avail)
                .map_err(|_| Errno::EIO)?;
            inner.last_used
        };

        // Single doorbell kick for the entire batch.
        self.ring_bell()?;

        let target_used = local_used.wrapping_add(chain_count);
        if self.wait_until_used(|used_idx| used_idx == target_used)? {
            let mut inner = self.inner.lock();
            inner.last_used = target_used;
            inner.free_count += total_u16;
            return Ok(());
        }
        ostd::error!(
            "virtio queue {}: submit_batch_wait timeout: target_used={}, local_used={}",
            self.index,
            target_used,
            local_used
        );
        crate::return_errno!(ETIMEDOUT, "virtqueue batch never completed")
    }

    /// Marks this queue as IRQ-driven via MSI-X.
    pub fn enable_irq(&self) {
        self.irq_enabled.store(true, Ordering::Release);
    }

    /// Top-half IRQ handler for this virtqueue: wakes any waiting submitters.
    pub fn handle_irq(&self) {
        self.wait_queue.wake_all();
    }

    /// Waits until `predicate(used_idx)` returns `true`, sleeping on `WaitQueue` when
    /// MSI-X IRQs are enabled in task context and falling back to bounded polling otherwise.
    fn wait_until_used<F>(&self, predicate: F) -> Result<bool>
    where
        F: Fn(u16) -> bool,
    {
        if self.irq_enabled.load(Ordering::Acquire)
            && InterruptLevel::current().is_task_context()
            && Task::current().is_some()
        {
            self.wait_queue.wait_until(|| {
                let used_idx: u16 = self.used_dma.read_val(2).ok()?;
                if predicate(used_idx) {
                    Some(true)
                } else {
                    None
                }
            });
            return Ok(true);
        }

        for _ in 0..SUBMIT_SPINS {
            core::hint::spin_loop();
            let used_idx: u16 = self.used_dma.read_val(2).map_err(|_| Errno::EIO)?;
            if predicate(used_idx) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Waits for the chain started at `head` to be reaped.
    fn wait_for_consumed(&self, head: u16, local_used: u16, chain_len: u16) -> Result<u32> {
        if self.wait_until_used(|used_idx| used_idx != local_used)? {
            // A synchronous submit means only this chain was outstanding,
            // so the first used element must be the awaited one.
            let slot = (local_used % self.size) as usize;
            let (id, len) = self.read_used_elem(slot)?;
            if id != head {
                crate::return_errno!(EIO, "virtqueue completed out of order");
            }
            {
                let mut inner = self.inner.lock();
                inner.last_used = local_used.wrapping_add(1);
                inner.free_count += chain_len;
            }
            return Ok(len);
        }
        let used_idx: u16 = self.used_dma.read_val(2).unwrap_or(9999);
        ostd::error!(
            "virtio queue {}: wait_for_consumed timeout: used_idx={}, local_used={}",
            self.index, used_idx, local_used
        );
        crate::return_errno!(ETIMEDOUT, "virtqueue command never completed");
    }

    /// Reads one `struct virtq_used_elem` (`(id, len)` pair).
    fn read_used_elem(&self, slot: usize) -> Result<(u16, u32)> {
        let offset = 4 + slot * 8;
        let id: u32 = self.used_dma.read_val(offset).map_err(|_| Errno::EIO)?;
        let len: u32 = self.used_dma.read_val(offset + 4).map_err(|_| Errno::EIO)?;
        Ok((id as u16, len))
    }

    /// Rings this queue's doorbell via the transport's notify window.
    fn ring_bell(&self) -> Result<()> {
        let transport = self.transport.upgrade().ok_or(Errno::ENODEV)?;
        transport.ring_bell(self.notify_off, self.index)
    }
}

/// Completion spin budget: bounded, never forever.
const SUBMIT_SPINS: usize = 10_000_000;

/// Rounds `bytes` up to whole 4 KiB frames.
const fn page_frames(bytes: usize) -> usize {
    (bytes + ostd::mm::PAGE_SIZE - 1) / ostd::mm::PAGE_SIZE
}

impl Debug for VirtQueue {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("VirtQueue")
            .field("index", &self.index)
            .field("size", &self.size)
            .finish()
    }
}
