/// `execve(pathname, argv, envp)` — execute program (SYS_execve = 59).
///
/// Replaces the current process image with a new process image loaded from
/// an ELF executable file.
///
/// Returns `0` on success, or a negated `errno` on failure.
use crate::proc::pid_table::PROCESS_TABLE;
use crate::proc::process::Process;
use crate::proc::userspace::read_user_string;
use crate::syscall::SyscallResult;
use crate::vm::vma::VmaManager;
use alloc::string::String;
use alloc::vec::Vec;
use ostd::Error;
use ostd::arch::cpu::context::UserContext;
use ostd::user::UserContextApi;

pub const EPERM: isize = 1;
pub const ENOENT: isize = 2;
pub const EIO: isize = 5;
pub const ENOEXEC: isize = 8;
pub const EACCES: isize = 13;
pub const EFAULT: isize = 14;

pub fn syscall_execve(
    arg0: usize, // const char *pathname
    arg1: usize, // char *const argv[]
    arg2: usize, // char *const envp[]
    _: usize,
    _: usize,
    _: usize,
    vm: &VmaManager,
    context: &mut UserContext,
) -> SyscallResult {
    let pathname_ptr = arg0;
    let argv_ptr = arg1;
    let envp_ptr = arg2;

    if pathname_ptr == 0 {
        return SyscallResult::Return((-EFAULT) as usize);
    }

    // 1. Read pathname, argv, and envp from user space.
    let path = match read_user_string(vm, pathname_ptr) {
        Ok(p) => p,
        Err(_) => return SyscallResult::Return((-EFAULT) as usize),
    };
    let argv = match read_user_string_array(vm, argv_ptr) {
        Ok(a) => a,
        Err(_) => return SyscallResult::Return((-EFAULT) as usize),
    };
    let envp = match read_user_string_array(vm, envp_ptr) {
        Ok(e) => e,
        Err(_) => return SyscallResult::Return((-EFAULT) as usize),
    };

    // 2. Resolve the path and read the ELF executable image.
    let dentry = match crate::fs::vfs::resolve_path(&path) {
        Ok(d) => d,
        Err(_) => return SyscallResult::Return((-ENOENT) as usize),
    };
    let meta = match dentry.inode.metadata() {
        Ok(m) => m,
        Err(_) => return SyscallResult::Return((-EIO) as usize),
    };
    let mut file_ops = match dentry.inode.open(0) {
        Ok(f) => f,
        Err(_) => return SyscallResult::Return((-EACCES) as usize),
    };

    let mut elf_image = alloc::vec![0u8; meta.size];
    let mut offset = 0;
    if file_ops.read(&mut elf_image, &mut offset).is_err() {
        return SyscallResult::Return((-EIO) as usize);
    }

    // Check ELF magic header
    if elf_image.len() < 4 || &elf_image[0..4] != b"\x7fELF" {
        return SyscallResult::Return((-ENOEXEC) as usize);
    }

    // 3. Perform exec on the current process to replace its address space.
    let current_process = Process::current();
    let mut entry = 0;
    let mut stack_ptr = 0;
    let mut exec_err = false;

    let argv_refs: Vec<&str> = argv.iter().map(|s| s.as_str()).collect();
    let envp_refs: Vec<&str> = envp.iter().map(|s| s.as_str()).collect();

    PROCESS_TABLE.update_process(current_process.pid, |p| {
        match p.exec(&path, &elf_image, &argv_refs, &envp_refs) {
            Ok((loaded, sp)) => {
                entry = loaded.entry;
                stack_ptr = sp;
            }
            Err(_) => {
                exec_err = true;
            }
        }
    });

    if exec_err {
        return SyscallResult::Return((-ENOEXEC) as usize);
    }

    // 5. Modify the UserContext to jump to the new entry point on syscall return.
    context.set_instruction_pointer(entry);
    context.set_stack_pointer(stack_ptr);
    context.set_rax(0);

    // Sync registers for fast sysret
    let rip = context.rip();
    let rflags = context.rflags();
    context.set_rcx(rip);
    context.set_r11(rflags);

    SyscallResult::Return(0)
}

// Helper to read string arrays from user space.
fn read_user_string_array(vm: &VmaManager, user_array_ptr: usize) -> Result<Vec<String>, Error> {
    if user_array_ptr == 0 {
        return Ok(Vec::new());
    }
    let mut array = Vec::new();
    let mut offset = 0;
    loop {
        let mut ptr_buf = [0u8; 8];
        vm.copy_from_user(user_array_ptr + offset, &mut ptr_buf)?;
        let str_ptr = usize::from_ne_bytes(ptr_buf);
        if str_ptr == 0 {
            break;
        }
        let s = read_user_string(vm, str_ptr)?;
        array.push(s);
        offset += 8;
        if array.len() > 1024 {
            return Err(Error::InvalidArgs);
        }
    }
    Ok(array)
}
