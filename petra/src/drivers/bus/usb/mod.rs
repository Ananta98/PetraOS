// SPDX-License-Identifier: GPL-2.0

//! Universal Serial Bus (USB) Core Subsystem.
//!
//! Provides standard USB descriptor definitions, request structures,
//! device abstractions, and integrates the xHCI host controller driver.

pub mod hid;
pub mod storage;
pub mod xhci;

use alloc::vec::Vec;

// ----------------------------------------------------------------------------
// USB Standard Request Constants
// ----------------------------------------------------------------------------

pub const USB_REQ_GET_STATUS: u8 = 0;
pub const USB_REQ_CLEAR_FEATURE: u8 = 1;
pub const USB_REQ_SET_FEATURE: u8 = 3;
pub const USB_REQ_SET_ADDRESS: u8 = 5;
pub const USB_REQ_GET_DESCRIPTOR: u8 = 6;
pub const USB_REQ_SET_DESCRIPTOR: u8 = 7;
pub const USB_REQ_GET_CONFIGURATION: u8 = 8;
pub const USB_REQ_SET_CONFIGURATION: u8 = 9;
pub const USB_REQ_GET_INTERFACE: u8 = 10;
pub const USB_REQ_SET_INTERFACE: u8 = 11;

// ----------------------------------------------------------------------------
// Descriptor Types
// ----------------------------------------------------------------------------

pub const USB_DESC_TYPE_DEVICE: u8 = 1;
pub const USB_DESC_TYPE_CONFIGURATION: u8 = 2;
pub const USB_DESC_TYPE_STRING: u8 = 3;
pub const USB_DESC_TYPE_INTERFACE: u8 = 4;
pub const USB_DESC_TYPE_ENDPOINT: u8 = 5;
pub const USB_DESC_TYPE_HID: u8 = 0x21;
pub const USB_DESC_TYPE_REPORT: u8 = 0x22;

// ----------------------------------------------------------------------------
// USB Classes
// ----------------------------------------------------------------------------

pub const USB_CLASS_PER_INTERFACE: u8 = 0x00;
pub const USB_CLASS_AUDIO: u8 = 0x01;
pub const USB_CLASS_CDC: u8 = 0x02;
pub const USB_CLASS_HID: u8 = 0x03;
pub const USB_CLASS_PHYSICAL: u8 = 0x05;
pub const USB_CLASS_IMAGE: u8 = 0x06;
pub const USB_CLASS_PRINTER: u8 = 0x07;
pub const USB_CLASS_MASS_STORAGE: u8 = 0x08;
pub const USB_CLASS_HUB: u8 = 0x09;
pub const USB_CLASS_CDC_DATA: u8 = 0x0A;
pub const USB_CLASS_SMART_CARD: u8 = 0x0B;
pub const USB_CLASS_SECURITY: u8 = 0x0D;
pub const USB_CLASS_VIDEO: u8 = 0x0E;
pub const USB_CLASS_WIRELESS: u8 = 0xE0;

// HID Subclasses & Protocols
pub const USB_HID_SUBCLASS_BOOT: u8 = 0x01;
pub const USB_HID_PROTOCOL_KEYBOARD: u8 = 0x01;
pub const USB_HID_PROTOCOL_MOUSE: u8 = 0x02;

// Mass Storage Subclasses & Protocols
pub const USB_STORAGE_SUBCLASS_RBC: u8 = 0x01;
pub const USB_STORAGE_SUBCLASS_ATAPI: u8 = 0x02;
pub const USB_STORAGE_SUBCLASS_QIC: u8 = 0x03;
pub const USB_STORAGE_SUBCLASS_UFI: u8 = 0x04;
pub const USB_STORAGE_SUBCLASS_SFF: u8 = 0x05;
pub const USB_STORAGE_SUBCLASS_SCSI: u8 = 0x06;

pub const USB_STORAGE_PROTOCOL_CBI_INT: u8 = 0x00;
pub const USB_STORAGE_PROTOCOL_CBI_NO_INT: u8 = 0x01;
pub const USB_STORAGE_PROTOCOL_BBB: u8 = 0x50; // Bulk-Only Transport (BOT)
pub const USB_STORAGE_PROTOCOL_UAS: u8 = 0x62; // USB Attached SCSI

// ----------------------------------------------------------------------------
// Standard Descriptors (Packed Layouts)
// ----------------------------------------------------------------------------

/// USB Device Descriptor (18 bytes).
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default)]
pub struct DeviceDescriptor {
    pub length: u8,
    pub descriptor_type: u8,
    pub bcd_usb: u16,
    pub device_class: u8,
    pub device_subclass: u8,
    pub device_protocol: u8,
    pub max_packet_size0: u8,
    pub id_vendor: u16,
    pub id_product: u16,
    pub bcd_device: u16,
    pub manufacturer_str_idx: u8,
    pub product_str_idx: u8,
    pub serial_str_idx: u8,
    pub num_configurations: u8,
}

/// USB Configuration Descriptor (9 bytes).
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default)]
pub struct ConfigurationDescriptor {
    pub length: u8,
    pub descriptor_type: u8,
    pub total_length: u16,
    pub num_interfaces: u8,
    pub configuration_value: u8,
    pub configuration_str_idx: u8,
    pub attributes: u8,
    pub max_power: u8,
}

/// USB Interface Descriptor (9 bytes).
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default)]
pub struct InterfaceDescriptor {
    pub length: u8,
    pub descriptor_type: u8,
    pub interface_number: u8,
    pub alternate_setting: u8,
    pub num_endpoints: u8,
    pub interface_class: u8,
    pub interface_subclass: u8,
    pub interface_protocol: u8,
    pub interface_str_idx: u8,
}

/// USB Endpoint Descriptor (7 bytes).
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default)]
pub struct EndpointDescriptor {
    pub length: u8,
    pub descriptor_type: u8,
    pub endpoint_address: u8,
    pub attributes: u8,
    pub max_packet_size: u16,
    pub interval: u8,
}

// ----------------------------------------------------------------------------
// USB Device Abstraction
// ----------------------------------------------------------------------------

/// Represents an enumerated USB device.
#[derive(Debug, Clone)]
pub struct UsbDevice {
    pub slot_id: u8,
    pub port_id: u8,
    pub speed: UsbSpeed,
    pub vendor_id: u16,
    pub product_id: u16,
    pub interfaces: Vec<InterfaceDescriptor>,
    pub endpoints: Vec<EndpointDescriptor>,
}

/// USB Port speed indicator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsbSpeed {
    FullSpeed,      // 12 Mbps (USB 1.1 / 2.0)
    LowSpeed,       // 1.5 Mbps (USB 1.0)
    HighSpeed,      // 480 Mbps (USB 2.0)
    SuperSpeed,     // 5 Gbps (USB 3.0)
    SuperSpeedPlus, // 10 Gbps (USB 3.1)
    Unknown,
}
