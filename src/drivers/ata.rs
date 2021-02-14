use core::panic;

use x86_64::instructions::port::Port;

const BSY : u16 = 0x80;
const DRQ : u16 = 0x08;
const ERR : u16 = 0x01;
const DRIVER_FAULT : u16 = 0x20;

struct ATA {
    base_port : u16,
    data_register : Port<u16>,
    error_register : Port<u16>,
    sector_counts : Port<u16>,
    lba_low : Port<u16>,
    lba_mid : Port<u16>,
    lba_high : Port<u16>,
    drive_register : Port<u16>,
    status_register : Port<u16>,
    command_register : Port<u16>,
    control_register : Port<u16>,
    alternate_status_register : Port<u16>,
}

impl ATA {
    pub fn new(base_port : u16) -> ATA {
        ATA {
           base_port : base_port,
           data_register : Port::new(base_port),
           error_register : Port::new(base_port + 1),
           sector_counts : Port::new(base_port + 2),
           lba_low : Port::new(base_port + 3),
           lba_mid : Port::new(base_port + 4),
           lba_high : Port::new(base_port + 5),
           drive_register : Port::new(base_port + 6),
           status_register : Port::new(base_port + 7),
           command_register : Port::new(base_port + 7),
           control_register : Port::new(base_port + 0xC),
           alternate_status_register : Port::new(base_port + 0xC),
        }
    }

    pub fn delay_400ns(&mut self) {
        unsafe {   
            self.alternate_status_register.read();
            self.alternate_status_register.read();
            self.alternate_status_register.read();
            self.alternate_status_register.read();
        }
    }

    pub fn poll(&mut self) {
        self.delay_400ns();
        loop {
            let status : u16 = unsafe { self.command_register.read() };
            if ((status & BSY) != 0x80) && ((status & DRQ) == 0x01) {
                if (status & DRIVER_FAULT) != 0 {
                    panic!("ATA FAULT (BIT FAULT SET) !!!")
                }
                if (status & ERR) != 0x0 {
                    panic!("ATA ERROR (BIT ERR SET) !!!");
                }
                break;
            }
        }
    }

    pub fn identify(&mut self) {
        let device_port : u16;
        if self.base_port == 0x1F0 {
            device_port = 0xA0;
        } else {
            device_port = 0xB0;
        }
        unsafe  {
            self.drive_register.write(device_port);
            self.sector_counts.write(0);
            self.lba_low.write(0);
            self.lba_mid.write(0);
            self.lba_high.write(0);
            self.command_register.write(0xEC);
        }
        self.poll();
    }   

    pub fn read_sector(&mut self, sector : u8) {
        
    }
}