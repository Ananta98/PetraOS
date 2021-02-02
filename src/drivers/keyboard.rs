use crate::print;
use spin::Mutex;
use lazy_static::lazy_static;
use x86_64::instructions::port::Port;
use pc_keyboard::{DecodedKey, HandleControl, Keyboard, ScancodeSet1, layouts};

lazy_static! {
    static ref KEYBOARD : Mutex<Keyboard<layouts::Us104Key, ScancodeSet1>> = Mutex::new(
        Keyboard::new(layouts::Us104Key,ScancodeSet1,HandleControl::Ignore)
    );
}

pub fn keyboard_pressed() {
    let mut keyboard = KEYBOARD.lock();
    let mut port = Port::new(0x60);
    let scancode : u8 = unsafe { port.read() };
    if let Ok(Some(key_event)) = keyboard.add_byte(scancode) {
        if let Some(key) = keyboard.process_keyevent(key_event) {
            match key {
                DecodedKey::Unicode(character) => print!("{}",character),
                DecodedKey::RawKey(key) => print!("{:?}",key),
            }
        }
    };
}