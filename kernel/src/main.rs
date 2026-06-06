use arch::ArchImpl;
use core::arch::asm;

#[unsafe(no_mangle)]
unsafe extern "C" fn kmain() -> ! {
    ArchImpl::init_hardware();
    logger::init();
    mm::init();
    drivers::framebuffer::init();

    log::info!("PetraOS Kernel Scaffolding Initialized.");
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
        unsafe {
            #[cfg(target_arch = "x86_64")]
            asm!("hlt");
            #[cfg(any(target_arch = "aarch64", target_arch = "riscv64"))]
            asm!("wfi");
            #[cfg(target_arch = "loongarch64")]
            asm!("idle 0");
        }
    }
}
