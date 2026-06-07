use crate::arch::{ArchImpl, CpuArch};
use crate::proc::process::PROCESS_MANAGER;
use crate::proc::thread::THREAD_MANAGER;
use crate::syscalls::SyscallFrame;
use crate::proc::elf::Elf;

pub fn sys_exec(frame: &mut SyscallFrame, elf_ptr: u64, elf_size: u64) -> Result<(), &'static str> {
    if elf_ptr == 0 || elf_size == 0 {
        return Err("Invalid arguments");
    }

    // SAFETY: Caller must guarantee memory matches length
    let elf_slice = unsafe { core::slice::from_raw_parts(elf_ptr as *const u8, elf_size as usize) };

    // Load the ELF executable
    let elf = Elf::new(elf_slice)?;
    let loaded = elf.load()?;
    let (addr_space, entry_point, stack_pointer) = (loaded.addr_space, loaded.entry_point, loaded.stack_pointer);

    let cpu_id = ArchImpl::cpu_id();
    let current_tid = THREAD_MANAGER.lock().current_thread_id(cpu_id)
        .ok_or("No current thread")?;
    let current_pid = {
        let tm = THREAD_MANAGER.lock();
        tm.threads.get(&current_tid).map(|t| t.process_id)
            .ok_or("No current process")?
    };

    // Set process address space
    {
        let mut pm = PROCESS_MANAGER.lock();
        let proc = pm.get_process_mut(current_pid)
            .ok_or("Current process not found")?;
        proc.addr_space = Some(addr_space);
    }

    // Activate the new address space
    {
        let pm = PROCESS_MANAGER.lock();
        let proc = pm.get_process(current_pid).unwrap();
        if let Some(ref space) = proc.addr_space {
            // SAFETY: Activating a validated loaded ELF address space is safe.
            unsafe {
                space.activate();
            }
        }
    }

    // Adjust the system call frame to return to user space entry point
    frame.setup_user_entry(entry_point.as_u64(), stack_pointer.as_u64());

    Ok(())
}
