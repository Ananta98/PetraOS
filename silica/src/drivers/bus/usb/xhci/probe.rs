// SPDX-License-Identifier: GPL-2.0

//! xHCI Root Hub Port Scanning and USB Device Probing.
//!
//! Enumerates connected USB devices, configures device slots and default control endpoints,
//! queries standard USB descriptors, and binds HID devices (Keyboards & Mice)
//! and Mass Storage devices to the kernel.

use alloc::{sync::Arc, vec::Vec};
use ostd::mm::{HasPaddr, VmWriter, dma::DmaCoherent, io::util::HasVmReaderWriter};
use spin::Mutex;

use super::{
    controller::XhciController,
    ring::{
        TRB_TYPE_ADDRESS_DEVICE, TRB_TYPE_DATA_STAGE, TRB_TYPE_ENABLE_SLOT, TRB_TYPE_SETUP_STAGE,
        TRB_TYPE_STATUS_STAGE, TransferRing,
    },
};
use crate::{
    drivers::{
        BlockDevice,
        bus::usb::{
            DeviceDescriptor, InterfaceDescriptor, USB_CLASS_HID, USB_CLASS_MASS_STORAGE,
            USB_DESC_TYPE_CONFIGURATION, USB_DESC_TYPE_INTERFACE, USB_REQ_GET_DESCRIPTOR,
            USB_REQ_SET_CONFIGURATION,
            hid::{HidDeviceType, probe_hid_interface},
            storage::probe_storage_interface,
        },
    },
    uapi::errno::{Errno, Result},
};

/// Represents an active enumerated USB device managed by xHCI.
pub struct XhciUsbDevice {
    pub slot_id: u8,
    pub port_idx: usize,
    pub ep0_ring: Mutex<TransferRing>,
    pub dev_desc: DeviceDescriptor,
    pub interfaces: Vec<InterfaceDescriptor>,
}

/// Global list of discovered and configured xHCI USB devices.
pub static XHCI_DEVICES: Mutex<Vec<Arc<XhciUsbDevice>>> = Mutex::new(Vec::new());

