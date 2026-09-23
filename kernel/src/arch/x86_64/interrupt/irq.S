/* irq.S — x86_64 interrupt/exception entry trampolines
 * Intel syntax for LLVM integrated assembler in global_asm!
 *
 * Calling convention on entry to Rust handlers (System V AMD64):
 *   exception_common / exception_err_common:
 *     rdi = &InterruptStackFrame
 *     rsi = error_code  (0 for no-error-code exceptions)
 *     rdx = vector number
 *
 *   irq_common:
 *     rdi = &InterruptStackFrame
 *     rsi = vector number
 */

.code64
.section .text

/* ── External Rust dispatcher symbols ──────────────────────────────────────── */
.extern handle_exception
.extern handle_irq

/* ════════════════════════════════════════════════════════════════════════════
 * Common exception path (no CPU error code)
 * On entry the stack holds: [rip | cs | rflags | rsp | ss]
 * ════════════════════════════════════════════════════════════════════════════ */
exception_common:
    /* swapgs if coming from user space (CS RPL bits 0-1 at [rsp+8]) */
    test byte ptr [rsp + 8], 3
    jz 1f
    swapgs
1:
    push rax
    push rbx
    push rcx
    push rdx
    push rbp
    push rdi
    push rsi
    push r8
    push r9
    push r10
    push r11
    push r12
    push r13
    push r14
    push r15
    cld

    /* rdi = &InterruptStackFrame  (15 pushes * 8 = 120 bytes above rsp) */
    lea rdi, [rsp + 120]
    /* rsi = error_code — callers stored it in r10 before jumping here    */
    mov rsi, r10
    /* rdx = vector   — callers stored it in r11 before jumping here      */
    mov rdx, r11

    sub rsp, 8            /* align stack to 16 bytes for ABI       */
    call handle_exception
    add rsp, 8

    pop r15
    pop r14
    pop r13
    pop r12
    pop r11
    pop r10
    pop r9
    pop r8
    pop rsi
    pop rdi
    pop rbp
    pop rdx
    pop rcx
    pop rbx
    pop rax

    /* restore user GS if returning to user space */
    test byte ptr [rsp + 8], 3
    jz 2f
    swapgs
2:
    iretq

/* ════════════════════════════════════════════════════════════════════════════
 * Common exception path WITH a CPU-pushed error code
 * On entry the stack holds: [error_code | rip | cs | rflags | rsp | ss]
 * ════════════════════════════════════════════════════════════════════════════ */
exception_err_common:
    /* swapgs if coming from user space (CS is at [rsp+16] with error code) */
    test byte ptr [rsp + 16], 3
    jz 1f
    swapgs
1:
    push rax
    push rbx
    push rcx
    push rdx
    push rbp
    push rdi
    push rsi
    push r8
    push r9
    push r10
    push r11
    push r12
    push r13
    push r14
    push r15
    cld

    /* rdi = &InterruptStackFrame  (15 pushes * 8 + 8 error_code = 128)  */
    lea rdi, [rsp + 128]
    /* rsi = error_code from stack (sits just below the 15 saved regs)   */
    mov rsi, [rsp + 120]
    /* rdx = vector — stored in r11 by caller                            */
    mov rdx, r11

    call handle_exception

    pop r15
    pop r14
    pop r13
    pop r12
    pop r11
    pop r10
    pop r9
    pop r8
    pop rsi
    pop rdi
    pop rbp
    pop rdx
    pop rcx
    pop rbx
    pop rax

    add rsp, 8            /* discard CPU-pushed error code          */

    test byte ptr [rsp + 8], 3
    jz 2f
    swapgs
2:
    iretq

/* ════════════════════════════════════════════════════════════════════════════
 * Common hardware IRQ path
 * On entry the stack holds: [rip | cs | rflags | rsp | ss]
 * ════════════════════════════════════════════════════════════════════════════ */
irq_common:
    test byte ptr [rsp + 8], 3
    jz 1f
    swapgs
