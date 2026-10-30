/* SPDX-License-Identifier: GPL-2.0 */
/*
 * Signal return trampoline with full DWARF CFI annotations.
 */

    .text
    .globl __vdso_rt_sigreturn
    .type __vdso_rt_sigreturn, @function
__vdso_rt_sigreturn:
    .cfi_startproc
    .cfi_signal_frame
    mov $15, %rax /* SYS_rt_sigreturn */
    syscall
    ud2
    .cfi_endproc
    .size __vdso_rt_sigreturn, . - __vdso_rt_sigreturn