/// Scans all root hub ports, detects connected devices, and performs USB device initialization.
pub fn probe_root_hub(controller: &mut XhciController) -> Result<()> {
    let num_ports = controller.num_ports;
    ostd::info!(
        "xHCI: Scanning {} Root Hub ports for USB devices...",
        num_ports
    );

    for port_idx in 0..(num_ports as usize) {
        if controller.is_port_connected(port_idx) {
            ostd::info!(
                "xHCI: Detected USB device connected on Root Hub Port {}",
                port_idx + 1
            );

            // 1. Reset port to transition device into Default State
            if let Err(err) = controller.reset_port(port_idx) {
                ostd::warn!("xHCI: Failed to reset port {}: {:?}", port_idx + 1, err);
                continue;
            }

            // Read port speed
            let speed = controller.port_speed(port_idx);
            ostd::info!(
                "xHCI: Port {} enabled with speed ID: {} (1=FS, 2=LS, 3=HS, 4=SS)",
                port_idx + 1,
                speed
            );

            // 2. Enable Slot Command
            let slot_trb = match controller.send_command(0, 0, TRB_TYPE_ENABLE_SLOT << 10) {
                Ok(trb) => trb,
                Err(err) => {
                    ostd::warn!(
                        "xHCI: Enable Slot failed on port {}: {:?}",
                        port_idx + 1,
                        err
                    );
                    continue;
                }
            };

            let slot_id = slot_trb.slot_id();
            if slot_id == 0 {
                ostd::warn!("xHCI: Controller returned invalid slot ID 0");
                continue;
            }
            ostd::info!(
                "xHCI: Assigned Slot ID {} for device on Port {}",
                slot_id,
                port_idx + 1
            );

            // 3. Allocate Output Context and register in DCBAA
            let out_ctx_dma = match DmaCoherent::alloc(1, true) {
                Ok(dma) => dma,
                Err(_) => continue,
            };
            let out_ctx_paddr = out_ctx_dma.paddr() as u64;

            // Write Output Context pointer to DCBAA at slot_id
            let dcbaa_offset = (slot_id as usize) * 8;
            let mut dcbaa_writer = controller.dcbaa.writer();
            dcbaa_writer.skip(dcbaa_offset);
            let _ = dcbaa_writer.write_val(&out_ctx_paddr);

            // 4. Allocate Input Context and EP0 Transfer Ring
            let in_ctx_dma = match DmaCoherent::alloc(1, true) {
                Ok(dma) => dma,
                Err(_) => continue,
            };
            let in_ctx_paddr = in_ctx_dma.paddr() as u64;

            let ep0_ring = match TransferRing::new() {
                Ok(ring) => ring,
                Err(_) => continue,
            };
            let ep0_ring_paddr = ep0_ring.paddr();

            // Determine context size (32 bytes vs 64 bytes)
            let ctx_size = if controller.is_csz_64 { 64 } else { 32 };
            let slot_ctx_offset = ctx_size;
            let ep0_ctx_offset = 2 * ctx_size;

            // Setup Input Control Context at offset 0:
            // Dword 0: Drop Context flags
            // Dword 1: Add Context flags (Slot Context bit 0, EP0 bit 1 -> 0b11)
            let mut in_writer = in_ctx_dma.writer();
            let _ = in_writer.write_val(&0u32); // Drop flags
            let _ = in_writer.write_val(&0b11u32); // Add flags: Slot (bit 0) + EP0 (bit 1)

            // Setup Slot Context at slot_ctx_offset:
            // Route string: 0, Speed: speed, Context Entries: 1, Root Hub Port Num: port_idx + 1
            in_writer = in_ctx_dma.writer();
            in_writer.skip(slot_ctx_offset);
            let slot_dw0 = ((speed as u32) << 20) | (1u32 << 27); // Context Entries = 1
            let slot_dw1 = (port_idx as u32 + 1) << 16; // Root Hub Port Number
            let _ = in_writer.write_val(&slot_dw0);
            let _ = in_writer.write_val(&slot_dw1);

            // Setup Endpoint 0 Context at ep0_ctx_offset:
            // EP Type: 4 (Control Bidirectional), Max Packet Size: 64 or 512, TR Dequeue Pointer: ep0_ring_paddr
            in_writer = in_ctx_dma.writer();
            in_writer.skip(ep0_ctx_offset);
            let max_packet = if speed >= 4 { 512u32 } else { 64u32 };
            let ep0_dw1 = (max_packet << 16) | (3 << 1) | (4 << 3); // CErr=3, EPType=4 (Control)
            let _ = in_writer.write_val(&0u32);
            let _ = in_writer.write_val(&ep0_dw1);
            let _ = in_writer.write_val(&(ep0_ring_paddr | 1)); // DCS = 1

            // 5. Address Device Command
            let addr_ctrl = (TRB_TYPE_ADDRESS_DEVICE << 10) | ((slot_id as u32) << 24);
            if let Err(err) = controller.send_command(in_ctx_paddr, 0, addr_ctrl) {
                ostd::warn!(
                    "xHCI: Address Device command failed on slot {}: {:?}",
                    slot_id,
                    err
                );
                continue;
            }
            ostd::info!(
                "xHCI: Device on Slot {} transitioned to Addressed State",
                slot_id
            );

            // 6. Enumerate Device & Probe Descriptors
            let mut dev = XhciUsbDevice {
                slot_id,
                port_idx,
                ep0_ring: Mutex::new(ep0_ring),
                dev_desc: DeviceDescriptor::default(),
                interfaces: Vec::new(),
            };

            // Read Device Descriptor (18 bytes)
            if let Ok(desc) = read_device_descriptor(controller, &mut dev) {
                dev.dev_desc = desc;
                let vendor_id = desc.id_vendor;
                let product_id = desc.id_product;
                ostd::info!(
                    "xHCI: Discovered USB Device [{:04X}:{:04X}] (Class: 0x{:02X}, SubClass: 0x{:02X})",
                    vendor_id,
                    product_id,
                    desc.device_class,
                    desc.device_subclass
                );

                // Read Configuration Descriptor and interfaces
                if let Ok(interfaces) = read_device_interfaces(controller, &mut dev) {
                    dev.interfaces = interfaces;

                    // Set Configuration 1 to move device from Address State to Configured State
                    if let Err(err) = set_device_configuration(controller, &dev, 1) {
                        ostd::warn!(
                            "xHCI: Failed to set device configuration 1 on slot {}: {:?}",
                            slot_id,
                            err
                        );
                    }

                    for iface in &dev.interfaces {
                        if iface.interface_class == USB_CLASS_HID {
                            // Set HID Boot Protocol (0) and disable idle repeat
                            let _ = set_hid_boot_protocol(controller, &dev, iface.interface_number);
                            let _ = set_hid_idle(controller, &dev, iface.interface_number);

                            match probe_hid_interface(iface) {
                                Ok(HidDeviceType::Keyboard) => {
                                    ostd::info!(
                                        "xHCI: Successfully bound USB Keyboard on slot {}",
                                        slot_id
                                    );
                                }
                                Ok(HidDeviceType::Mouse) => {
                                    ostd::info!(
                                        "xHCI: Successfully bound USB Mouse on slot {}",
                                        slot_id
                                    );
                                }
                                Ok(HidDeviceType::Generic) => {
                                    ostd::info!("xHCI: Bound generic USB HID on slot {}", slot_id);
                                }
                                Err(err) => {
                                    ostd::warn!("xHCI: HID probe failed: {:?}", err);
                                }
                            }
                        } else if iface.interface_class == USB_CLASS_MASS_STORAGE {
                            match probe_storage_interface(slot_id, iface) {
                                Ok(sdev) => {
                                    ostd::info!(
                                        "xHCI: Successfully bound USB Mass Storage \"/dev/{}\" on slot {}",
                                        sdev.name(),
                                        slot_id
                                    );
                                }
                                Err(err) => {
                                    ostd::warn!("xHCI: Storage probe failed: {:?}", err);
                                }
                            }
                        }
                    }
                }
            }

            XHCI_DEVICES.lock().push(Arc::new(dev));
        }
    }

    Ok(())
}

