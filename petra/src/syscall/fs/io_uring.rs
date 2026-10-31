// SPDX-License-Identifier: GPL-2.0

//! System calls `io_uring_setup(2)`, `io_uring_enter(2)`, and `io_uring_register(2)`
//! (Syscalls #425, #426, and #427 on x86_64).

use alloc::sync::Arc;
use core::mem::size_of;
use ostd::{arch::cpu::context::UserContext, mm::io::util::HasVmReaderWriter};

use crate::{
    fs::{
        fd::FdFlags,
        io_uring::{
            IORING_ENTER_GETEVENTS, IORING_FEAT_FAST_POLL, IORING_FEAT_NODROP,
            IORING_FEAT_SINGLE_MMAP, IORING_FEAT_SUBMIT_STABLE, IORING_OP_CLOSE,
            IORING_OP_FSYNC, IORING_OP_NOP, IORING_OP_POLL_ADD, IORING_OP_READ,
            IORING_OP_READV, IORING_OP_WRITE, IORING_OP_WRITEV, IoCqringOffsets,
            IoSqringOffsets, IoUringCqe, IoUringParams, IoUringSqe, create_io_uring_file,
        },
    },
    proc::{Proc, thread::Thread},
    syscall::fs::utils::UserPtr,
    uapi::errno::{Errno, Result},
    vm::backing::Backing,
};

const MAX_IOURING_ENTRIES: u32 = 4096;
const IORING_SQ_HEAD_OFFSET: usize = 0;
const IORING_SQ_TAIL_OFFSET: usize = 4;
const IORING_SQ_ARRAY_OFFSET: usize = 64;
const IORING_SQES_OFFSET: usize = 1024;

fn serialize_params(params: &IoUringParams, out: &mut [u8; 120]) {
    out[0..4].copy_from_slice(&params.sq_entries.to_ne_bytes());
    out[4..8].copy_from_slice(&params.cq_entries.to_ne_bytes());
    out[8..12].copy_from_slice(&params.flags.to_ne_bytes());
    out[12..16].copy_from_slice(&params.sq_thread_cpu.to_ne_bytes());
    out[16..20].copy_from_slice(&params.sq_thread_idle.to_ne_bytes());
    out[20..24].copy_from_slice(&params.features.to_ne_bytes());
    out[24..28].copy_from_slice(&params.wq_fd.to_ne_bytes());
    out[28..40].copy_from_slice(&[0u8; 12]);
    // sq_off
    out[40..44].copy_from_slice(&params.sq_off.head.to_ne_bytes());
    out[44..48].copy_from_slice(&params.sq_off.tail.to_ne_bytes());
    out[48..52].copy_from_slice(&params.sq_off.ring_mask.to_ne_bytes());
    out[52..56].copy_from_slice(&params.sq_off.ring_entries.to_ne_bytes());
    out[56..60].copy_from_slice(&params.sq_off.flags.to_ne_bytes());
    out[60..64].copy_from_slice(&params.sq_off.dropped.to_ne_bytes());
    out[64..68].copy_from_slice(&params.sq_off.array.to_ne_bytes());
    out[68..72].copy_from_slice(&params.sq_off.resv1.to_ne_bytes());
    out[72..80].copy_from_slice(&params.sq_off.user_addr.to_ne_bytes());
    // cq_off
    out[80..84].copy_from_slice(&params.cq_off.head.to_ne_bytes());
    out[84..88].copy_from_slice(&params.cq_off.tail.to_ne_bytes());
    out[88..92].copy_from_slice(&params.cq_off.ring_mask.to_ne_bytes());
    out[92..96].copy_from_slice(&params.cq_off.ring_entries.to_ne_bytes());
    out[96..100].copy_from_slice(&params.cq_off.overflow.to_ne_bytes());
    out[100..104].copy_from_slice(&params.cq_off.cqes.to_ne_bytes());
    out[104..108].copy_from_slice(&params.cq_off.flags.to_ne_bytes());
    out[108..112].copy_from_slice(&params.cq_off.resv1.to_ne_bytes());
    out[112..120].copy_from_slice(&params.cq_off.user_addr.to_ne_bytes());
}

fn parse_sqe(raw: &[u8; 64]) -> IoUringSqe {
    IoUringSqe {
        opcode: raw[0],
        flags: raw[1],
        ioprio: u16::from_ne_bytes(raw[2..4].try_into().unwrap()),
        fd: i32::from_ne_bytes(raw[4..8].try_into().unwrap()),
        off: u64::from_ne_bytes(raw[8..16].try_into().unwrap()),
        addr: u64::from_ne_bytes(raw[16..24].try_into().unwrap()),
        len: u32::from_ne_bytes(raw[24..28].try_into().unwrap()),
        rw_flags: u32::from_ne_bytes(raw[28..32].try_into().unwrap()),
        user_data: u64::from_ne_bytes(raw[32..40].try_into().unwrap()),
        pad: [0u8; 24],
    }
}

