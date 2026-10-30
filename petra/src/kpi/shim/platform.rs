// SPDX-License-Identifier: GPL-2.0

//! Linux `platform_driver_register` C-ABI bridge.
//!
//! Adapts C-style `struct platform_driver` into PetraOS's safe `PlatformDriver` trait.

use alloc::{ffi::CString, string::String, sync::Arc, vec::Vec};
use core::ffi::c_int;

use super::types::{
    dev_get_drvdata, LinuxDevice, LinuxPlatformDevice, LinuxPlatformDriver, LinuxResource,
    IORESOURCE_IRQ, IORESOURCE_MEM,
};
use crate::kpi::device::{platform_driver_register, PlatformDevice, PlatformDriver, ResourceType};
use crate::uapi::errno::Result;


/// Safe Rust wrapper around a C-style `LinuxPlatformDriver`.
pub struct CPlatformDriverAdapter {
    c_driver: LinuxPlatformDriver,
    name: String,
}

impl CPlatformDriverAdapter {
    pub fn new(c_driver: LinuxPlatformDriver) -> Self {
        let name = if !c_driver.name.is_null() {
            unsafe {
                core::ffi::CStr::from_ptr(c_driver.name)
                    .to_str()
                    .unwrap_or("c_platform_drv")
                    .into()
            }
        } else {
            String::from("c_platform_drv")
        };

        Self { c_driver, name }
    }
}

impl PlatformDriver for CPlatformDriverAdapter {
    fn name(&self) -> &str {
        &self.name
    }

    fn probe(&self, pdev: &Arc<PlatformDevice>) -> Result<()> {
        let probe_fn = match self.c_driver.probe {
            Some(f) => f,
            None => return Ok(()),
        };

        // Convert safe PlatformDevice resources into C LinuxResource array
        let mut c_resources: Vec<LinuxResource> = pdev
            .resources()
            .iter()
            .map(|r| LinuxResource {
                start: r.start,
                end: r.end,
                name: core::ptr::null(),
                flags: match r.resource_type {
                    ResourceType::Mem => IORESOURCE_MEM,
                    ResourceType::Irq => IORESOURCE_IRQ,
                    ResourceType::Io => super::types::IORESOURCE_IO,
                },
            })
            .collect();

        let c_name = CString::new(pdev.name()).unwrap_or_default();
        let mut c_pdev = LinuxPlatformDevice {
            name: c_name.as_ptr(),
            id: pdev.id(),
            dev: LinuxDevice {
                name: c_name.as_ptr(),
                parent: core::ptr::null_mut(),
                driver_data: core::ptr::null_mut(),
            },
            num_resources: c_resources.len() as u32,
            resource: c_resources.as_mut_ptr(),
        };

        let ret = unsafe { probe_fn(&mut c_pdev) };
        if ret < 0 {
            // Linux negative return code convention -> translate to Errno
            return Err(super::types::errno_from_code(-ret));
        }

        // Preserve any drvdata stored by the driver in the PetraOS core device
        let drvdata = unsafe { dev_get_drvdata(&c_pdev.dev) };
        if !drvdata.is_null() {
            pdev.device().set_drvdata(drvdata as usize);
        }

        Ok(())
    }

    fn remove(&self, pdev: &Arc<PlatformDevice>) -> Result<()> {
        let remove_fn = match self.c_driver.remove {
            Some(f) => f,
            None => return Ok(()),
        };

        // Convert safe PlatformDevice resources into C LinuxResource array for teardown
        let mut c_resources: Vec<LinuxResource> = pdev
            .resources()
            .iter()
            .map(|r| LinuxResource {
                start: r.start,
                end: r.end,
                name: core::ptr::null(),
                flags: match r.resource_type {
                    ResourceType::Mem => IORESOURCE_MEM,
                    ResourceType::Irq => IORESOURCE_IRQ,
                    ResourceType::Io => super::types::IORESOURCE_IO,
                },
            })
            .collect();

        let c_name = CString::new(pdev.name()).unwrap_or_default();
        let drvdata_val = pdev
            .device()
            .with_drvdata::<usize, _>(|&val| val)
            .unwrap_or(0);

        let mut c_pdev = LinuxPlatformDevice {
            name: c_name.as_ptr(),
            id: pdev.id(),
            dev: LinuxDevice {
                name: c_name.as_ptr(),
                parent: core::ptr::null_mut(),
                driver_data: drvdata_val as *mut core::ffi::c_void,
            },
            num_resources: c_resources.len() as u32,
            resource: c_resources.as_mut_ptr(),
        };

        let ret = unsafe { remove_fn(&mut c_pdev) };
        if ret < 0 {
            return Err(super::types::errno_from_code(-ret));
        }

        // Clean up driver data upon successful removal
        let _ = pdev.device().take_drvdata::<usize>();

        Ok(())
    }
}

/// Linux `platform_driver_register`: C-ABI entry point for registering platform drivers.
pub unsafe extern "C" fn linux_platform_driver_register(
    driver: *const LinuxPlatformDriver,
) -> c_int {
    if driver.is_null() {
        return -14; // -EFAULT
    }

    let c_drv = unsafe { *driver };
    let adapter = Arc::new(CPlatformDriverAdapter::new(c_drv));
    match platform_driver_register(adapter) {
        Ok(()) => 0,
        Err(e) => -(e.as_i32()),
    }
}

