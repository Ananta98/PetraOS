//! Smoltcp PHY Device Adapter for PetraOS Network Drivers
//!
//! Connects smoltcp's PHY abstraction layer to kernel network drivers (Intel e1000).

use alloc::vec::Vec;
use smoltcp::phy::{Device, DeviceCapabilities, Medium, RxToken, TxToken};
use smoltcp::time::Instant;

use crate::drivers::net::intel::e1000::E1000_DEVICE;

/// Device token for consuming a received Ethernet frame.
pub struct E1000RxToken {
    buffer: Vec<u8>,
}

impl RxToken for E1000RxToken {
    fn consume<R, F>(self, f: F) -> R
    where
        F: FnOnce(&[u8]) -> R,
    {
        f(&self.buffer[..])
    }
}

/// Device token for transmitting an Ethernet frame.
pub struct E1000TxToken;

impl TxToken for E1000TxToken {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        let mut buffer = alloc::vec![0u8; len];
        let result = f(&mut buffer[..]);

        if let Some(ref mut e1000) = *E1000_DEVICE.lock() {
            let _ = e1000.send_packet(&buffer);
        }

        result
    }
}

/// Smoltcp device adapter wrapping the active kernel network controller.
pub struct PetraNetDevice;

impl Device for PetraNetDevice {
    type RxToken<'a> = E1000RxToken where Self: 'a;
    type TxToken<'a> = E1000TxToken where Self: 'a;

    fn receive(&mut self, _timestamp: Instant) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        let mut temp_buf = [0u8; 2048];
        let maybe_len = if let Some(ref mut e1000) = *E1000_DEVICE.lock() {
            e1000.receive_packet(&mut temp_buf).ok().flatten()
        } else {
            None
        };

        if let Some(len) = maybe_len {
            let mut packet = alloc::vec![0u8; len];
            packet.copy_from_slice(&temp_buf[..len]);
            Some((E1000RxToken { buffer: packet }, E1000TxToken))
        } else {
            None
        }
    }

    fn transmit(&mut self, _timestamp: Instant) -> Option<Self::TxToken<'_>> {
        Some(E1000TxToken)
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut caps = DeviceCapabilities::default();
        caps.max_transmission_unit = 1500;
        caps.medium = Medium::Ethernet;
        caps
    }
}
