.global syscall_entry
.extern do_syscall

# System call entry handler invoked by the `syscall` instruction
syscall_entry:
  # 1. Switch to per-CPU kernel stack
  swapgs
  mov gs:[8], rsp          # save user rsp into CpuLocal.user_rsp_scratch
  mov rsp, gs:[0]          # load CpuLocal.kernel_rsp into rsp

  # 2. Construct the iretq stack frame: SS, RSP, RFLAGS, CS, RIP
  push 0x23                # user SS (USER_DS with RPL=3)
  push gs:[8]              # user RSP (from scratch)
  push r11                 # user RFLAGS (saved by CPU in r11)
  push 0x1B                # user CS (USER_CS with RPL=3)
  push rcx                 # user RIP (saved by CPU in rcx)

  # 3. Save all general purpose registers up to r15 (15 pushes = 120 bytes)
  # Plus 5 iretq pushes (40 bytes), total 160 bytes. Stack is 16-byte aligned.
  push rax
  push rbx
  push rcx
  push rdx
  push rsi
  push rdi
  push rbp
  push r8
  push r9
  push r10
  push r11
  push r12
  push r13
  push r14
  push r15

  # 4. Pass pointer to SyscallFrame in RDI
  mov rdi, rsp
  call do_syscall

  # 5. Restore registers
  pop r15
  pop r14
  pop r13
  pop r12
  pop r11
  pop r10
  pop r9
  pop r8
  pop rbp
  pop rdi
  pop rsi
  pop rdx
  pop rcx
  pop rbx
  pop rax

  # 6. Return to user mode via iretq
  swapgs                   # restore user GS
  iretq

