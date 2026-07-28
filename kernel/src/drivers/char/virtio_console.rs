use super::{CharDevice, InputBuffer, register_char_device};
use crate::drivers::pci::{self, PciBar, PciDevice};
use crate::irq::IrqRegistration;
use alloc::sync::Arc;
use alloc::vec::Vec;
use ostd::arch::device::io_port::ReadWriteAccess;
use ostd::io::{IoMem, IoPort};
use ostd::mm::dma::DmaCoherent;
use ostd::mm::{HasDaddr, VmIo, VmIoOnce};
use ostd::sync::SpinLock;

const VIRTIO_VENDOR_ID: u16 = 0x1AF4;
const VIRTIO_CONSOLE_LEGACY_DEVICE_ID: u16 = 0x1003;
const VIRTIO_CONSOLE_MODERN_DEVICE_ID: u16 = 0x1043;

// VirtIO Legacy Register Offsets
const REG_HOST_FEATURES: u16 = 0x00;
const REG_GUEST_FEATURES: u16 = 0x04;
const REG_QUEUE_PFN: u16 = 0x08;
const REG_QUEUE_SIZE: u16 = 0x0C;
const REG_QUEUE_SEL: u16 = 0x0E;
const REG_QUEUE_NOTIFY: u16 = 0x10;
const REG_DEVICE_STATUS: u16 = 0x12;
const REG_ISR_STATUS: u16 = 0x13;

// VirtIO Device Status Flags
const STATUS_RESET: u8 = 0x00;
const STATUS_ACKNOWLEDGE: u8 = 0x01;
const STATUS_DRIVER: u8 = 0x02;
const STATUS_DRIVER_OK: u8 = 0x04;

// Virtqueue Descriptor Flags
const VIRTQ_DESC_F_WRITE: u16 = 2;

/// Abstract access to VirtIO BAR (Port I/O or Memory-Mapped I/O).
pub enum VirtioBar {
    Io { port_base: u16 },
    Mmio { mem: IoMem },
}

impl VirtioBar {
    pub fn read_u8(&self, offset: u16) -> u8 {
        match self {
            VirtioBar::Io { port_base } => {
                if let Ok(port) = IoPort::<u8, ReadWriteAccess>::acquire_overlapping(port_base + offset) {
                    port.read()
                } else {
                    0
                }
            }
            VirtioBar::Mmio { mem } => mem.read_once(offset as usize).unwrap_or(0),
        }
    }

    pub fn write_u8(&self, offset: u16, val: u8) {
        match self {
            VirtioBar::Io { port_base } => {
                if let Ok(port) = IoPort::<u8, ReadWriteAccess>::acquire_overlapping(port_base + offset) {
                    port.write(val);
                }
            }
            VirtioBar::Mmio { mem } => {
                let _ = mem.write_once(offset as usize, &val);
            }
        }
    }

    pub fn read_u16(&self, offset: u16) -> u16 {
        match self {
            VirtioBar::Io { port_base } => {
                if let Ok(port) = IoPort::<u16, ReadWriteAccess>::acquire_overlapping(port_base + offset) {
                    port.read()
                } else {
                    0
                }
            }
            VirtioBar::Mmio { mem } => mem.read_once(offset as usize).unwrap_or(0),
        }
    }

    pub fn write_u16(&self, offset: u16, val: u16) {
        match self {
            VirtioBar::Io { port_base } => {
                if let Ok(port) = IoPort::<u16, ReadWriteAccess>::acquire_overlapping(port_base + offset) {
                    port.write(val);
                }
            }
            VirtioBar::Mmio { mem } => {
                let _ = mem.write_once(offset as usize, &val);
            }
        }
    }

    pub fn read_u32(&self, offset: u16) -> u32 {
        match self {
            VirtioBar::Io { port_base } => {
                if let Ok(port) = IoPort::<u32, ReadWriteAccess>::acquire_overlapping(port_base + offset) {
                    port.read()
                } else {
                    0
                }
            }
            VirtioBar::Mmio { mem } => mem.read_once(offset as usize).unwrap_or(0),
        }
    }

    pub fn write_u32(&self, offset: u16, val: u32) {
        match self {
            VirtioBar::Io { port_base } => {
                if let Ok(port) = IoPort::<u32, ReadWriteAccess>::acquire_overlapping(port_base + offset) {
                    port.write(val);
                }
            }
            VirtioBar::Mmio { mem } => {
                let _ = mem.write_once(offset as usize, &val);
            }
        }
    }
}

