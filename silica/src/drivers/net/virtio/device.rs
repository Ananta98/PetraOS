// SPDX-License-Identifier: GPL-2.0

//! `virtio-net` PCI device glue and [`NetDevice`] implementation.
//!
//! [`VirtioNetDriver`] registers with the VirtIO bus and constructs one
//! [`VirtioNet`] instance per attached PCI transport (`eth0`, `eth1`, ...).

use alloc::format;
use alloc::string::String;
use alloc::sync::Arc;
use core::mem::size_of;
use core::sync::atomic::{AtomicUsize, Ordering};
use ostd::mm::{
    HasPaddr, PAGE_SIZE, VmIo, VmReader, VmWriter, dma::DmaCoherent, io::util::HasVmReaderWriter,
};
use spin::Mutex;

use super::protocol::{
    FEATURE_MAC, FEATURE_MTU, FEATURE_STATUS, NetConfig, PCI_DEVICE_ID_VIRTIO_NET, QUEUE_COUNT,
    RX_QUEUE, TX_QUEUE, VirtioNetHdr,
};
use crate::drivers::bus::virtio::{
    SubmitBuffer, VIRTIO_F_VERSION_1, VirtQueue, VirtioDevice, VirtioTransport,
};
use crate::drivers::net::{NetDevice, get_network_device, register_network_device};
use crate::uapi::errno::{Errno, Result};

/// Size of each pre-posted RX slot in bytes (12-byte header + Ethernet frame + headroom).
const RX_BUFFER_SIZE: usize = 2048;

/// Maximum number of RX buffers pre-populated into `receiveq1`.
const MAX_RX_BUFFERS: usize = 32;

/// Size of the TX staging buffer in bytes (1 page = 4096 bytes).
const TX_BUFFER_SIZE: usize = PAGE_SIZE;

/// Monotonic interface counter for fallback `ethN` naming.
static NEXT_IFACE_INDEX: AtomicUsize = AtomicUsize::new(0);

/// Coherent DMA pool holding pre-posted receive buffers (`rx_count * RX_BUFFER_SIZE`).
struct RxRingState {
    rx_dma: DmaCoherent,
    rx_count: usize,
}

impl RxRingState {
    fn new(rx_queue: &VirtQueue) -> Result<Self> {
        let rx_count = (rx_queue.size() as usize).min(MAX_RX_BUFFERS);
        if rx_count == 0 {
            crate::return_errno!(ENODEV, "virtio-net RX queue has zero capacity");
        }
        let total_bytes = rx_count * RX_BUFFER_SIZE;
        let frames = total_bytes.div_ceil(PAGE_SIZE);
        let rx_dma = DmaCoherent::alloc(frames, true).map_err(|_| Errno::ENOMEM)?;
        let base_paddr = rx_dma.paddr() as u64;

        for slot in 0..rx_count {
            let slot_paddr = base_paddr + (slot * RX_BUFFER_SIZE) as u64;
            rx_queue.push_avail_desc(
                slot as u16,
                SubmitBuffer {
                    addr: slot_paddr,
                    len: RX_BUFFER_SIZE as u32,
                    writable: true,
                },
            )?;
        }
        rx_queue.notify()?;

        Ok(Self { rx_dma, rx_count })
    }
}

/// Coherent DMA staging page used to serialize outgoing frames on `transmitq1`.
struct TxStaging {
    tx_dma: DmaCoherent,
}

impl TxStaging {
    fn new() -> Result<Self> {
        Ok(Self {
            tx_dma: DmaCoherent::alloc(1, true).map_err(|_| Errno::ENOMEM)?,
        })
    }
}

/// One active `virtio-net` network interface bound to a PCI transport.
pub struct VirtioNet {
    _transport: Arc<VirtioTransport>,
    rx_queue: Arc<VirtQueue>,
    tx_queue: Arc<VirtQueue>,
    rx_state: Mutex<RxRingState>,
    tx_staging: Mutex<TxStaging>,
    config: NetConfig,
    name: String,
}

impl VirtioNet {
    /// Initializes a `VirtioNet` device over an activated [`VirtioTransport`].
    pub fn new(transport: &Arc<VirtioTransport>) -> Result<Arc<Self>> {
        let Some(rx_queue) = transport.queue(RX_QUEUE) else {
            ostd::error!(
                "virtio-net: missing RX queue (activated queues = {})",
                transport.queues().len()
            );
            crate::return_errno!(ENODEV, "virtio-net does not expose a receive queue");
        };
        let Some(tx_queue) = transport.queue(TX_QUEUE) else {
            ostd::error!(
                "virtio-net: missing TX queue (activated queues = {})",
                transport.queues().len()
            );
            crate::return_errno!(ENODEV, "virtio-net does not expose a transmit queue");
        };

        let config = NetConfig::read(transport)?;
        let rx_state = RxRingState::new(&rx_queue)?;
        let tx_staging = TxStaging::new()?;
        let name = allocate_interface_name();

        ostd::info!(
            "virtio-net: initialized \"{}\" (MAC: {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}, MTU: {}, link_up: {}, rx_ring: {})",
            name,
            config.mac[0],
            config.mac[1],
            config.mac[2],
            config.mac[3],
            config.mac[4],
            config.mac[5],
            config.mtu,
            config.link_up,
            rx_state.rx_count,
        );

        Ok(Arc::new(Self {
            _transport: Arc::clone(transport),
            rx_queue,
            tx_queue,
            rx_state: Mutex::new(rx_state),
            tx_staging: Mutex::new(tx_staging),
            config,
            name,
        }))
    }
}

