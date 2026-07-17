use crate::device::device::{Device, DeviceType};
use crate::drivers::net::NetDevice;
use crate::drivers::pci::{PciBar, PciDevice};
use alloc::sync::Arc;
use alloc::vec::Vec;
use ostd::io::IoMem;
use ostd::mm::dma::DmaCoherent;
use ostd::mm::{HasDaddr, VmIo, VmIoOnce};

// E1000 Register Offsets
const REG_CTRL: u16 = 0x0000;
const REG_STATUS: u16 = 0x0008;
const REG_ICR: u16 = 0x00C0;
const REG_IMS: u16 = 0x00D0;
const REG_IMC: u16 = 0x00D8;
const REG_RCTL: u16 = 0x0100;
const REG_TCTL: u16 = 0x0400;
const REG_TIPG: u16 = 0x0410;
const REG_RDBAL: u16 = 0x2800;
const REG_RDBAH: u16 = 0x2804;
const REG_RDLEN: u16 = 0x2808;
const REG_RDH: u16 = 0x2810;
const REG_RDT: u16 = 0x2818;
const REG_TDBAL: u16 = 0x3800;
const REG_TDBAH: u16 = 0x3804;
const REG_TDLEN: u16 = 0x3808;
const REG_TDH: u16 = 0x3810;
const REG_TDT: u16 = 0x3818;
const REG_RAL0: u16 = 0x5400;
const REG_RAH0: u16 = 0x5404;

const NUM_RX_DESC: usize = 32;
const NUM_TX_DESC: usize = 32;

struct RxDesc {
    addr: u64,
    length: u16,
    checksum: u16,
    status: u8,
    errors: u8,
    special: u16,
}

struct TxDesc {
    addr: u64,
    length: u16,
    cso: u8,
    cmd: u8,
    status: u8,
    css: u8,
    special: u16,
}

pub struct E1000 {
    mem: IoMem,
    rx_ring: DmaCoherent,
    tx_ring: DmaCoherent,
    rx_buffers: DmaCoherent,
    tx_buffers: DmaCoherent,
    rx_index: core::sync::atomic::AtomicUsize,
    tx_index: core::sync::atomic::AtomicUsize,
    mac: [u8; 6],
}

impl E1000 {
    pub fn new(pci_dev: &PciDevice) -> Result<Self, ostd::Error> {
        let base_addr = match pci_dev.bars[0] {
            PciBar::MemoryMapped { base_addr, .. } if base_addr != 0 => base_addr,
            _ => return Err(ostd::Error::InvalidArgs),
        };

        pci_dev.enable_memory_space();
        pci_dev.enable_bus_mastering();

        // Map E1000 register space (128KB)
        let mem = IoMem::acquire(base_addr as usize..base_addr as usize + 0x20000)?;

        // Allocate descriptor rings (1 page each)
        let rx_ring = DmaCoherent::alloc(1, true)?;
        let tx_ring = DmaCoherent::alloc(1, true)?;

        // Allocate packet buffers: 32 buffers * 2048 bytes = 64KB = 16 pages
        let rx_buffers = DmaCoherent::alloc(16, true)?;
        let tx_buffers = DmaCoherent::alloc(16, true)?;

        let mut dev = Self {
            mem,
            rx_ring,
            tx_ring,
            rx_buffers,
            tx_buffers,
            rx_index: core::sync::atomic::AtomicUsize::new(0),
            tx_index: core::sync::atomic::AtomicUsize::new(0),
            mac: [0; 6],
        };

        dev.init_hardware()?;
        Ok(dev)
    }

    fn write_reg(&self, reg: u16, val: u32) {
        let _ = self.mem.write_once(reg as usize, &val);
    }

    fn read_reg(&self, reg: u16) -> u32 {
        self.mem.read_once(reg as usize).unwrap_or(0)
    }