/// Managed Virtqueue for VirtIO operations using DMA coherent memory.
struct Virtqueue {
    dma: DmaCoherent,
    queue_size: u16,
    avail_idx: u16,
    last_used_idx: u16,
}

impl Virtqueue {
    pub fn new(queue_size: u16) -> Result<Self, ostd::Error> {
        let size = if queue_size == 0 { 16 } else { queue_size };
        let dma = DmaCoherent::alloc(2, true)?;
        Ok(Self {
            dma,
            queue_size: size,
            avail_idx: 0,
            last_used_idx: 0,
        })
    }

    pub fn pfn(&self) -> u32 {
        (self.dma.daddr() >> 12) as u32
    }

    pub fn set_desc(&self, idx: u16, addr: u64, len: u32, flags: u16, next: u16) -> Result<(), ostd::Error> {
        let mut buf = [0u8; 16];
        buf[0..8].copy_from_slice(&addr.to_le_bytes());
        buf[8..12].copy_from_slice(&len.to_le_bytes());
        buf[12..14].copy_from_slice(&flags.to_le_bytes());
        buf[14..16].copy_from_slice(&next.to_le_bytes());
        self.dma.write_bytes((idx as usize) * 16, &buf)
    }

    pub fn push_avail(&mut self, desc_idx: u16) -> Result<(), ostd::Error> {
        let ring_offset = (self.queue_size as usize) * 16 + 4 + (self.avail_idx as usize % self.queue_size as usize) * 2;
        self.dma.write_bytes(ring_offset, &desc_idx.to_le_bytes())?;
        self.avail_idx = self.avail_idx.wrapping_add(1);
        let idx_offset = (self.queue_size as usize) * 16 + 2;
        self.dma.write_bytes(idx_offset, &self.avail_idx.to_le_bytes())
    }

