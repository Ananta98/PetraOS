//! Intel 8254x (e1000) PCI Network Interface Driver
//!
//! Provides PCI probing, device registration, and module lifecycle management
//! for Intel e1000 Gigabit Ethernet controllers.

pub mod descriptors;
pub mod device;
pub mod eeprom;
pub mod registers;

pub use descriptors::{RxDesc, TxDesc};
pub use device::{BUFFER_SIZE, E1000Device, RX_NUM_DESCS, TX_NUM_DESCS};
pub use registers::*;

use alloc::boxed::Box;
use alloc::sync::Arc;

use crate::device::{Device, DeviceType, Driver, DriverError};
use crate::drivers::bus::pci::PciBus;
use crate::sync::Mutex;

/// Global Intel e1000 active device instance.
pub static E1000_DEVICE: Mutex<Option<E1000Device>> = Mutex::new(None);

/// Device manager proxy wrapper for Intel e1000 controller.
pub struct E1000DeviceRef;

impl Device for E1000DeviceRef {
    fn dev_type(&self) -> DeviceType {
        DeviceType::Network
    }

    fn name(&self) -> &'static str {
        "Intel e1000 Gigabit Ethernet"
    }

    fn dev_name(&self) -> Option<&'static str> {
        Some("eth0")
    }

    fn init(&mut self) -> Result<(), DriverError> {
        if let Some(ref mut dev) = *E1000_DEVICE.lock() {
            dev.init_hardware()
        } else {
            Err(DriverError::NoDevice)
        }
    }
}

/// Known Intel e1000 PCI Vendor and Device IDs
pub const INTEL_VENDOR_ID: u16 = 0x8086;

pub const INTEL_E1000_DEV_IDS: &[u16] = &[
    0x100E, // 82540EM Gigabit Ethernet (Default in QEMU e1000)
    0x1004, // 82543GC
    0x100F, // 82545EM
    0x107C, // 82541PI
    0x10D3, // 82574L Gigabit Network Connection
    0x153A, // I217-LM
    0x153B, // I217-V
];

#[derive(Default)]
pub struct IntelE1000Driver;

impl Driver for IntelE1000Driver {
    fn name(&self) -> &'static str {
        "e1000"
    }

    fn bus_name(&self) -> &'static str {
        "pci"
    }

    fn description(&self) -> &'static str {
        "Intel 8254x Gigabit Ethernet Controller Driver"
    }

    fn probe(&self) -> Result<(), DriverError> {
        let discovery = PciBus::enumerate();
        for i in 0..discovery.count {
            let pci_dev = discovery.devices[i];
            if pci_dev.vendor_id == INTEL_VENDOR_ID
                && INTEL_E1000_DEV_IDS.contains(&pci_dev.device_id)
            {
                log::info!(
                    "[e1000] Discovered Intel e1000 at PCI {}:{}:{} (Device ID: {:#x})",
                    pci_dev.bus,
                    pci_dev.device,
                    pci_dev.function,
                    pci_dev.device_id
                );

                match E1000Device::new(pci_dev) {
                    Ok(dev) => {
                        let mac = dev.mac_address();
                        log::info!(
                            "[e1000] MAC Address: {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                            mac[0],
                            mac[1],
                            mac[2],
                            mac[3],
                            mac[4],
                            mac[5]
                        );

                        *E1000_DEVICE.lock() = Some(dev);

                        let dev_ref: Arc<Mutex<Box<dyn Device>>> =
                            Arc::new(Mutex::new(Box::new(E1000DeviceRef)));
                        crate::device::DEVICE_MANAGER.write().register(dev_ref);

                        log::info!("[e1000] Registered device to DEVICE_MANAGER as eth0");
                        return Ok(());
                    }
                    Err(err) => {
                        log::error!("[e1000] Device initialization failed: {:?}", err);
                        return Err(err);
                    }
                }
            }
        }

        log::debug!("[e1000] No supported Intel e1000 PCI controller discovered");
        Err(DriverError::NoDevice)
    }
}

crate::MODULE_LICENSE!("BSD-2-Clause");
crate::MODULE_AUTHOR!("Ananta98");
crate::MODULE_DESCRIPTION!("Intel 8254x Gigabit Ethernet Network Controller Driver");
crate::MODULE_VERSION!("1.0.0");
crate::module_driver!(
    E1000_INITCALL,
    e1000_driver_init,
    "e1000",
    IntelE1000Driver
);
