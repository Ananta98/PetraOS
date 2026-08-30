.global fork_child_return
fork_child_return:
  # Pop general purpose registers matching SyscallFrame layout
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
  # Return to Ring 3 user mode with restored user RIP, CS, RFLAGS, RSP, SS
  iretq