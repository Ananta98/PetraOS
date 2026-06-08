#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

extern crate alloc;

pub mod arch;
pub mod drivers;
pub mod limine;
pub mod logger;
pub mod mm;
pub mod proc;
pub mod sched;
pub mod sync;
pub mod syscalls;
pub mod fs;

use arch::{ArchImpl, CpuArch};
use core::arch::asm;

#[unsafe(no_mangle)]
unsafe extern "C" fn kmain() -> ! {
    logger::init();
    mm::init();
    ArchImpl::init_hardware();

    drivers::framebuffer::init();

    log::info!("PetraOS Kernel Scaffolding Initialized.");
    
    // Initialize Virtual File System (VFS)
    fs::init();

    // 1. Demonstrate Everything is a File: Write to stdout (FD 1, bound to console)
    let msg = b"Hello from stdout (FD 1) bound to /dev/console!\n";
    if let Ok(bytes) = fs::write(1, msg) {
        log::info!("Wrote {} bytes to FD 1", bytes);
    } else {
        log::error!("Failed to write to stdout (FD 1)");
    }

    // 2. Demonstrate Everything is a File: open, write, close, open, read, close
    log::info!("Opening /hello.txt with O_CREAT...");
    match fs::open("/hello.txt", fs::O_CREAT | fs::O_RDWR) {
        Ok(fd) => {
            log::info!("Successfully opened /hello.txt with FD {}", fd);
            let content = b"PetraOS VFS File Content via Descriptor!";
            if let Ok(bytes) = fs::write(fd, content) {
                log::info!("Wrote {} bytes to FD {}", bytes, fd);
            }
            if let Err(e) = fs::close(fd) {
                log::error!("Failed to close FD {}: {:?}", fd, e);
            } else {
                log::info!("Successfully closed FD {}", fd);
            }
        }
        Err(e) => log::error!("Failed to open /hello.txt: {:?}", e),
    }

    log::info!("Opening /hello.txt for reading...");
    match fs::open("/hello.txt", fs::O_RDONLY) {
        Ok(fd) => {
            log::info!("Successfully opened /hello.txt with FD {}", fd);
            let mut read_buf = [0u8; 64];
            match fs::read(fd, &mut read_buf) {
                Ok(bytes_read) => {
                    if let Ok(read_str) = core::str::from_utf8(&read_buf[..bytes_read]) {
                        log::info!("File /hello.txt read back successfully via FD {}: {}", fd, read_str);
                    }
                }
                Err(e) => log::error!("Failed to read from FD {}: {:?}", fd, e),
            }
            let _ = fs::close(fd);
        }
        Err(e) => log::error!("Failed to open /hello.txt for reading: {:?}", e),
    }

    log::warn!("Testing warning log levels!");
    log::error!("Testing error log levels!");

    hcf();
}

#[panic_handler]
fn rust_panic(info: &core::panic::PanicInfo) -> ! {
    log::error!("{}", info);
    hcf();
}

fn hcf() -> ! {
    loop {
       <ArchImpl as CpuArch>::halt();
    }
}
