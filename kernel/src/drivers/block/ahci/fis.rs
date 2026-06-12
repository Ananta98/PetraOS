#[derive(Clone, Copy, Debug)]
#[repr(u8)]
pub enum FisType {
    RegH2D = 0x27,   // Register FIS - Host to Device
    RegD2H = 0x34,   // Register FIS - Device to Host
    DmaAct = 0x39,   // DMA Activate FIS - Device to Host
    DmaSetup = 0x41, // DMA Setup FIS - Bidirectional
    Data = 0x46,     // Data FIS - Bidirectional
    Bist = 0x58,     // BIST Activate FIS - Bidirectional
    PioSetup = 0x5F, // PIO Setup FIS - Device to Host
    DevBits = 0xA1,  // Set Device Bits FIS - Device to Host
}

#[repr(C, packed)]
pub struct FisRegH2D {
    pub fis_type: u8,
    pub pmport_c: u8, // Port multiplier & Command bit
    pub command: u8,
    pub feature_l: u8,
    pub lba0: u8,
    pub lba1: u8,
    pub lba2: u8,
    pub device: u8,
    pub lba3: u8,
    pub lba4: u8,
    pub lba5: u8,
    pub feature_h: u8,
    pub count_l: u8,
    pub count_h: u8,
    pub icc: u8,
    pub control: u8,
    pub rsv1: [u8; 4],
}
