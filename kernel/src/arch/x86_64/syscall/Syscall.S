.global syscall_asm_entry
.extern do_syscall

syscall_asm_entry:
  # Save the syscall register state into the SyscallFrame layout.
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

  # Pass the frame pointer as the first Rust argument.
  mov rdi, rsp
  call do_syscall

  # Restore the register state before returning to user mode.
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
