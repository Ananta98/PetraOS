//! Intel 8254x (e1000) Hardware Device Controller
//!
//! Handles MMIO register programming, DMA ring allocation, packet transmission,
//! packet reception, and status monitoring for the Intel e1000 network adapter.

use alloc::vec::Vec;
use core::mem::size_of;

use crate::device::DriverError;
use crate::drivers::bus::pci::device::PciDevice;
use crate::drivers::pci::config;
use crate::mm::dma::DmaCoherent;
use crate::mm::map_mmio;

use super::descriptors::*;
use super::eeprom::read_mac_address;
use super::registers::*;

/// Number of receive descriptors in the circular ring (must be a multiple of 8).
pub const RX_NUM_DESCS: usize = 32;

/// Number of transmit descriptors in the circular ring (must be a multiple of 8).
pub const TX_NUM_DESCS: usize = 32;

/// Size in bytes for each packet buffer (2 KB standard Ethernet frame).
pub const BUFFER_SIZE: usize = 2048;

/// Intel e1000 device driver state.
pub struct E1000Device {
    pub pci_device: PciDevice,
    mmio_base: *mut u8,
    mac_addr: [u8; 6],
    rx_ring: DmaCoherent,
    rx_buffers: Vec<DmaCoherent>,
    rx_cur: usize,
    tx_ring: DmaCoherent,
    tx_buffers: Vec<DmaCoherent>,
    tx_cur: usize,
    link_up: bool,
}

// SAFETY: All MMIO pointer accesses and DMA ring accesses are synchronized via
// standard kernel locks (Mutex) when instantiated as a shared device.
unsafe impl Send for E1000Device {}
unsafe impl Sync for E1000Device {}

impl E1000Device {
    /// Probe and initialize an Intel e1000 controller from a discovered PCI device.
    pub fn new(pci_device: PciDevice) -> Result<Self, DriverError> {
        // Enable Bus Master and Memory Space in PCI Command register
        let mut cmd = config::read_u16(
            pci_device.bus,
            pci_device.device,
            pci_device.function,
            0x04,
        );
        cmd |= 0x0007; // Bus Master (0x04) | Memory Space (0x02) | I/O Space (0x01)
        config::write_u16(
            pci_device.bus,
            pci_device.device,
            pci_device.function,
            0x04,
            cmd,
        );

        // Read BAR0 (MMIO Base Address)
        let bar0 = config::read_u32(
            pci_device.bus,
            pci_device.device,
            pci_device.function,
            0x10,
        );
        if bar0 == 0 || bar0 == 0xFFFF_FFFF {
            return Err(DriverError::NoDevice);
        }

        let mmio_phys = (bar0 & 0xFFFF_FFF0) as u64;
        let mmio_size = 0x20000; // 128 KB standard e1000 MMIO size

        map_mmio(mmio_phys, mmio_size);

        let hhdm = crate::mm::hhdm_offset();
        let mmio_base = (mmio_phys + hhdm) as *mut u8;

        // Allocate DMA ring for RX descriptors
        let rx_ring_size = RX_NUM_DESCS * size_of::<RxDesc>();
        let rx_ring = DmaCoherent::alloc(rx_ring_size).map_err(|_| DriverError::AllocFailed)?;

        // Allocate RX data buffers
        let mut rx_buffers = Vec::with_capacity(RX_NUM_DESCS);
        for _ in 0..RX_NUM_DESCS {
            let buf = DmaCoherent::alloc(BUFFER_SIZE).map_err(|_| DriverError::AllocFailed)?;
            rx_buffers.push(buf);
        }

        // Allocate DMA ring for TX descriptors
        let tx_ring_size = TX_NUM_DESCS * size_of::<TxDesc>();
        let tx_ring = DmaCoherent::alloc(tx_ring_size).map_err(|_| DriverError::AllocFailed)?;

        // Allocate TX data buffers
        let mut tx_buffers = Vec::with_capacity(TX_NUM_DESCS);
        for _ in 0..TX_NUM_DESCS {
            let buf = DmaCoherent::alloc(BUFFER_SIZE).map_err(|_| DriverError::AllocFailed)?;
            tx_buffers.push(buf);
        }

        // Read hardware MAC address
        // SAFETY: mmio_base is mapped and exclusively assigned to this driver instance.
        let mac_addr = unsafe { read_mac_address(mmio_base) };

        let mut dev = Self {
            pci_device,
            mmio_base,
            mac_addr,
            rx_ring,
            rx_buffers,
            rx_cur: 0,
            tx_ring,
            tx_buffers,
            tx_cur: 0,
            link_up: false,
        };

        dev.init_hardware()?;
        Ok(dev)
    }

    /// Read a 32-bit MMIO register.
    #[inline]
    pub fn read_reg(&self, reg: usize) -> u32 {
        // SAFETY: reg is a valid offset within the mapped 128KB MMIO window.
        unsafe {
            let ptr = self.mmio_base.add(reg) as *const u32;
            core::ptr::read_volatile(ptr)
        }
    }

