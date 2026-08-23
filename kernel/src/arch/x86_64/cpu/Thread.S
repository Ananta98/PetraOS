.global thread_bootstrapper
.extern thread_exit
thread_bootstrapper:
  mov rdi, r13
  call r12
  call thread_exit
