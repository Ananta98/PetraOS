//! Intel 8254x (e1000) EEPROM and MAC Address Reader
//!
//! Provides routines to read the 6-byte Ethernet MAC address from EEPROM
//! or from the hardware Receive Address Filter registers (RAL/RAH).

use super::registers::*;

/// Read a 16-bit word from the e1000 EEPROM at `addr` using the EERD register.
pub unsafe fn read_eeprom_word(mmio_base: *mut u8, addr: u8) -> Option<u16> {
    // EERD layout:
    // Bit 0: START
    // Bits 2..15 (or 8..15 in older models): Address
    // Bit 4 (or bit 1 in 82540): DONE
    // Bits 16..31: DATA

    // Try standard 82540 format (Start bit 0, Address at bits 2..15)
    let cmd = EERD_START | ((addr as u32) << EERD_ADDR_SHIFT_NEW);
    let eer_ptr = unsafe { mmio_base.add(REG_EERD) as *mut u32 };
    unsafe { core::ptr::write_volatile(eer_ptr, cmd) };

    for _ in 0..10_000 {
        let val = unsafe { core::ptr::read_volatile(eer_ptr) };
        if (val & EERD_DONE_NEW) != 0 || (val & EERD_DONE) != 0 {
            return Some(((val >> EERD_DATA_SHIFT) & 0xFFFF) as u16);
        }
        core::hint::spin_loop();
    }

    // Try 82544 legacy format (Start bit 0, Address at bits 8..15)
    let cmd_legacy = EERD_START | ((addr as u32) << EERD_ADDR_SHIFT);
    unsafe { core::ptr::write_volatile(eer_ptr, cmd_legacy) };

    for _ in 0..10_000 {
        let val = unsafe { core::ptr::read_volatile(eer_ptr) };
        if (val & EERD_DONE) != 0 || (val & EERD_DONE_NEW) != 0 {
            return Some(((val >> EERD_DATA_SHIFT) & 0xFFFF) as u16);
        }
        core::hint::spin_loop();
    }

    None
}

/// Retrieve the 6-byte Ethernet MAC address from EEPROM (words 0..2) or fallback to RAL/RAH registers.
pub unsafe fn read_mac_address(mmio_base: *mut u8) -> [u8; 6] {
    let mut mac = [0u8; 6];

    let w0 = unsafe { read_eeprom_word(mmio_base, 0) };
    let w1 = unsafe { read_eeprom_word(mmio_base, 1) };
    let w2 = unsafe { read_eeprom_word(mmio_base, 2) };

    if let (Some(w0), Some(w1), Some(w2)) = (w0, w1, w2) {
        mac[0] = (w0 & 0xFF) as u8;
        mac[1] = ((w0 >> 8) & 0xFF) as u8;
        mac[2] = (w1 & 0xFF) as u8;
        mac[3] = ((w1 >> 8) & 0xFF) as u8;
        mac[4] = (w2 & 0xFF) as u8;
        mac[5] = ((w2 >> 8) & 0xFF) as u8;

        if mac != [0, 0, 0, 0, 0, 0] && mac != [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF] {
            return mac;
        }
    }

    // Fallback: Read directly from RAL and RAH registers (Receive Address Filter 0)
    let ral_ptr = unsafe { mmio_base.add(REG_RAL) as *const u32 };
    let rah_ptr = unsafe { mmio_base.add(REG_RAH) as *const u32 };

    let ral = unsafe { core::ptr::read_volatile(ral_ptr) };
    let rah = unsafe { core::ptr::read_volatile(rah_ptr) };

    mac[0] = (ral & 0xFF) as u8;
    mac[1] = ((ral >> 8) & 0xFF) as u8;
    mac[2] = ((ral >> 16) & 0xFF) as u8;
    mac[3] = ((ral >> 24) & 0xFF) as u8;
    mac[4] = (rah & 0xFF) as u8;
    mac[5] = ((rah >> 8) & 0xFF) as u8;

    mac
}
