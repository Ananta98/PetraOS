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