    /// Write a 32-bit MMIO register.
    #[inline]
    pub fn write_reg(&mut self, reg: usize, val: u32) {
        // SAFETY: reg is a valid offset within the mapped 128KB MMIO window.
        unsafe {
            let ptr = self.mmio_base.add(reg) as *mut u32;
            core::ptr::write_volatile(ptr, val);
        }
    }

    /// Initialize controller hardware, reset logic, setup RX/TX rings, and enable packet processing.
    pub fn init_hardware(&mut self) -> Result<(), DriverError> {
        // 1. Device Reset
        let ctrl = self.read_reg(REG_CTRL);
        self.write_reg(REG_CTRL, ctrl | CTRL_RST);

        // Wait for reset completion
        for _ in 0..10_000 {
            if (self.read_reg(REG_CTRL) & CTRL_RST) == 0 {
                break;
            }
            core::hint::spin_loop();
        }

        // 2. Clear Multicast Table Array (MTA)
        for i in 0..128 {
            self.write_reg(REG_MTA + i * 4, 0);
        }

        // 3. Configure Link Up and Auto-Speed Detection
        let mut ctrl = self.read_reg(REG_CTRL);
        ctrl |= CTRL_SLU | CTRL_ASDE | CTRL_FD;
        ctrl &= !(CTRL_ILOS | CTRL_FRCSPD | CTRL_FRCDPLX);
        self.write_reg(REG_CTRL, ctrl);

        // 4. Initialize Receive Ring
        let rx_descs_ptr = self.rx_ring.as_mut_ptr() as *mut RxDesc;
        for i in 0..RX_NUM_DESCS {
            let buf_phys = self.rx_buffers[i].phys().as_u64();
            // SAFETY: rx_descs_ptr points to our allocated DmaCoherent RX ring.
            unsafe {
                let desc = rx_descs_ptr.add(i);
                core::ptr::write_volatile(
                    desc,
                    RxDesc {
                        address: buf_phys,
                        length: 0,
                        checksum: 0,
                        status: 0,
                        errors: 0,
                        special: 0,
                    },
                );
            }
        }

        let rx_ring_phys = self.rx_ring.phys().as_u64();
        self.write_reg(REG_RDBAL, (rx_ring_phys & 0xFFFF_FFFF) as u32);
        self.write_reg(REG_RDBAH, ((rx_ring_phys >> 32) & 0xFFFF_FFFF) as u32);
        self.write_reg(
            REG_RDLEN,
            (RX_NUM_DESCS * size_of::<RxDesc>()) as u32,
        );
        self.write_reg(REG_RDH, 0);
        self.write_reg(REG_RDT, (RX_NUM_DESCS - 1) as u32);
        self.rx_cur = 0;

        // Enable receiver in RCTL
        let rctl = RCTL_EN
            | RCTL_BAM
            | RCTL_BSIZE_2048
            | RCTL_SECRC
            | RCTL_MPE
            | RCTL_UPE;
        self.write_reg(REG_RCTL, rctl);

        // 5. Initialize Transmit Ring
        let tx_descs_ptr = self.tx_ring.as_mut_ptr() as *mut TxDesc;
        for i in 0..TX_NUM_DESCS {
            let buf_phys = self.tx_buffers[i].phys().as_u64();
            // SAFETY: tx_descs_ptr points to our allocated DmaCoherent TX ring.
            unsafe {
                let desc = tx_descs_ptr.add(i);
                core::ptr::write_volatile(
                    desc,
                    TxDesc {
                        address: buf_phys,
                        length: 0,
                        cso: 0,
                        cmd: 0,
                        status: TXD_STAT_DD,
                        css: 0,
                        special: 0,
                    },
                );
            }
        }

        let tx_ring_phys = self.tx_ring.phys().as_u64();
        self.write_reg(REG_TDBAL, (tx_ring_phys & 0xFFFF_FFFF) as u32);
        self.write_reg(REG_TDBAH, ((tx_ring_phys >> 32) & 0xFFFF_FFFF) as u32);
        self.write_reg(
            REG_TDLEN,
            (TX_NUM_DESCS * size_of::<TxDesc>()) as u32,
        );
        self.write_reg(REG_TDH, 0);
        self.write_reg(REG_TDT, 0);
        self.tx_cur = 0;

        // Configure Transmit Control (TCTL) & Inter-Packet Gap (TIPG)
        let tctl = TCTL_EN
            | TCTL_PSP
            | (15 << TCTL_CT_SHIFT)
            | (64 << TCTL_COLD_SHIFT)
            | TCTL_RTLC;
        self.write_reg(REG_TCTL, tctl);

        // Recommended IPG values for IEEE 802.3
        let tipg = 10 | (8 << 10) | (6 << 20);
        self.write_reg(REG_TIPG, tipg);

        // 6. Program MAC address into Receive Address Filter 0
        let ral = (self.mac_addr[0] as u32)
            | ((self.mac_addr[1] as u32) << 8)
            | ((self.mac_addr[2] as u32) << 16)
            | ((self.mac_addr[3] as u32) << 24);
        let rah = (self.mac_addr[4] as u32)
            | ((self.mac_addr[5] as u32) << 8)
            | RAH_AV;
        self.write_reg(REG_RAL, ral);
        self.write_reg(REG_RAH, rah);

        // 7. Clear pending interrupts and unmask
        let _ = self.read_reg(REG_ICR);
        self.write_reg(REG_IMS, INT_RXT0 | INT_RXO | INT_LSC | INT_TXDW);

        // Check link status
        let status = self.read_reg(REG_STATUS);
        self.link_up = (status & STATUS_LU) != 0;

        Ok(())
    }

