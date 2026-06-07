.global switch_context
.global switch_context_to

# switch_context(prev_rsp_ptr: *mut u64, next_rsp: u64)
# Saves current context, writes stack pointer to [rdi],
# loads next stack pointer from rsi, restores context, and returns.
switch_context:
  push rbp
  push rbx
  push r12
  push r13
  push r14
  push r15
  mov [rdi], rsp
  mov rsp, rsi
  pop r15
  pop r14
  pop r13
  pop r12
  pop rbx
  pop rbp
  ret

# switch_context_to(next_rsp: u64) -> !
# Loads next stack pointer from rdi, restores registers, and returns (jumps to rip).
switch_context_to:
  mov rsp, rdi
  pop r15
  pop r14
  pop r13
  pop r12
  pop rbx
  pop rbp
  ret

.global thread_bootstrapper
.extern thread_exit
thread_bootstrapper:
  mov rdi, r13
  call r12
  call thread_exit

.global syscall_asm_entry
.extern syscall_rust_handler
syscall_asm_entry:
  # 1. Save all registers (SyscallFrame structure)
  push rax
  push rbp
  push rdi
  push rsi
  push rdx
  push rcx
  push r8
  push r9
  push r10
  push r11

  # 2. Pass pointer to the SyscallFrame as the first argument (rdi)
  mov rdi, rsp

  # Call Rust handler
  call syscall_rust_handler

  # 3. Restore all registers
  pop r11
  pop r10
  pop r9
  pop r8
  pop rcx
  pop rdx
  pop rsi
  pop rdi
  pop rbp
  pop rax

  iretq

