use crate::device::device::{Device, DeviceType};
use crate::device::manager::register_device;
use crate::drivers::pci;
use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::sync::Arc;
use ostd::sync::SpinLock;
use spin::Once;

pub mod e1000;
pub mod rtl8139;

/// Global reference to the active network interface.
pub static DEFAULT_NET_DEVICE: Once<Arc<dyn NetDevice>> = Once::new();

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
