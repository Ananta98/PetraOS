use x86_64::instructions::port::Port;

const BSY : u8 = 0x80;
const DRQ : u8 = 0x08;
const ERR : u8 = 0x01;
const DRIVER_FAULT : u8 = 0x20;
const SECTOR_SIZE : usize = 512;

pub struct ATA {
    base_port : u16,
    data_register : Port<u16>,
    error_register : Port<u8>,
    sector_counts : Port<u8>,
    lba_low : Port<u8>,
    lba_mid : Port<u8>,
    lba_high : Port<u8>,
    drive_register : Port<u8>,
    command_register : Port<u8>,
    control_register : Port<u8>,
    alternate_status_register : Port<u8>,
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
            let status : u8 = unsafe { self.command_register.read() };
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
        let device_port : u8 ;
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

    fn write_lba(&mut self, lba : u32) {
        let device_port : u8;
        if self.base_port == 0x1F0 {
            device_port = 0xE0;
        } else {
            device_port = 0xF0;
        }

        unsafe {
            self.data_register.write(0);
            self.sector_counts.write(1);
            self.lba_low.write(lba as u8);
            self.lba_mid.write((lba >> 8) as u8);
            self.lba_high.write((lba >> 16) as u8);
            self.drive_register.write(device_port | ((lba >> 24) & 0x0F) as u8 )
        }

    }

    pub fn flush_ata(&mut self) {
        let device_port : u8;
        if self.base_port == 0x1F0 {
            device_port = 0xE0;
        } else {
            device_port = 0xF0;
        }
        unsafe {
            self.drive_register.write(device_port);
            self.command_register.write(0xE7);
        }
        self.poll();
    }

    pub fn read_sector(&mut self,buffer : &mut [u8], lba: u32, size : usize)  {
        self.write_lba(lba);
        unsafe {
            self.command_register.write(0x20);
        }
        self.poll();
        for index in lba as usize..size {
            let cmd = unsafe {
                self.data_register.read()
            };
            buffer[index] = cmd as u8;
            buffer[index + 1] = (cmd >> 8) as u8;
        }
        self.delay_400ns();
    }

    pub fn write_sector(&mut self, buffer : &[u8], lba : u32, size : usize) {
        self.write_lba(lba);
        unsafe {
            self.command_register.write(0x30);
        }
        self.poll();
        for index in lba as usize..size / 2 {
            let data_write = ((buffer[index * 2 + 1]) | buffer[index * 2]) as u16;
            unsafe {
                self.data_register.write(data_write);
            }
        } 
        self.delay_400ns();
    }

    pub fn read_all_sectors(&mut self, offset : usize, buffer : &mut [u8], size : usize) {
        let num_blocks = (size / SECTOR_SIZE) + 1;
        let lba_start = offset / SECTOR_SIZE;
        let mut rem_size = size;
        for index in 0..num_blocks {
            let read_size = core::cmp::min(rem_size, SECTOR_SIZE);
            self.read_sector(buffer, (lba_start + index) as u32, read_size);
            rem_size -= read_size;
        }
    } 
    
    pub fn write_all_sectors(&mut self, offset : usize, buffer : &[u8], size : usize) {
        let num_blocks = (size / SECTOR_SIZE) + 1;
        let lba_start = offset / SECTOR_SIZE;
        let mut rem_size = size;
        for index in 0..num_blocks {
            let write_size = core::cmp::min(rem_size, SECTOR_SIZE);
            self.write_sector(buffer, (lba_start + index) as u32, write_size);
            rem_size -= write_size;
        }
        self.flush_ata();
    } 
}