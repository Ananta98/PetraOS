//! Smoltcp PHY Device Adapter for Network Drivers
//!
//! Connects smoltcp's PHY abstraction layer to generic kernel network devices (`NetDevice`).

use alloc::boxed::Box;
use alloc::sync::Arc;
use alloc::vec::Vec;
use smoltcp::phy::{Device, DeviceCapabilities, Medium, RxToken, TxToken};
use smoltcp::time::Instant;

use crate::device::Device as KernelDevice;
use crate::sync::Mutex;

/// Device token for consuming a received Ethernet frame.
pub struct NetRxToken {
    buffer: Vec<u8>,
}

impl RxToken for NetRxToken {
    fn consume<R, F>(self, f: F) -> R
    where
        F: FnOnce(&[u8]) -> R,
    {
        f(&self.buffer[..])
    }
}

/// Device token for transmitting an Ethernet frame.
pub struct NetTxToken {
    device: Arc<Mutex<Box<dyn KernelDevice>>>,
}

impl TxToken for NetTxToken {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        let mut buffer = alloc::vec![0u8; len];
        let result = f(&mut buffer[..]);

        let mut dev_guard = self.device.lock();
        if let Some(net_dev) = dev_guard.as_net_device_mut() {
            let _ = net_dev.send_packet(&buffer);
        }

        result
    }
}

/// Smoltcp PHY device adapter wrapping a generic kernel network controller.
pub struct NetDeviceAdapter {
    device: Arc<Mutex<Box<dyn KernelDevice>>>,
}

impl NetDeviceAdapter {
    /// Create a new PHY adapter wrapping the given network device.
    pub fn new(device: Arc<Mutex<Box<dyn KernelDevice>>>) -> Self {
        Self { device }
    }

    /// Access the underlying kernel device handle.
    pub fn device(&self) -> &Arc<Mutex<Box<dyn KernelDevice>>> {
        &self.device
    }
}

impl Device for NetDeviceAdapter {
    type RxToken<'a> = NetRxToken where Self: 'a;
    type TxToken<'a> = NetTxToken where Self: 'a;

    fn receive(&mut self, _timestamp: Instant) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        let mut temp_buf = [0u8; 2048];
        let mut dev_guard = self.device.lock();
        let maybe_len = dev_guard
            .as_net_device_mut()
            .and_then(|dev| dev.receive_packet(&mut temp_buf).ok().flatten());
        drop(dev_guard);

        if let Some(len) = maybe_len {
            let mut packet = alloc::vec![0u8; len];
            packet.copy_from_slice(&temp_buf[..len]);
            Some((
                NetRxToken { buffer: packet },
                NetTxToken {
                    device: self.device.clone(),
                },
            ))
        } else {
            None
        }
    }

    fn transmit(&mut self, _timestamp: Instant) -> Option<Self::TxToken<'_>> {
        Some(NetTxToken {
            device: self.device.clone(),
        })
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut caps = DeviceCapabilities::default();
        caps.max_transmission_unit = 1500;
        caps.medium = Medium::Ethernet;
        caps
    }
}

