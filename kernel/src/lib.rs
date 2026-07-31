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

use ostd::{early_println, task::scheduler::enable_preemption_on_cpu};
use proc::thread::KernelThread;

fn ap_entry() {
    enable_preemption_on_cpu();
    if KernelThread::spawn_idle(|| {}).is_err() {
        loop {
            ostd::task::halt_cpu();
        }
    }
}

#[ostd::main]
fn kernel_main() {
    arch::init();
    vm::init();
    irq::init();
    modules::init().expect("failed to initialize kernel modules");
    fs::init().expect("failed to initialize filesystem");
    net::init();
    scheduler::init();
    ostd::boot::smp::register_ap_entry(ap_entry);
    crate::proc::thread::KernelThread::spawn_idle(proc::spawn_init_process)
        .expect("failed to spawn BSP idle thread");

    loop {
        ostd::task::halt_cpu();
    }
}