/// Issues a USB Control Transfer on Endpoint 0 to query the Device Descriptor.
fn read_device_descriptor(
    controller: &XhciController,
    dev: &mut XhciUsbDevice,
) -> Result<DeviceDescriptor> {
    let dma = DmaCoherent::alloc(1, true).map_err(|_| Errno::ENOMEM)?;
    let paddr = dma.paddr() as u64;

    // USB Setup Packet for GET_DESCRIPTOR: Device
    let setup_low: u32 = 0x80 | ((USB_REQ_GET_DESCRIPTOR as u32) << 8) | (1u32 << 24);
    let setup_high: u32 = 18 << 16;
    let setup_param = (setup_low as u64) | ((setup_high as u64) << 32);

    // Enqueue Setup Stage TRB
    let setup_ctrl = (TRB_TYPE_SETUP_STAGE << 10) | (3 << 6) | (1 << 5); // Transfer Type 3 (In), Immediate Data
    let mut ring = dev.ep0_ring.lock();
    ring.enqueue(setup_param, 8, setup_ctrl)?;

    // Enqueue Data Stage TRB (18 bytes into buffer)
    let data_ctrl = (TRB_TYPE_DATA_STAGE << 10) | (1 << 16); // Dir = IN (1)
    ring.enqueue(paddr, 18, data_ctrl)?;

    // Enqueue Status Stage TRB (Dir = OUT (0))
    let status_ctrl = (TRB_TYPE_STATUS_STAGE << 10) | (1 << 5); // IOC = 1
    let status_trb_paddr = ring.enqueue(0, 0, status_ctrl)?;
    drop(ring);

    // Ring Doorbell for EP0 (Target 1)
    controller.ring_doorbell(dev.slot_id, 1);

    // Wait for transfer completion
    controller.wait_transfer(dev.slot_id, status_trb_paddr)?;

    // Read result descriptor from DMA buffer
    let mut reader = dma.reader();
    let length: u8 = reader.read_val().unwrap_or(0);
    let descriptor_type: u8 = reader.read_val().unwrap_or(0);
    let bcd_usb: u16 = reader.read_val().unwrap_or(0x0200);
    let device_class: u8 = reader.read_val().unwrap_or(0);
    let device_subclass: u8 = reader.read_val().unwrap_or(0);
    let device_protocol: u8 = reader.read_val().unwrap_or(0);
    let max_packet_size0: u8 = reader.read_val().unwrap_or(64);
    let id_vendor: u16 = reader.read_val().unwrap_or(0);
    let id_product: u16 = reader.read_val().unwrap_or(0);
    let bcd_device: u16 = reader.read_val().unwrap_or(0);
    let manufacturer_str_idx: u8 = reader.read_val().unwrap_or(0);
    let product_str_idx: u8 = reader.read_val().unwrap_or(0);
    let serial_str_idx: u8 = reader.read_val().unwrap_or(0);
    let num_configurations: u8 = reader.read_val().unwrap_or(1);

    Ok(DeviceDescriptor {
        length,
        descriptor_type,
        bcd_usb,
        device_class,
        device_subclass,
        device_protocol,
        max_packet_size0,
        id_vendor,
        id_product,
        bcd_device,
        manufacturer_str_idx,
        product_str_idx,
        serial_str_idx,
        num_configurations,
    })
}