    /// Return the 6-byte hardware Ethernet MAC address.
    pub fn mac_address(&self) -> [u8; 6] {
        self.mac_addr
    }

    /// Return `true` if physical link is established.
    pub fn is_link_up(&mut self) -> bool {
        let status = self.read_reg(REG_STATUS);
        self.link_up = (status & STATUS_LU) != 0;
        self.link_up
    }

    /// Transmit an Ethernet frame.
    pub fn send_packet(&mut self, data: &[u8]) -> Result<(), DriverError> {
        if data.is_empty() || data.len() > BUFFER_SIZE {
            return Err(DriverError::InvalidBlock);
        }

        let idx = self.tx_cur;
        let tx_descs_ptr = self.tx_ring.as_mut_ptr() as *mut TxDesc;

        // Check if descriptor is available (DD bit set)
        // SAFETY: idx is bounded by TX_NUM_DESCS.
        let desc = unsafe { &mut *tx_descs_ptr.add(idx) };
        if (desc.status & TXD_STAT_DD) == 0 {
            return Err(DriverError::Timeout); // Transmit ring full
        }

        // Copy packet payload to DMA buffer
        let buf = &mut self.tx_buffers[idx];
        let slice = buf.as_mut_slice();
        slice[..data.len()].copy_from_slice(data);

        // Configure transmit descriptor
        desc.length = data.len() as u16;
        desc.cmd = TXD_CMD_EOP | TXD_CMD_IFCS | TXD_CMD_RS;
        desc.status = 0;

        // Update tail register to notify controller of new transmit job
        self.tx_cur = (idx + 1) % TX_NUM_DESCS;
        self.write_reg(REG_TDT, self.tx_cur as u32);

        Ok(())
    }

    /// Receive a pending Ethernet frame into `buf` if available.
    pub fn receive_packet(&mut self, buf: &mut [u8]) -> Result<Option<usize>, DriverError> {
        let idx = self.rx_cur;
        let rx_descs_ptr = self.rx_ring.as_mut_ptr() as *mut RxDesc;

        // Check if current descriptor is written back by hardware (DD bit set)
        // SAFETY: idx is bounded by RX_NUM_DESCS.
        let desc = unsafe { &mut *rx_descs_ptr.add(idx) };
        if (desc.status & RXD_STAT_DD) == 0 {
            return Ok(None); // No packet pending
        }

        let len = desc.length as usize;
        if len > buf.len() {
            return Err(DriverError::InvalidBlock);
        }

        // Copy received frame to output buffer
        let rx_buf = &self.rx_buffers[idx];
        buf[..len].copy_from_slice(&rx_buf.as_slice()[..len]);

        // Reset descriptor status for reuse
        desc.status = 0;

        // Advance tail register to return descriptor to hardware ring
        let old_tail = self.rx_cur;
        self.rx_cur = (idx + 1) % RX_NUM_DESCS;
        self.write_reg(REG_RDT, old_tail as u32);

        Ok(Some(len))
    }

    /// Non-destructively report whether the RX ring holds an unread frame.
    ///
    /// Unlike [`Self::receive_packet`] this only inspects the descriptor status
    /// bit and never advances the ring, so it is safe to call from poll paths.
    pub fn has_pending_rx(&mut self) -> bool {
        let idx = self.rx_cur;
        let rx_descs_ptr = self.rx_ring.as_mut_ptr() as *const RxDesc;

        // SAFETY: idx is bounded by RX_NUM_DESCS and the descriptor ring is a
        // DMA-coherent allocation exclusively owned by this driver instance.
        let desc = unsafe { &*rx_descs_ptr.add(idx) };
        (desc.status & RXD_STAT_DD) != 0
    }

    /// Non-destructively report whether the TX ring has a free descriptor.
    pub fn is_tx_ready(&mut self) -> bool {
        let idx = self.tx_cur;
        let tx_descs_ptr = self.tx_ring.as_mut_ptr() as *const TxDesc;

        // SAFETY: idx is bounded by TX_NUM_DESCS and the descriptor ring is a
        // DMA-coherent allocation exclusively owned by this driver instance.
        let desc = unsafe { &*tx_descs_ptr.add(idx) };
        (desc.status & TXD_STAT_DD) != 0
    }

    /// Acknowledge and return pending interrupt causes from ICR.
    pub fn handle_interrupt(&mut self) -> u32 {
        let icr = self.read_reg(REG_ICR);
        if (icr & INT_LSC) != 0 {
            let _ = self.is_link_up();
        }
        icr
    }
}
