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
  swapgs                   # restore user GS

  # 3. Save general purpose registers matching SyscallFrame layout
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

  # 4. Pass pointer to SyscallFrame in RDI
  mov rdi, rsp
  call do_syscall

  # 5. Restore registers
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

  # 6. Return to user mode via iretq
  iretq