/// System call `io_uring_setup(u32 entries, struct io_uring_params *params)` (Syscall #425).
pub fn sys_io_uring_setup(entries: u32, params_ptr: usize, _ctx: &mut UserContext) -> Result<usize> {
    if entries == 0 || entries > MAX_IOURING_ENTRIES {
        return Err(Errno::EINVAL);
    }
    if params_ptr == 0 {
        return Err(Errno::EFAULT);
    }

    let proc = Thread::proc_or_esrch()?;
    let vmar = proc.vmspace();

    let mut raw_params = [0u8; size_of::<IoUringParams>()];
    UserPtr::new(params_ptr).read_bytes(&vmar, &mut raw_params)?;

    let input_flags = u32::from_ne_bytes(raw_params[8..12].try_into().unwrap());
    let file = create_io_uring_file(entries, input_flags)?;

    let sq_entries = entries.next_power_of_two().max(1).min(MAX_IOURING_ENTRIES);
    let cq_entries = (sq_entries * 2).min(MAX_IOURING_ENTRIES * 2);

    let out_params = IoUringParams {
        sq_entries,
        cq_entries,
        flags: input_flags,
        sq_thread_cpu: 0,
        sq_thread_idle: 0,
        features: IORING_FEAT_SINGLE_MMAP
            | IORING_FEAT_NODROP
            | IORING_FEAT_SUBMIT_STABLE
            | IORING_FEAT_FAST_POLL,
        wq_fd: 0,
        resv: [0; 3],
        sq_off: IoSqringOffsets {
            head: IORING_SQ_HEAD_OFFSET as u32,
            tail: IORING_SQ_TAIL_OFFSET as u32,
            ring_mask: sq_entries - 1,
            ring_entries: sq_entries,
            flags: 16,
            dropped: 20,
            array: IORING_SQ_ARRAY_OFFSET as u32,
            resv1: 0,
            user_addr: 0,
        },
        cq_off: IoCqringOffsets {
            head: 128,
            tail: 132,
            ring_mask: cq_entries - 1,
            ring_entries: cq_entries,
            overflow: 144,
            cqes: 192,
            flags: 148,
            resv1: 0,
            user_addr: 0,
        },
    };

    let mut out_bytes = [0u8; 120];
    serialize_params(&out_params, &mut out_bytes);
    UserPtr::new(params_ptr).write_bytes(&vmar, &out_bytes)?;

    let fd = proc.fd_table.lock().alloc_fd(file, FdFlags::FD_CLOEXEC)?;
    Ok(fd as usize)
}

/// Executes a single submission queue entry safely without kernel panics.
fn execute_single_sqe(
    sqe: &IoUringSqe,
    proc: &Arc<Proc>,
    vmar: &Arc<crate::vm::Vmar>,
) -> IoUringCqe {
    let mut cqe = IoUringCqe {
        user_data: sqe.user_data,
        res: 0,
        flags: 0,
    };

    match sqe.opcode {
        IORING_OP_NOP => {
            cqe.res = 0;
        }
        IORING_OP_READ | IORING_OP_READV => {
            let target_file = match proc.file(sqe.fd) {
                Ok(f) => f,
                Err(e) => {
                    cqe.res = -(e as i32);
                    return cqe;
                }
            };

            let len = sqe.len as usize;
            if len == 0 {
                cqe.res = 0;
                return cqe;
            }

            const CHUNK_SIZE: usize = 4096;
            let mut stack_buf = [0u8; CHUNK_SIZE];
            let mut total_read = 0;

            while total_read < len {
                let to_read = (len - total_read).min(CHUNK_SIZE);
                let current_off = sqe.off.saturating_add(total_read as u64);
                let res = target_file.ops.read(current_off, &mut stack_buf[..to_read]);
                match res {
                    Ok(0) => break,
                    Ok(n) => {
                        let dest_ptr = sqe.addr.saturating_add(total_read as u64) as usize;
                        if let Err(e) = UserPtr::new(dest_ptr).write_bytes(vmar, &stack_buf[..n]) {
                            cqe.res = -(e as i32);
                            return cqe;
                        }
                        total_read += n;
                        if n < to_read {
                            break;
                        }
                    }
                    Err(e) => {
                        if total_read > 0 {
                            break;
                        }
                        cqe.res = -(e as i32);
                        return cqe;
                    }
                }
            }
            cqe.res = total_read as i32;
        }
        IORING_OP_WRITE | IORING_OP_WRITEV => {
            let target_file = match proc.file(sqe.fd) {
                Ok(f) => f,
                Err(e) => {
                    cqe.res = -(e as i32);
                    return cqe;
                }
            };

            let len = sqe.len as usize;
            if len == 0 {
                cqe.res = 0;
                return cqe;
            }

            const CHUNK_SIZE: usize = 4096;
            let mut stack_buf = [0u8; CHUNK_SIZE];
            let mut total_written = 0;

            while total_written < len {
                let to_write = (len - total_written).min(CHUNK_SIZE);
                let src_ptr = sqe.addr.saturating_add(total_written as u64) as usize;
                if let Err(e) = UserPtr::new(src_ptr).read_bytes(vmar, &mut stack_buf[..to_write]) {
                    if total_written > 0 {
                        break;
                    }
                    cqe.res = -(e as i32);
                    return cqe;
                }

                let current_off = sqe.off.saturating_add(total_written as u64);
                match target_file.ops.write(current_off, &stack_buf[..to_write]) {
                    Ok(0) => break,
                    Ok(n) => {
                        total_written += n;
                        if n < to_write {
                            break;
                        }
                    }
                    Err(e) => {
                        if total_written > 0 {
                            break;
                        }
                        cqe.res = -(e as i32);
                        return cqe;
                    }
                }
            }
            cqe.res = total_written as i32;
        }
        IORING_OP_FSYNC => {
            match proc.file(sqe.fd) {
                Ok(target_file) => match target_file.ops.fsync(false) {
                    Ok(()) => cqe.res = 0,
                    Err(e) => cqe.res = -(e as i32),
                },
                Err(e) => cqe.res = -(e as i32),
            }
        }
        IORING_OP_POLL_ADD => {
            match proc.file(sqe.fd) {
                Ok(target_file) => {
                    let revents = target_file.poll().bits();
                    cqe.res = revents as i32;
                }
                Err(e) => cqe.res = -(e as i32),
            }
        }
        IORING_OP_CLOSE => {
            if sqe.fd < 0 {
                cqe.res = -(Errno::EBADF as i32);
            } else {
                let _ = proc.fd_table.lock().close(sqe.fd);
                cqe.res = 0;
            }
        }
        _ => {
            cqe.res = -(Errno::EOPNOTSUPP as i32);
        }
    }

    cqe
}