    fn set_rx_desc(&self, idx: usize, desc: &RxDesc) -> Result<(), ostd::Error> {
        let mut bytes = [0u8; 16];
        bytes[0..8].copy_from_slice(&desc.addr.to_le_bytes());
        bytes[8..10].copy_from_slice(&desc.length.to_le_bytes());
        bytes[10..12].copy_from_slice(&desc.checksum.to_le_bytes());
        bytes[12] = desc.status;
        bytes[13] = desc.errors;
        bytes[14..16].copy_from_slice(&desc.special.to_le_bytes());
        self.rx_ring.write_bytes(idx * 16, &bytes)
    }

    fn get_rx_desc(&self, idx: usize) -> Result<RxDesc, ostd::Error> {
        let mut bytes = [0u8; 16];
        self.rx_ring.read_bytes(idx * 16, &mut bytes)?;
        Ok(RxDesc {
            addr: u64::from_le_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            ]),
            length: u16::from_le_bytes([bytes[8], bytes[9]]),
            checksum: u16::from_le_bytes([bytes[10], bytes[11]]),
            status: bytes[12],
            errors: bytes[13],
            special: u16::from_le_bytes([bytes[14], bytes[15]]),
        })
    }

    fn set_tx_desc(&self, idx: usize, desc: &TxDesc) -> Result<(), ostd::Error> {
        let mut bytes = [0u8; 16];
        bytes[0..8].copy_from_slice(&desc.addr.to_le_bytes());
        bytes[8..10].copy_from_slice(&desc.length.to_le_bytes());
        bytes[10] = desc.cso;
        bytes[11] = desc.cmd;
        bytes[12] = desc.status;
        bytes[13] = desc.css;
        bytes[14..16].copy_from_slice(&desc.special.to_le_bytes());
        self.tx_ring.write_bytes(idx * 16, &bytes)
    }

    fn get_tx_desc(&self, idx: usize) -> Result<TxDesc, ostd::Error> {
        let mut bytes = [0u8; 16];
        self.tx_ring.read_bytes(idx * 16, &mut bytes)?;
        Ok(TxDesc {
            addr: u64::from_le_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            ]),
            length: u16::from_le_bytes([bytes[8], bytes[9]]),
            cso: bytes[10],
            cmd: bytes[11],
            status: bytes[12],
            css: bytes[13],
            special: u16::from_le_bytes([bytes[14], bytes[15]]),
        })
    }

    fn init_hardware(&mut self) -> Result<(), ostd::Error> {
        // Reset device
        self.write_reg(REG_CTRL, self.read_reg(REG_CTRL) | 0x0400_0000);
        // Spin a bit for reset
        for _ in 0..1000 {
            core::hint::spin_loop();
        }

        // Disable interrupts
        self.write_reg(REG_IMC, 0xFFFF_FFFF);
        let _ = self.read_reg(REG_ICR); // Clear pending

        // Read MAC Address from Receive Address Registers
        let ral = self.read_reg(REG_RAL0);
        let rah = self.read_reg(REG_RAH0);
        self.mac[0] = (ral & 0xFF) as u8;
        self.mac[1] = ((ral >> 8) & 0xFF) as u8;
        self.mac[2] = ((ral >> 16) & 0xFF) as u8;
        self.mac[3] = ((ral >> 24) & 0xFF) as u8;
        self.mac[4] = (rah & 0xFF) as u8;
        self.mac[5] = ((rah >> 8) & 0xFF) as u8;

        // Initialize RX Ring
        let rx_phys = self.rx_ring.daddr() as u64;
        self.write_reg(REG_RDBAL, (rx_phys & 0xFFFF_FFFF) as u32);
        self.write_reg(REG_RDBAH, (rx_phys >> 32) as u32);
        self.write_reg(REG_RDLEN, (NUM_RX_DESC * 16) as u32);
        self.write_reg(REG_RDH, 0);
        self.write_reg(REG_RDT, (NUM_RX_DESC - 1) as u32);

        // Fill RX descriptors
        let buf_base_phys = self.rx_buffers.daddr() as u64;
        for i in 0..NUM_RX_DESC {
            let desc = RxDesc {
                addr: buf_base_phys + (i * 2048) as u64,
                length: 0,
                checksum: 0,
                status: 0,
                errors: 0,
                special: 0,
            };
            self.set_rx_desc(i, &desc)?;
        }

        // Initialize TX Ring
        let tx_phys = self.tx_ring.daddr() as u64;
        self.write_reg(REG_TDBAL, (tx_phys & 0xFFFF_FFFF) as u32);
        self.write_reg(REG_TDBAH, (tx_phys >> 32) as u32);
        self.write_reg(REG_TDLEN, (NUM_TX_DESC * 16) as u32);
        self.write_reg(REG_TDH, 0);
        self.write_reg(REG_TDT, 0);

        // Clear TX descriptors (set status to DD/done)
        for i in 0..NUM_TX_DESC {
            let desc = TxDesc {
                addr: 0,
                length: 0,
                cso: 0,
                cmd: 0,
                status: 0x01, // DD flag set
                css: 0,
                special: 0,
            };
            self.set_tx_desc(i, &desc)?;
        }

        // Link Up
        self.write_reg(REG_CTRL, self.read_reg(REG_CTRL) | 0x40);

        // Transmit options (IPG)
        self.write_reg(REG_TIPG, 0x0060_200A);

        // Enable TX
        self.write_reg(
            REG_TCTL,
            0x0000_0002 | 0x0000_0008 | (0x10 << 4) | (0x40 << 12),
        );

        // Enable RX: EN (bit 1) | UPE (bit 3) | MPE (bit 4) | BAM (bit 15) | SECRC (bit 26)
        self.write_reg(
            REG_RCTL,
            0x0000_0002 | 0x0000_0008 | 0x0000_0010 | 0x0000_8000 | 0x0400_0000,
        );

        Ok(())
    }
}

