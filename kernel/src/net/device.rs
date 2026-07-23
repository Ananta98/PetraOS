//! Network device physical layer adapter bridging PetraOS NetDevice to smoltcp Device trait.

use crate::drivers::net::NetDevice;
use alloc::sync::Arc;
use smoltcp::phy::{Device, DeviceCapabilities, RxToken, TxToken};
use smoltcp::time::Instant;

/// `SmoltcpDevice` wraps a reference to a driver implementing `NetDevice`.
pub struct SmoltcpDevice {
    pub device: Arc<dyn NetDevice>,
}

impl SmoltcpDevice {
    /// Create a new `SmoltcpDevice` wrapper.
    pub fn new(device: Arc<dyn NetDevice>) -> Self {
        Self { device }
    }
}

impl<'a> Device for SmoltcpDevice {
    type RxToken<'b>
        = SmoltcpRxToken
    where
        Self: 'b;
    type TxToken<'b>
        = SmoltcpTxToken
    where
        Self: 'b;

    fn receive(&mut self, _timestamp: Instant) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        let mut buf = alloc::vec![0u8; 2048];
        match self.device.recv(&mut buf) {
            Ok(len) if len > 0 => {
                buf.truncate(len);
                Some((
                    SmoltcpRxToken { buf },
                    SmoltcpTxToken {
                        device: self.device.clone(),
                    },
                ))
            }
            _ => None,
        }
    }

    fn transmit(&mut self, _timestamp: Instant) -> Option<Self::TxToken<'_>> {
        Some(SmoltcpTxToken {
            device: self.device.clone(),
        })
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut caps = DeviceCapabilities::default();
        caps.max_transmission_unit = 1500;
        caps.medium = smoltcp::phy::Medium::Ethernet;
        caps
    }
}

pub struct SmoltcpRxToken {
    buf: alloc::vec::Vec<u8>,
}

impl RxToken for SmoltcpRxToken {
    fn consume<R, F>(mut self, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        f(&mut self.buf)
    }
}

pub struct SmoltcpTxToken {
    device: Arc<dyn NetDevice>,
}

impl TxToken for SmoltcpTxToken {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        let mut buf = alloc::vec![0u8; len];
        let res = f(&mut buf);
        let _ = self.device.send(&buf);
        res
    }
}