1:
    push rax
    push rbx
    push rcx
    push rdx
    push rbp
    push rdi
    push rsi
    push r8
    push r9
    push r10
    push r11
    push r12
    push r13
    push r14
    push r15
    cld

    /* rdi = &InterruptStackFrame */
    lea rdi, [rsp + 120]
    /* rsi = vector — stored in r11 by caller                            */
    mov rsi, r11

    sub rsp, 8
    call handle_irq
    add rsp, 8

    pop r15
    pop r14
    pop r13
    pop r12
    pop r11
    pop r10
    pop r9
    pop r8
    pop rsi
    pop rdi
    pop rbp
    pop rdx
    pop rcx
    pop rbx
    pop rax

    test byte ptr [rsp + 8], 3
    jz 2f
    swapgs
2:
    iretq

/* ════════════════════════════════════════════════════════════════════════════
 * Stub generator macros
 *
 * EXCEPTION_STUB vec — no CPU error code (vectors 0-7, 9, 16, 18-20)
 * EXCEPTION_ERR_STUB vec — CPU pushes error code (vectors 8, 10-14, 17, 21)
 * IRQ_STUB vec — hardware IRQ
 * ════════════════════════════════════════════════════════════════════════════ */

.macro EXCEPTION_STUB vec
    .global stub_ex\vec
    .type   stub_ex\vec, @function
stub_ex\vec:
    mov r11, \vec               /* vector -> rdx in common path            */
    xor r10, r10                /* error_code = 0                          */
    jmp exception_common
    .size stub_ex\vec, . - stub_ex\vec
.endm

.macro EXCEPTION_ERR_STUB vec
    .global stub_ex\vec
    .type   stub_ex\vec, @function
stub_ex\vec:
    mov r11, \vec               /* vector -> rdx in common path            */
    jmp exception_err_common
    .size stub_ex\vec, . - stub_ex\vec
.endm

.macro IRQ_STUB vec
    .global stub_irq_\vec
    .type   stub_irq_\vec, @function
stub_irq_\vec:
    mov r11, \vec               /* vector -> rsi in common path            */
    jmp irq_common
    .size stub_irq_\vec, . - stub_irq_\vec
.endm

/* ── CPU Exception stubs ────────────────────────────────────────────────── */
EXCEPTION_STUB     0       /* #DE  Divide Error                           */
EXCEPTION_STUB     1       /* #DB  Debug                                  */
EXCEPTION_STUB     2       /* #NMI Non-Maskable Interrupt                 */
EXCEPTION_STUB     3       /* #BP  Breakpoint                             */
EXCEPTION_STUB     4       /* #OF  Overflow                               */
EXCEPTION_STUB     5       /* #BR  Bound Range Exceeded                   */
EXCEPTION_STUB     6       /* #UD  Invalid Opcode                         */
EXCEPTION_STUB     7       /* #NM  Device Not Available                   */
EXCEPTION_ERR_STUB 8       /* #DF  Double Fault                           */
EXCEPTION_STUB     9       /* Coprocessor Segment Overrun (legacy)        */
EXCEPTION_ERR_STUB 10      /* #TS  Invalid TSS                            */
EXCEPTION_ERR_STUB 11      /* #NP  Segment Not Present                    */
EXCEPTION_ERR_STUB 12      /* #SS  Stack Segment Fault                    */
EXCEPTION_ERR_STUB 13      /* #GP  General Protection                     */
EXCEPTION_ERR_STUB 14      /* #PF  Page Fault                             */
EXCEPTION_STUB     16      /* #MF  x87 FPU Error                          */
EXCEPTION_ERR_STUB 17      /* #AC  Alignment Check                        */
EXCEPTION_STUB     18      /* #MC  Machine Check                          */
EXCEPTION_STUB     19      /* #XM  SIMD FPU Exception                     */
EXCEPTION_STUB     20      /* #VE  Virtualization Exception               */
EXCEPTION_ERR_STUB 21      /* #CP  Control Protection Exception           */

/* ── Hardware IRQ stubs ─────────────────────────────────────────────────── */
IRQ_STUB 33         /* PS/2 Keyboard (ISA IRQ 1)                         */
IRQ_STUB 44         /* PS/2 Mouse (ISA IRQ 12)                           */
IRQ_STUB 48         /* LAPIC Timer                                        */
IRQ_STUB 255        /* Spurious APIC interrupt (0xFF)                     */

