// SPDX-License-Identifier: GPL-2.0

//! System call dispatch and definitions.
//!
//! # The syscall ABI
//!
//! This kernel follows the **Linux** syscall ABI on x86-64: the numbers in
//! [`arch`] are Linux's, arguments arrive in the Linux register order
//! (`rdi`, `rsi`, `rdx`, `r10`, `r8`, `r9`), and a handler that fails returns
//! the **negated** error number in `rax`.
//!
//! [`arch`]: arch

/// Set to `true` to log every syscall (and user page fault) to the serial
/// console. Very slow under QEMU, so it is off by default.
pub const TRACE_SYSCALLS: bool = false;

/// Returns whether `sys_no` is a standard capability probe from modern glibc or systemd.
///
/// Cleanly returning `ENOSYS` without emitting noisy error diagnostics prevents serial
/// console log flooding while userspace takes its graceful fallback path.
pub const fn is_benign_probe(sys_no: usize) -> bool {
    matches!(
        sys_no,
        167 // swapon
        | 168 // swapoff
        | 272 // unshare
        | 308 // setns
        | 317 // seccomp
        | 321 // bpf
        | 323 // userfaultfd
        | 334 // rseq
        | 424 // pidfd_send_signal
        | 425 // io_uring_setup
        | 426 // io_uring_enter
        | 427 // io_uring_register
        | 434 // pidfd_open
        | 437 // openat2
        | 438 // pidfd_getfd
    )
}

pub mod arch;
pub mod fs;
pub mod ipc;
pub mod metadata;
pub mod mm;
pub mod net;
pub mod platform;
pub mod proc;
pub mod sched;
pub mod signals;
pub mod sync;
pub mod time;

pub use metadata as sys_info;

pub use arch::dispatch;

/// Calls a syscall handler, casting the register array to the argument types the
/// syscall table declares for it.
///
/// A handler always takes the user context last, so that a handler needing the
/// saved registers can reach them.
#[macro_export]
macro_rules! invoke_syscall_handler {
    ($handler:path, $args:expr, $ctx:expr) => {
        $handler($ctx)
    };
    ($handler:path, $args:expr, $ctx:expr, $t1:ty) => {
        $handler($args[0] as $t1, $ctx)
    };
    ($handler:path, $args:expr, $ctx:expr, $t1:ty, $t2:ty) => {
        $handler($args[0] as $t1, $args[1] as $t2, $ctx)
    };
    ($handler:path, $args:expr, $ctx:expr, $t1:ty, $t2:ty, $t3:ty) => {
        $handler($args[0] as $t1, $args[1] as $t2, $args[2] as $t3, $ctx)
    };
    ($handler:path, $args:expr, $ctx:expr, $t1:ty, $t2:ty, $t3:ty, $t4:ty) => {
        $handler(
            $args[0] as $t1,
            $args[1] as $t2,
            $args[2] as $t3,
            $args[3] as $t4,
            $ctx,
        )
    };
    ($handler:path, $args:expr, $ctx:expr, $t1:ty, $t2:ty, $t3:ty, $t4:ty, $t5:ty) => {
        $handler(
            $args[0] as $t1,
            $args[1] as $t2,
            $args[2] as $t3,
            $args[3] as $t4,
            $args[4] as $t5,
            $ctx,
        )
    };
    ($handler:path, $args:expr, $ctx:expr, $t1:ty, $t2:ty, $t3:ty, $t4:ty, $t5:ty, $t6:ty) => {
        $handler(
            $args[0] as $t1,
            $args[1] as $t2,
            $args[2] as $t3,
            $args[3] as $t4,
            $args[4] as $t5,
            $args[5] as $t6,
            $ctx,
        )
    };
}

/// Defines the syscall table and the [`dispatch`] function that runs it.
///
/// One entry per syscall:
///
/// ```ignore
/// SYS_WRITE = 1 => fs::sys_write(i32, usize, usize);
/// ```
///
/// The names become `pub const`s so the number is never written twice, and the
/// argument types are the casts applied to the register array in the Linux order
/// (`rdi`, `rsi`, `rdx`, `r10`, `r8`, `r9`). The handler is invoked with the
/// context appended, via [`invoke_syscall_handler`].
///
/// [`dispatch`]: dispatch
#[macro_export]
macro_rules! impl_syscall_nums_and_dispatch_fn {
    (
        $(
            $name:ident = $num:expr => $(::)? $($handler:ident)::+ ( $($arg_ty:ty),* ) ;
        )*
    ) => {
        $(
            pub const $name: usize = $num;
        )*

        /// Reads the syscall number and arguments out of the user register file,
        /// runs the handler, and writes the result back.
        ///
        /// On success `rax` becomes the handler's value. On failure it becomes
        /// the negated error number, which is how Linux user space recognises an
        /// error: anything in `-4095..=-1` is `errno`, everything else is a
        /// successful return.
        ///
        /// `rflags` is deliberately left untouched. See the module documentation
        /// for why writing to it is expensive.
        ///
        /// `rt_sigreturn` is special-cased above, because it restores the register
        /// file itself. Every other syscall reports through `rax`.
        pub fn dispatch(ctx: &mut ostd::arch::cpu::context::UserContext) {
            let (sys_no, args) = {
                let regs = ctx.general_regs();
                (regs.rax, [regs.rdi, regs.rsi, regs.rdx, regs.r10, regs.r8, regs.r9])
            };

            // `rt_sigreturn` restores the whole register file from the frame the
            // handler left behind. Writing a result over it afterwards would throw
            // away everything the handler just returned to, so this one number is
            // dispatched and then left alone.
            if sys_no == SYS_RT_SIGRETURN {
                // A failure here leaves the process with a half-restored register
                // file, so it is reported through `rax` after all rather than
                // unwinding into a state that cannot be described.
                if let Err(err) = $crate::syscall::signals::sys_rt_sigreturn(ctx) {
                    ctx.general_regs_mut().rax = err.to_posix_raw() as isize as usize;
                }
                return;
            }

            let pid_val = $crate::proc::thread::Thread::current_proc().map(|p| p.pid.as_u32()).unwrap_or(0);
            if $crate::syscall::TRACE_SYSCALLS && sys_no != 228 && sys_no != 96 {
                ostd::info!("syscall: pid {} sys_{} args={:x?}", pid_val, sys_no, args);
            }

            let res = match sys_no {
                $(
                    $num => $crate::invoke_syscall_handler!($($handler)::+, args, ctx $(, $arg_ty)*),
                )*
                _ => {
                    if !$crate::syscall::is_benign_probe(sys_no) {
                        ostd::warn!("Unsupported syscall number: {sys_no}");
                    }
                    Err($crate::uapi::errno::Errno::ENOSYS)
                }
            };

            ctx.general_regs_mut().rax = match res {
                Ok(val) => {
                    if $crate::syscall::TRACE_SYSCALLS && sys_no != 228 && sys_no != 96 {
                        ostd::info!("syscall: pid {} sys_{} -> Ok({:#x})", pid_val, sys_no, val);
                    }
                    val
                }
                Err(err) => {
                    if err == $crate::uapi::errno::Errno::ENOSYS && !$crate::syscall::is_benign_probe(sys_no) {
                        ostd::warn!("Syscall sys_{sys_no} returned ENOSYS");
                    }
                    if $crate::syscall::TRACE_SYSCALLS && sys_no != 228 && sys_no != 96 {
                        ostd::info!("syscall: pid {} sys_{} -> Err({:?})", pid_val, sys_no, err);
                    }
                    err.to_posix_raw() as isize as usize
                }
            };
        }
    };
}