/// Issues a USB Control Transfer on Endpoint 0 to query the Configuration and Interface Descriptors.
fn read_device_interfaces(
    controller: &XhciController,
    dev: &mut XhciUsbDevice,
) -> Result<Vec<InterfaceDescriptor>> {
    let dma = DmaCoherent::alloc(1, true).map_err(|_| Errno::ENOMEM)?;
    let paddr = dma.paddr() as u64;

    // First request: read 9-byte Configuration Descriptor header to know total_length
    let setup_low: u32 = 0x80
        | ((USB_REQ_GET_DESCRIPTOR as u32) << 8)
        | ((USB_DESC_TYPE_CONFIGURATION as u32) << 24);
    let setup_high: u32 = 9 << 16;
    let setup_param = (setup_low as u64) | ((setup_high as u64) << 32);

    let mut ring = dev.ep0_ring.lock();
    ring.enqueue(
        setup_param,
        8,
        (TRB_TYPE_SETUP_STAGE << 10) | (3 << 6) | (1 << 5),
    )?;
    ring.enqueue(paddr, 9, (TRB_TYPE_DATA_STAGE << 10) | (1 << 16))?;
    let status_trb_paddr = ring.enqueue(0, 0, (TRB_TYPE_STATUS_STAGE << 10) | (1 << 5))?;
    drop(ring);

    controller.ring_doorbell(dev.slot_id, 1);
    controller.wait_transfer(dev.slot_id, status_trb_paddr)?;

    let mut reader = dma.reader();
    let _length: u8 = reader.read_val().unwrap_or(9);
    let _descriptor_type: u8 = reader.read_val().unwrap_or(USB_DESC_TYPE_CONFIGURATION);
    let total_length: u16 = reader.read_val().unwrap_or(9);
    let _num_interfaces: u8 = reader.read_val().unwrap_or(1);

    let mut interfaces = Vec::new();

    // If total length exceeds standard header, fetch full configuration blob
    let read_len = (total_length as usize).min(1024).max(9);
    let full_setup_high: u32 = (read_len as u32) << 16;
    let full_setup_param = (setup_low as u64) | ((full_setup_high as u64) << 32);

    let mut ring = dev.ep0_ring.lock();
    ring.enqueue(
        full_setup_param,
        8,
        (TRB_TYPE_SETUP_STAGE << 10) | (3 << 6) | (1 << 5),
    )?;
    ring.enqueue(
        paddr,
        read_len as u32,
        (TRB_TYPE_DATA_STAGE << 10) | (1 << 16),
    )?;
    let full_status_paddr = ring.enqueue(0, 0, (TRB_TYPE_STATUS_STAGE << 10) | (1 << 5))?;
    drop(ring);

    controller.ring_doorbell(dev.slot_id, 1);
    controller.wait_transfer(dev.slot_id, full_status_paddr)?;

    // Parse interface descriptors from buffer
    let mut buf = [0u8; 1024];
    let mut dma_reader = dma.reader();
    let mut writer = VmWriter::from(&mut buf[..read_len]);
    let count = dma_reader.read(&mut writer);

    let mut offset = 0;
    while offset + 2 <= count {
        let desc_len = buf[offset] as usize;
        let desc_type = buf[offset + 1];

        if desc_len == 0 || offset + desc_len > count {
            break;
        }

        if desc_type == USB_DESC_TYPE_INTERFACE && desc_len >= 9 {
            interfaces.push(InterfaceDescriptor {
                length: buf[offset],
                descriptor_type: buf[offset + 1],
                interface_number: buf[offset + 2],
                alternate_setting: buf[offset + 3],
                num_endpoints: buf[offset + 4],
                interface_class: buf[offset + 5],
                interface_subclass: buf[offset + 6],
                interface_protocol: buf[offset + 7],
                interface_str_idx: buf[offset + 8],
            });
        }

        offset += desc_len;
    }

    Ok(interfaces)
}