impl NetDevice for VirtioNet {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn mac_address(&self) -> [u8; 6] {
        self.config.mac
    }

    fn can_transmit(&self) -> bool {
        self.config.link_up
    }

    fn transmit(&self, packet: &[u8]) -> Result<()> {
        if packet.is_empty() {
            return Ok(());
        }
        let hdr_len = size_of::<VirtioNetHdr>();
        let total_len = hdr_len
            .checked_add(packet.len())
            .ok_or(Errno::EMSGSIZE)?;
        if total_len > TX_BUFFER_SIZE {
            crate::return_errno!(EMSGSIZE, "packet exceeds virtio-net TX staging size");
        }

        let staging = self.tx_staging.lock();
        let hdr = VirtioNetHdr::new_tx();
        staging
            .tx_dma
            .write_val(0, &hdr)
            .map_err(|_| Errno::EIO)?;

        let mut writer = staging.tx_dma.writer();
        writer.skip(hdr_len);
        let mut reader = VmReader::from(packet);
        writer.write(&mut reader);

        let tx_paddr = staging.tx_dma.paddr() as u64;
        self.tx_queue.submit_wait(&[SubmitBuffer {
            addr: tx_paddr,
            len: total_len as u32,
            writable: false,
        }])?;

        Ok(())
    }

    fn can_receive(&self) -> bool {
        self.rx_queue.has_used()
    }

    fn receive(&self, buf: &mut [u8]) -> Result<usize> {
        let rx_state = self.rx_state.lock();
        let Some((desc_id, written_len)) = self.rx_queue.pop_used_desc()? else {
            return Ok(0);
        };

        let slot = desc_id as usize;
        if slot >= rx_state.rx_count {
            crate::return_errno!(EIO, "virtio-net RX completion returned invalid descriptor");
        }

        let hdr_len = size_of::<VirtioNetHdr>();
        let total_len = (written_len as usize).min(RX_BUFFER_SIZE);
        let copy_len = if total_len > hdr_len {
            (total_len - hdr_len).min(buf.len())
        } else {
            0
        };

        if copy_len > 0 {
            let payload_offset = slot * RX_BUFFER_SIZE + hdr_len;
            let mut reader = rx_state.rx_dma.reader();
            reader.skip(payload_offset);
            let mut writer = VmWriter::from(&mut buf[..copy_len]);
            reader.read(&mut writer);
        }

        // Recycle the descriptor back into `receiveq1` and notify the device.
        let slot_paddr = (rx_state.rx_dma.paddr() as u64) + (slot * RX_BUFFER_SIZE) as u64;
        self.rx_queue.push_avail_desc(
            desc_id,
            SubmitBuffer {
                addr: slot_paddr,
                len: RX_BUFFER_SIZE as u32,
                writable: true,
            },
        )?;
        self.rx_queue.notify()?;

        Ok(copy_len)
    }

    fn mtu(&self) -> usize {
        self.config.mtu
    }
}

/// Picks the next unused `ethN` interface name so `virtio-net` and other NIC
/// drivers (e.g. `e1000`) never collide in `NetClass`.
fn allocate_interface_name() -> String {
    loop {
        let idx = NEXT_IFACE_INDEX.fetch_add(1, Ordering::Relaxed);
        let candidate = format!("eth{}", idx);
        if get_network_device(&candidate).is_none() {
            return candidate;
        }
    }
}

/// VirtIO bus registration adapter for `virtio-net`.
pub struct VirtioNetDriver;

impl VirtioDevice for VirtioNetDriver {
    fn device_id(&self) -> u16 {
        PCI_DEVICE_ID_VIRTIO_NET
    }

    fn name(&self) -> &'static str {
        "virtio-net"
    }

    fn queue_count(&self) -> usize {
        QUEUE_COUNT
    }

    fn accept_features(&self, device_features: u64) -> u64 {
        device_features & (VIRTIO_F_VERSION_1 | FEATURE_MAC | FEATURE_STATUS | FEATURE_MTU)
    }

    fn on_init(&self, transport: &Arc<VirtioTransport>) -> Result<()> {
        let net = VirtioNet::new(transport)?;
        register_network_device(net)?;
        Ok(())
    }
}
