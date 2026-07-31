#![no_std]
#![deny(unsafe_code)]

extern crate alloc;

#[cfg_attr(target_arch = "x86_64", path = "arch/x86_64/mod.rs")]
mod arch;

mod device;
mod drivers;
mod fs;
mod ipc;
mod irq;
mod modules;
mod net;
mod proc;
mod scheduler;
mod syscall;
mod vm;

#[ostd::main]
fn kernel_main() {
    arch::init();
    vm::init();
    irq::init();
    modules::init().expect("failed to initialize kernel modules");
    fs::init().expect("failed to initialize filesystem");
    net::init();

    // The scheduler must be injected *before* any task is spawned; otherwise
    // OSTD lazily installs its default FIFO scheduler when `spawn_init_process`
    // enqueues the init task, and the subsequent `inject_scheduler` call would
    // panic ("a scheduler has already been initialized").
    scheduler::init();

    // Spawn the init process (PID 1).  With a correctly-injected scheduler its
    // main thread immediately enters user mode and runs the init program.
    proc::spawn_init_process();

    loop {
        ostd::task::halt_cpu();
    }
}
