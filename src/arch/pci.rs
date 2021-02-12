use crate::println;
use x86_64::instructions::port::Port;

#[derive(Default)]
struct FunctionInfo {
    device_id : u16,
    vendor_id : u16,
    status : u8,
    command : u8,
    revision_id : u8,
    prog_if : u8,
    subclass : u8,
    class : u8,
    cache_line_size : u8,
    latency_timer : u8,
    header_type : u8,
    bist : u8,
}

impl FunctionInfo {
    pub fn new() -> Self {
        FunctionInfo::default()
    }

    pub fn read_in_device(&mut self) {

    }
}

pub fn pci_read_config(bus : u8, slot : u8, function : u8, offset : u8) -> u16 {
    let mut data_port : Port<u16> = Port::new(0xCFC);
    let mut command_port : Port<u16> = Port::new(0xCF8);

    let lbus = bus as u32;
    let lslot = slot as u32;
    let lfunction = function as u32;
    
    let address : u32 = (lbus << 16) | (lslot << 11) | (lfunction << 8) | ((offset & 0xfc) as u32) | (0x80000000 as u32);

    let  result = unsafe  {
        command_port.write(address as u16);
        ((data_port.read() >> ((offset & 2) * 8)) & 0xffff) as u16
    };

    result
}

pub fn pci_write_config(bus : u8, slot : u8, function : u8, offset : u8, data : u32) {
    let mut data_port : Port<u32> = Port::new(0xCFC);
    let mut command_port : Port<u32> = Port::new(0xCF8);

    let lbus = bus as u32;
    let lslot = slot as u32;
    let lfunction = function as u32;
    
    let address : u32 = (lbus << 16) | (lslot << 11) | (lfunction << 8) | ((offset & 0xfc) as u32) | (0x80000000 as u32);

    unsafe  {
        data_port.write(address);
        command_port.write(data);
    }
}

pub fn pci_check_vendor(bus : u8, slot : u8) {
    let vendor = pci_read_config(bus, slot, 0, 0);
    if vendor != 0xFFFF {
        let device = pci_read_config(bus, slot, 0, 2);
        println!("The bus {} has slot {} with device {:x}",bus,slot,device);
    }
}