impl NetDevice for E1000 {
    fn mac_address(&self) -> [u8; 6] {
        self.mac
    }

    fn send(&self, packet: &[u8]) -> Result<(), ostd::Error> {
        let idx = self
            .tx_index
            .fetch_add(1, core::sync::atomic::Ordering::SeqCst)
            % NUM_TX_DESC;

        // Spin-wait until descriptor is done (DD flag is 1)
        loop {
            let desc = self.get_tx_desc(idx)?;
            if (desc.status & 0x01) != 0 {
                break;
            }
            core::hint::spin_loop();
        }

        let write_len = core::cmp::min(packet.len(), 2048);
        self.tx_buffers
            .write_bytes(idx * 2048, &packet[..write_len])?;

        let buf_phys = self.tx_buffers.daddr() as u64 + (idx * 2048) as u64;

        // Configure Tx Descriptor: EOP (bit 0) | IFCS (bit 1) | RS (bit 3)
        let desc = TxDesc {
            addr: buf_phys,
            length: write_len as u16,
            cso: 0,
            cmd: 0x01 | 0x02 | 0x08,
            status: 0,
            css: 0,
            special: 0,
        };
        self.set_tx_desc(idx, &desc)?;

        // Update Tail
        self.write_reg(REG_TDT, ((idx + 1) % NUM_TX_DESC) as u32);
        Ok(())
    }

    fn recv(&self, buf: &mut [u8]) -> Result<usize, ostd::Error> {
        let idx = self.rx_index.load(core::sync::atomic::Ordering::Relaxed);
        let mut desc = self.get_rx_desc(idx)?;

        // Check if DD flag is set (bit 0 of status)
        if (desc.status & 0x01) == 0 {
            return Ok(0);
        }

        let len = desc.length as usize;
        let read_len = core::cmp::min(len, buf.len());

        self.rx_buffers
            .read_bytes(idx * 2048, &mut buf[..read_len])?;

        // Reset descriptor status/done flag for card reuse
        desc.status = 0;
        desc.length = 0;
        self.set_rx_desc(idx, &desc)?;

        // Update RDT tail register
        self.write_reg(REG_RDT, idx as u32);

        self.rx_index.store(
            (idx + 1) % NUM_RX_DESC,
            core::sync::atomic::Ordering::Relaxed,
        );
        Ok(read_len)
    }
}
