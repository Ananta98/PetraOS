use crate::device::device::{Device, DeviceType};
use crate::drivers::net::NetDevice;
use crate::drivers::pci::{PciBar, PciDevice};
use alloc::sync::Arc;
use alloc::vec::Vec;
use ostd::arch::device::io_port::ReadWriteAccess;
use ostd::io::IoPort;
use ostd::mm::dma::DmaCoherent;
use ostd::mm::{HasDaddr, VmIo};

pub struct Rtl8139 {
    io_base: u32,
    rx_buffer: DmaCoherent,
    tx_buffers: Vec<DmaCoherent>,
    next_tx_desc: core::sync::atomic::AtomicUsize,
    rx_offset: core::sync::atomic::AtomicUsize,
    mac: [u8; 6],
}

impl Rtl8139 {
    pub fn new(pci_dev: &PciDevice) -> Result<Self, ostd::Error> {
        let io_base = match pci_dev.bars[0] {
            PciBar::IoSpace { port, .. } => port,
            _ => return Err(ostd::Error::InvalidArgs),
        };

        pci_dev.enable_io_space();
        pci_dev.enable_bus_mastering();

        // Allocate 3 pages (12KB) for the receive buffer.
        // RTL8139 requires 8KB + 16 bytes rx buffer, so 3 pages is sufficient.
        let rx_buffer = DmaCoherent::alloc(3, true)?;

        // Allocate 4 transmit buffers (1 page each)
        let mut tx_buffers = Vec::new();
        for _ in 0..4 {
            tx_buffers.push(DmaCoherent::alloc(1, true)?);
        }

        let mut rtl = Self {
            io_base,
            rx_buffer,
            tx_buffers,
            next_tx_desc: core::sync::atomic::AtomicUsize::new(0),
            rx_offset: core::sync::atomic::AtomicUsize::new(0),
            mac: [0; 6],
        };

        rtl.init_hardware()?;
        rtl.read_mac();
        Ok(rtl)
    }

    fn outb(&self, offset: u16, val: u8) {
        if let Ok(port) = IoPort::<u8, ReadWriteAccess>::acquire(self.io_base as u16 + offset) {
            port.write(val);
        }
    }

    fn outw(&self, offset: u16, val: u16) {
        if let Ok(port) = IoPort::<u16, ReadWriteAccess>::acquire(self.io_base as u16 + offset) {
            port.write(val);
        }
    }

    fn outd(&self, offset: u16, val: u32) {
        if let Ok(port) = IoPort::<u32, ReadWriteAccess>::acquire(self.io_base as u16 + offset) {
            port.write(val);
        }
    }

    fn inb(&self, offset: u16) -> u8 {
        if let Ok(port) = IoPort::<u8, ReadWriteAccess>::acquire(self.io_base as u16 + offset) {
            port.read()
        } else {
            0
        }
    }

    fn inw(&self, offset: u16) -> u16 {
        if let Ok(port) = IoPort::<u16, ReadWriteAccess>::acquire(self.io_base as u16 + offset) {
            port.read()
        } else {
            0
        }
    }

    fn ind(&self, offset: u16) -> u32 {
        if let Ok(port) = IoPort::<u32, ReadWriteAccess>::acquire(self.io_base as u16 + offset) {
            port.read()
        } else {
            0
        }
    }

    fn init_hardware(&self) -> Result<(), ostd::Error> {
        // Wake up / power on Config 1
        self.outb(0x52, 0x00);

        // Reset the device
        self.outb(0x37, 0x10);
        // Spin-wait until reset is complete
        while (self.inb(0x37) & 0x10) != 0 {
            core::hint::spin_loop();
        }

        // Configure receive buffer address
        let rx_phys = self.rx_buffer.daddr() as u32;
        self.outd(0x30, rx_phys);

        // Configure Interrupts: TOK | ROK (Receive OK and Transmit OK)
        self.outw(0x3C, 0x0005);

        // RxConfig: AAP | APM | AM | AB | WRAP (0x8F)
        self.outd(0x44, 0x8F);

        // Enable transmitter and receiver
        self.outb(0x37, 0x0C);

        // Initial CAPR offset
        self.outw(0x38, 0xFFF0);

        Ok(())
    }

    /// Read the MAC address from the hardware registers IDR0..5
    fn read_mac(&mut self) {
        let mut mac = [0u8; 6];
        for i in 0..6 {
            mac[i] = self.inb(i as u16);
        }
        self.mac = mac;
    }
}

impl NetDevice for Rtl8139 {
    fn mac_address(&self) -> [u8; 6] {
        self.mac
    }

    fn send(&self, packet: &[u8]) -> Result<(), ostd::Error> {
        let desc = self
            .next_tx_desc
            .fetch_add(1, core::sync::atomic::Ordering::SeqCst)
            % 4;
        let tsd_offset = 0x10 + (desc as u16) * 4;
        let tsad_offset = 0x20 + (desc as u16) * 4;

        // Spin-wait until descriptor is owned by host (OWN bit 13 is 1)
        while (self.ind(tsd_offset) & (1 << 13)) == 0 {
            core::hint::spin_loop();
        }

        self.tx_buffers[desc].write_bytes(0, packet)?;

        let phys = self.tx_buffers[desc].daddr() as u32;
        self.outd(tsad_offset, phys);

        self.outd(tsd_offset, packet.len() as u32 & 0x1FFF);
        Ok(())
    }

    fn recv(&self, buf: &mut [u8]) -> Result<usize, ostd::Error> {
        let cr = self.inb(0x37);
        if (cr & 0x01) != 0 {
            return Ok(0);
        }

        let rx_offset = self.rx_offset.load(core::sync::atomic::Ordering::Relaxed);

        let mut header = [0u8; 4];
        self.rx_buffer.read_bytes(rx_offset, &mut header)?;

        let status = u16::from_le_bytes([header[0], header[1]]);
        let len = u16::from_le_bytes([header[2], header[3]]) as usize;

        if len == 0 || (status & 0x01) == 0 {
            return Ok(0);
        }

        let data_len = len.saturating_sub(4);
        let read_len = core::cmp::min(data_len, buf.len());

        self.rx_buffer
            .read_bytes(rx_offset + 4, &mut buf[..read_len])?;

        let next_offset = (rx_offset + len + 4 + 3) & !3;
        let wrapped_offset = next_offset % 8192;
        self.rx_offset
            .store(wrapped_offset, core::sync::atomic::Ordering::Relaxed);

        let capr = (wrapped_offset as isize - 0x10) as u16;
        self.outw(0x38, capr);

        Ok(read_len)
    }
}