    pub fn pop_used(&mut self) -> Result<Option<(u16, u32)>, ostd::Error> {
        let mut idx_buf = [0u8; 2];
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

/// VirtIO Console Device Implementation.
pub struct VirtioConsole {
    bar: VirtioBar,
    rx_vq: SpinLock<Virtqueue>,
    tx_vq: SpinLock<Virtqueue>,
    rx_buf_dma: DmaCoherent,
    tx_buf_dma: DmaCoherent,
    input_buffer: InputBuffer,
    _irq: Option<IrqRegistration>,
}

impl VirtioConsole {
    pub fn new(pci_dev: &PciDevice) -> Result<Self, ostd::Error> {
        pci_dev.enable_bus_mastering();

        let bar = match pci_dev.bars[0] {
            PciBar::IoSpace { port, .. } => {
                pci_dev.enable_io_space();
                VirtioBar::Io { port_base: port as u16 }
            }
            PciBar::MemoryMapped { base_addr, size, .. } if base_addr != 0 => {
                pci_dev.enable_memory_space();
                let mem_size = if size > 0 { size as usize } else { 0x1000 };
                let mem = IoMem::acquire(base_addr as usize..base_addr as usize + mem_size)?;
                VirtioBar::Mmio { mem }
            }
            _ => return Err(ostd::Error::InvalidArgs),
        };

        // Reset device and negotiate status
        bar.write_u8(REG_DEVICE_STATUS, STATUS_RESET);
        bar.write_u8(REG_DEVICE_STATUS, STATUS_ACKNOWLEDGE);
        bar.write_u8(REG_DEVICE_STATUS, STATUS_ACKNOWLEDGE | STATUS_DRIVER);

        let _host_features = bar.read_u32(REG_HOST_FEATURES);
        bar.write_u32(REG_GUEST_FEATURES, 0);

        // Setup Queue 0 (RX)
        bar.write_u16(REG_QUEUE_SEL, 0);
        let rx_qsize = bar.read_u16(REG_QUEUE_SIZE);
        let mut rx_vq = Virtqueue::new(rx_qsize)?;
        bar.write_u32(REG_QUEUE_PFN, rx_vq.pfn());

        let rx_buf_dma = DmaCoherent::alloc(2, true)?;
        const SLOT_SIZE: usize = 512;
        const NUM_SLOTS: usize = 16;
        for i in 0..NUM_SLOTS {
            let slot_paddr = rx_buf_dma.daddr() as u64 + (i * SLOT_SIZE) as u64;
            rx_vq.set_desc(i as u16, slot_paddr, SLOT_SIZE as u32, VIRTQ_DESC_F_WRITE, 0)?;
            rx_vq.push_avail(i as u16)?;
        }
        bar.write_u16(REG_QUEUE_NOTIFY, 0);

        // Setup Queue 1 (TX)
        bar.write_u16(REG_QUEUE_SEL, 1);
        let tx_qsize = bar.read_u16(REG_QUEUE_SIZE);
        let tx_vq = Virtqueue::new(tx_qsize)?;
        bar.write_u32(REG_QUEUE_PFN, tx_vq.pfn());

        let tx_buf_dma = DmaCoherent::alloc(2, true)?;

        // Driver initialization complete
        bar.write_u8(
            REG_DEVICE_STATUS,
            STATUS_ACKNOWLEDGE | STATUS_DRIVER | STATUS_DRIVER_OK,
        );

        let dev = Self {
            bar,
            rx_vq: SpinLock::new(rx_vq),
            tx_vq: SpinLock::new(tx_vq),
            rx_buf_dma,
            tx_buf_dma,
            input_buffer: InputBuffer::new(4096),
            _irq: None,
        };

        Ok(dev)
    }

    /// Poll RX virtqueue for incoming console bytes.
    pub fn poll_rx(&self) {
        let mut rx_vq = self.rx_vq.lock();
        let mut received_any = false;
        let mut read_buf = [0u8; 512];

        while let Ok(Some((desc_id, len))) = rx_vq.pop_used() {
            let desc_idx = desc_id as usize;
            let copy_len = core::cmp::min(len as usize, 512);
            if copy_len > 0 && desc_idx < 16 {
                if self.rx_buf_dma.read_bytes(desc_idx * 512, &mut read_buf[..copy_len]).is_ok() {
                    self.input_buffer.push(&read_buf[..copy_len]);
                    received_any = true;
                }
            }
            let _ = rx_vq.push_avail(desc_id);
        }

        if received_any {
            self.bar.write_u16(REG_QUEUE_NOTIFY, 0);
        }
    }

    /// Transmit bytes over VirtIO Console TX virtqueue.
    pub fn send_bytes(&self, bytes: &[u8]) -> Result<usize, ostd::Error> {
        if bytes.is_empty() {
            return Ok(0);
        }

        let send_len = core::cmp::min(bytes.len(), 1024);
        self.tx_buf_dma.write_bytes(0, &bytes[..send_len])?;

        let mut tx_vq = self.tx_vq.lock();
        let tx_paddr = self.tx_buf_dma.daddr() as u64;
        tx_vq.set_desc(0, tx_paddr, send_len as u32, 0, 0)?;
        tx_vq.push_avail(0)?;

        self.bar.write_u16(REG_QUEUE_NOTIFY, 1);

        Ok(send_len)
    }
}

impl CharDevice for VirtioConsole {
    fn read(&self, buf: &mut [u8]) -> Result<usize, ostd::Error> {
        self.poll_rx();
        self.input_buffer.read_into(buf)
    }

    fn write(&self, buf: &[u8]) -> Result<usize, ostd::Error> {
        self.send_bytes(buf)
    }
}

/// VirtIO Console Driver Registration.
pub struct VirtioConsoleDriver;

impl crate::device::Driver for VirtioConsoleDriver {
    fn name(&self) -> &str {
        "virtio_console"
    }

    fn bus_name(&self) -> &str {
        "pci"
    }

    fn description(&self) -> &str {
        "VirtIO Console Character Device Driver"
    }

    fn probe(&self) -> Result<(), ostd::Error> {
        let pci_devices = pci::enumerate();
        for pci_dev in pci_devices {
            if pci_dev.vendor_id == VIRTIO_VENDOR_ID
                && (pci_dev.device_id == VIRTIO_CONSOLE_LEGACY_DEVICE_ID
                    || pci_dev.device_id == VIRTIO_CONSOLE_MODERN_DEVICE_ID)
            {
                if let Ok(console_dev) = VirtioConsole::new(&pci_dev) {
                    let dev = Arc::new(console_dev);
                    let _ = register_char_device("virtio_console", dev.clone());
                    let _ = register_char_device("hvc0", dev);
                    return Ok(());
                }
            }
        }
        Ok(())
    }
}

crate::module_driver!(
    VIRTIO_CONSOLE_INITCALL,
    virtio_console_driver_init,
    "virtio_console",
    VirtioConsoleDriver
);