/// Issues a USB Control Transfer on Endpoint 0 to set the active device configuration.
pub fn set_device_configuration(
    controller: &XhciController,
    dev: &XhciUsbDevice,
    config_val: u8,
) -> Result<()> {
    let setup_low: u32 =
        0x00 | ((USB_REQ_SET_CONFIGURATION as u32) << 8) | ((config_val as u32) << 16);
    let setup_param = setup_low as u64;

    let mut ring = dev.ep0_ring.lock();
    ring.enqueue(
        setup_param,
        8,
        (TRB_TYPE_SETUP_STAGE << 10) | (0 << 6) | (1 << 5),
    )?;
    let status_trb_paddr =
        ring.enqueue(0, 0, (TRB_TYPE_STATUS_STAGE << 10) | (1 << 16) | (1 << 5))?;
    drop(ring);

    controller.ring_doorbell(dev.slot_id, 1);
    controller.wait_transfer(dev.slot_id, status_trb_paddr)?;
    ostd::info!(
        "xHCI: Device on slot {} activated with Configuration {}",
        dev.slot_id,
        config_val
    );
    Ok(())
}

/// Issues a USB HID Class Control Transfer to select the Boot Protocol (protocol = 0).
pub fn set_hid_boot_protocol(
    controller: &XhciController,
    dev: &XhciUsbDevice,
    iface_num: u8,
) -> Result<()> {
    // SET_PROTOCOL (0x0B), wValue = 0 (Boot Protocol), wIndex = iface_num
    let setup_low: u32 = 0x21 | (0x0B << 8); // bmRequestType=0x21 (Class, Interface), bRequest=0x0B
    let setup_high: u32 = (iface_num as u32) << 16;
    let setup_param = (setup_low as u64) | ((setup_high as u64) << 32);

    let mut ring = dev.ep0_ring.lock();
    ring.enqueue(
        setup_param,
        8,
        (TRB_TYPE_SETUP_STAGE << 10) | (0 << 6) | (1 << 5),
    )?;
    let status_trb_paddr =
        ring.enqueue(0, 0, (TRB_TYPE_STATUS_STAGE << 10) | (1 << 16) | (1 << 5))?;
    drop(ring);

    controller.ring_doorbell(dev.slot_id, 1);
    controller.wait_transfer(dev.slot_id, status_trb_paddr)?;
    ostd::info!(
        "xHCI: Selected HID Boot Protocol on slot {} (interface {})",
        dev.slot_id,
        iface_num
    );
    Ok(())
}

/// Issues a USB HID Class Control Transfer to set report idle rate to 0 (reports only on state change).
pub fn set_hid_idle(controller: &XhciController, dev: &XhciUsbDevice, iface_num: u8) -> Result<()> {
    // SET_IDLE (0x0A), wValue = 0 (duration=0), wIndex = iface_num
    let setup_low: u32 = 0x21 | (0x0A << 8); // bmRequestType=0x21, bRequest=0x0A
    let setup_high: u32 = (iface_num as u32) << 16;
    let setup_param = (setup_low as u64) | ((setup_high as u64) << 32);

    let mut ring = dev.ep0_ring.lock();
    ring.enqueue(
        setup_param,
        8,
        (TRB_TYPE_SETUP_STAGE << 10) | (0 << 6) | (1 << 5),
    )?;
    let status_trb_paddr =
        ring.enqueue(0, 0, (TRB_TYPE_STATUS_STAGE << 10) | (1 << 16) | (1 << 5))?;
    drop(ring);

    controller.ring_doorbell(dev.slot_id, 1);
    controller.wait_transfer(dev.slot_id, status_trb_paddr)?;
    Ok(())
}