/// System call `io_uring_enter(unsigned int fd, u32 to_submit, u32 min_complete, u32 flags, sigset_t *sig, size_t sigsz)` (Syscall #426).
pub fn sys_io_uring_enter(
    fd: u32,
    to_submit: u32,
    min_complete: u32,
    flags: u32,
    _sig: usize,
    _sigsz: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;
    let file = proc.file(fd as i32)?;
    let ring = file.as_io_uring().ok_or(Errno::EBADF)?;
    let vmar = proc.vmspace();

    let mut submitted = 0u32;

    // Process batched submissions safely
    if to_submit > 0 {
        let max_batch = to_submit.min(ring.sq_entries);
        for _ in 0..max_batch {
            let sqe_offset = IORING_SQES_OFFSET + (submitted as usize) * 64;
            if sqe_offset + 64 > ring.shared_pages.size() {
                break;
            }

            let mut sqe_raw = [0u8; 64];
            if let Ok(frame) = ring.shared_pages.frame(sqe_offset) {
                let frame_off = sqe_offset % ostd::mm::PAGE_SIZE;
                let mut reader = frame.reader();
                reader.skip(frame_off);
                let _ = reader.read(&mut ostd::mm::VmWriter::from(&mut sqe_raw[..]));
            }

            let sqe = parse_sqe(&sqe_raw);
            let cqe = execute_single_sqe(&sqe, &proc, &vmar);
            ring.push_cqe(cqe);
            submitted += 1;
        }
    }

    // Wait for requested completion events if IORING_ENTER_GETEVENTS is set
    if (flags & IORING_ENTER_GETEVENTS) != 0 && min_complete > 0 {
        let (waiter, _) = ostd::sync::Waiter::new_pair();
        let target_completions = min_complete as usize;

        let cond = || {
            let count = ring.cqes.lock().len();
            if count >= target_completions {
                Some(count)
            } else {
                ring.wait_queue.enqueue(waiter.waker());
                None
            }
        };

        let cancel = || {
            if Thread::should_interrupt() {
                Err(Errno::EINTR)
            } else {
                Ok(())
            }
        };

        let _ = waiter.wait_until_or_cancelled(cond, cancel)?;
    }

    Ok(submitted as usize)
}

/// System call `io_uring_register(unsigned int fd, unsigned int opcode, void *arg, unsigned int nr_args)` (Syscall #427).
pub fn sys_io_uring_register(
    fd: u32,
    opcode: u32,
    arg_ptr: usize,
    _nr_args: u32,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;
    let file = proc.file(fd as i32)?;
    let _ring = file.as_io_uring().ok_or(Errno::EBADF)?;

    match opcode {
        18 => {
            // IORING_REGISTER_PROBE: query supported operations
            if arg_ptr == 0 {
                return Err(Errno::EFAULT);
            }
            Ok(0)
        }
        _ => Ok(0),
    }
}
