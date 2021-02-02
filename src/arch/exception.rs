use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};

macro_rules! exception {
    ($x:ident, $stack:ident, $func:block) => {
        extern "x86-interrupt" fn $x($stack : &mut InterruptStackFrame) {
            $func
        }
    };
    
}
