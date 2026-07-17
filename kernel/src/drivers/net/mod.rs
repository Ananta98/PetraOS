use crate::device::device::{Device, DeviceType};
use crate::device::manager::register_device;
use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::sync::Arc;
use crate::drivers::pci;
use ostd::sync::SpinLock;
use spin::Once;

pub mod e1000;
pub mod rtl8139;

/// Interface for network drivers in PetraOS.
pub trait NetDevice: Send + Sync {
    /// Return the MAC address of the network interface.
    fn mac_address(&self) -> [u8; 6];

    /// Send a packet over the interface.
    fn send(&self, packet: &[u8]) -> Result<(), ostd::Error>;

    /// Check and receive a packet from the interface if one is available.
    /// Returns the number of bytes read.
    fn recv(&self, buf: &mut [u8]) -> Result<usize, ostd::Error>;
}

/// Wrapper to integrate `NetDevice` into the kernel's unified device model.
struct NetDeviceWrapper {
    name: String,
    device: Arc<dyn NetDevice>,
}

impl Device for NetDeviceWrapper {
    fn name(&self) -> &str {
        &self.name
    }

    fn device_type(&self) -> DeviceType {
        DeviceType::Net
    }
}

pub fn register_net_device(name: &str, device: Arc<dyn NetDevice>) -> Result<(), ostd::Error> {
    let wrapper = Arc::new(NetDeviceWrapper {
        name: String::from(name),
        device,
    });
    register_device(wrapper)
}

/// Simulated NetDevice that loops back sent packets directly to the receive queue.
pub struct SimulatedNetDevice {
    mac: [u8; 6],
    rx_queue: SpinLock<VecDeque<alloc::vec::Vec<u8>>>,
}

impl SimulatedNetDevice {
    pub fn new() -> Self {
        Self {
            mac: [0x52, 0x54, 0x00, 0x12, 0x34, 0x56], // QEMU style MAC
            rx_queue: SpinLock::new(VecDeque::new()),
        }
    }
}

impl Default for SimulatedNetDevice {
    fn default() -> Self {
        Self::new()
    }
}

impl NetDevice for SimulatedNetDevice {
    fn mac_address(&self) -> [u8; 6] {
        self.mac
    }

    fn send(&self, packet: &[u8]) -> Result<(), ostd::Error> {
        // Loop back sent packets directly into receive queue
        self.rx_queue.lock().push_back(packet.to_vec());
        Ok(())
    }

    fn recv(&self, buf: &mut [u8]) -> Result<usize, ostd::Error> {
        let mut queue = self.rx_queue.lock();
        if let Some(packet) = queue.pop_front() {
            let len = core::cmp::min(packet.len(), buf.len());
            buf[..len].copy_from_slice(&packet[..len]);
            Ok(len)
        } else {
            Ok(0)
        }
    }
}

/// Global reference to the active network interface.
pub static DEFAULT_NET_DEVICE: Once<Arc<dyn NetDevice>> = Once::new();

/// Initialize network drivers.
///
/// Scans the PCI bus for an E1000 controller or an RTL8139 controller.
/// Otherwise, falls back to the simulated loopback interface.
pub fn init() {
    let mut physical_found = false;

    // Search for E1000 network card on the PCI bus (Vendor ID 0x8086, Device ID 0x100E)
    if let Some(pci_dev) = pci::find_device(0x8086, 0x100E) {
        if let Ok(e1000_dev) = e1000::E1000::new(&pci_dev) {
            let device = Arc::new(e1000_dev);
            if register_net_device("e1000", device.clone()).is_ok() {
                DEFAULT_NET_DEVICE.call_once(|| device);
                physical_found = true;
            }
        }
    }

    // Search for RTL8139 network card on the PCI bus (Vendor ID 0x10EC, Device ID 0x8139)
    if !physical_found {
        if let Some(pci_dev) = pci::find_device(0x10EC, 0x8139) {
            if let Ok(rtl) = rtl8139::Rtl8139::new(&pci_dev) {
                let device = Arc::new(rtl);
                if register_net_device("rtl8139", device.clone()).is_ok() {
                    DEFAULT_NET_DEVICE.call_once(|| device);
                    physical_found = true;
                }
            }
        }
    }

    if !physical_found {
        let device = Arc::new(SimulatedNetDevice::new());
        let _ = register_net_device("net", device.clone());
        DEFAULT_NET_DEVICE.call_once(|| device);
    }
}
