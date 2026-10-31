// SPDX-License-Identifier: GPL-2.0

//! Virtual Dynamic Shared Object (vDSO) subsystem for x86_64.
//!
//! Provides the userspace `linux-vdso.so.1` ELF shared library (sourced from
//! `asterinas/linux_vdso`) and the shared `vvar` timekeeping page to eliminate
//! kernel mode context switch overhead for time queries.

pub mod data;

use alloc::sync::Arc;
use core::sync::atomic::{AtomicU32, Ordering};
use ostd::mm::{PAGE_SIZE, UFrame, VmReader, io::VmIo, io::util::HasVmReaderWriter};
use ostd::sync::SpinLock;
use zerocopy::IntoBytes;

pub use self::data::{
    CLOCK_BOOTTIME, CLOCK_MONOTONIC, CLOCK_MONOTONIC_COARSE, CLOCK_MONOTONIC_RAW, CLOCK_REALTIME,
    CLOCK_REALTIME_COARSE, CLOCK_TAI, VCLOCK_NONE, VCLOCK_TSC, VDSO_BASES, ArchVdsoData, VdsoData,
    VdsoInstant,
};
use crate::{
    uapi::errno::Result,
    vm::backing::{Backing, alloc_zeroed_frame},
};

/// Embedded prebuilt ELF shared library image (`linux-vdso.so.1`) from asterinas/linux_vdso.
pub static VDSO_IMAGE: &[u8] = include_bytes!("vdso_x86_64.so");

/// Layout constants for x86_64 vDSO and vvar mapping (matching Linux v6.2.10).
pub const VDSO_DATA_SEGMENT_OFFSET: usize = 0;
pub const VDSO_DATA_SEGMENT_SIZE: usize = PAGE_SIZE;
pub const VDSO_TEXT_SEGMENT_OFFSET: usize = 4 * PAGE_SIZE;
pub const VDSO_TEXT_SEGMENT_SIZE: usize = PAGE_SIZE;
pub const VDSO_DATA_OFFSET: usize = 0x80;
pub const VDSO_TOTAL_SIZE: usize = 5 * PAGE_SIZE;

/// Backing implementation for a single static physical memory frame.
#[derive(Debug)]
pub struct SingleFrameBacking(pub UFrame);

impl Backing for SingleFrameBacking {
    fn size(&self) -> usize {
        PAGE_SIZE
    }

    fn frame(&self, _offset: usize) -> Result<UFrame> {
        Ok(self.0.clone())
    }
}

/// Global singleton managing the physical frames for `vvar` and `vdso`.
struct VdsoState {
    vvar_frame: UFrame,
    vvar_backing: Arc<dyn Backing>,
    vdso_backing: Arc<dyn Backing>,
    data: VdsoData,
}

static VDSO_STATE: SpinLock<Option<VdsoState>> = SpinLock::new(None);
static VDSO_SEQ: AtomicU32 = AtomicU32::new(0);

/// Initializes the global vDSO frames and backings.
pub fn init() {
    let mut lock = VDSO_STATE.lock();
    if lock.is_some() {
        return;
    }

    let Ok(vvar_frame) = alloc_zeroed_frame() else {
        ostd::error!("vDSO: failed to allocate physical frame for vvar");
        return;
    };

    let Ok(vdso_frame) = alloc_zeroed_frame() else {
        ostd::error!("vDSO: failed to allocate physical frame for vdso");
        return;
    };

    // Copy the loadable ELF text into the vdso code frame (first page)
    let copy_len = VDSO_TEXT_SEGMENT_SIZE.min(VDSO_IMAGE.len());
    let mut reader = VmReader::from(&VDSO_IMAGE[..copy_len]);
    vdso_frame.writer().write(&mut reader);

    let vdata = VdsoData {
        seq: 0,
        clock_mode: VCLOCK_NONE,
        cycle_last: 0,
        mask: !0u64,
        mult: 0,
        shift: 0,
        basetime: Default::default(),
        tz_minuteswest: 0,
        tz_dsttime: 0,
        hrtimer_res: 1,
        __unused: 0,
        arch_data: ArchVdsoData {},
    };

    let mut vdata_reader = VmReader::from(vdata.as_bytes()).to_fallible();
    let _ = vvar_frame.write(VDSO_DATA_OFFSET, &mut vdata_reader);

    let vvar_backing: Arc<dyn Backing> = Arc::new(SingleFrameBacking(vvar_frame.clone()));
    let vdso_backing: Arc<dyn Backing> = Arc::new(SingleFrameBacking(vdso_frame));

    *lock = Some(VdsoState {
        vvar_frame,
        vvar_backing,
        vdso_backing,
        data: vdata,
    });

    ostd::info!(
        "vDSO: Initialized x86_64 Linux vDSO from asterinas/linux_vdso (image size {} bytes)",
        VDSO_IMAGE.len()
    );
}

/// Returns the shared memory backings for `vvar` and `vdso`.
pub fn get_backings() -> Result<(Arc<dyn Backing>, Arc<dyn Backing>)> {
    let lock = VDSO_STATE.lock();
    if let Some(state) = lock.as_ref() {
        Ok((state.vvar_backing.clone(), state.vdso_backing.clone()))
    } else {
        drop(lock);
        init();
        let lock = VDSO_STATE.lock();
        let state = lock.as_ref().ok_or(crate::uapi::errno::Errno::ENOMEM)?;
        Ok((state.vvar_backing.clone(), state.vdso_backing.clone()))
    }
}

/// Updates the vDSO timekeeping parameters locklessly using the sequence counter.
pub fn update_time(
    clock_mode: i32,
    cycle_last: u64,
    mult: u32,
    shift: u32,
    realtime_sec: u64,
    realtime_nsec: u64,
    monotonic_sec: u64,
    monotonic_nsec: u64,
) {
    let mut lock = VDSO_STATE.lock();
    let Some(state) = lock.as_mut() else {
        return;
    };

    // Increment seq to odd (writer active)
    let seq = VDSO_SEQ.fetch_add(1, Ordering::Acquire) + 1;
    state.data.seq = seq;

    state.data.clock_mode = clock_mode;
    state.data.cycle_last = cycle_last;
    state.data.mult = mult;
    state.data.shift = shift;
    state.data.mask = !0u64;
    state.data.hrtimer_res = 1;

    let real_shifted = realtime_nsec << (shift as u64);
    let mono_shifted = monotonic_nsec << (shift as u64);

    // High-resolution clocks: store shifted nanoseconds
    state.data.basetime[CLOCK_REALTIME] = VdsoInstant {
        sec: realtime_sec,
        nsec_info: real_shifted,
    };
    state.data.basetime[CLOCK_MONOTONIC] = VdsoInstant {
        sec: monotonic_sec,
        nsec_info: mono_shifted,
    };
    state.data.basetime[CLOCK_MONOTONIC_RAW] = state.data.basetime[CLOCK_MONOTONIC];
    state.data.basetime[CLOCK_BOOTTIME] = state.data.basetime[CLOCK_MONOTONIC];
    state.data.basetime[CLOCK_TAI] = state.data.basetime[CLOCK_REALTIME];

    // Coarse clocks: store unshifted nanoseconds
    state.data.basetime[CLOCK_REALTIME_COARSE] = VdsoInstant {
        sec: realtime_sec,
        nsec_info: realtime_nsec,
    };
    state.data.basetime[CLOCK_MONOTONIC_COARSE] = VdsoInstant {
        sec: monotonic_sec,
        nsec_info: monotonic_nsec,
    };

    // Write updated state to the vvar page at VDSO_DATA_OFFSET
    let mut reader = VmReader::from(state.data.as_bytes()).to_fallible();
    let _ = state.vvar_frame.write(VDSO_DATA_OFFSET, &mut reader);

    // Increment seq to even (writer finished)
    let end_seq = VDSO_SEQ.fetch_add(1, Ordering::Release) + 1;
    state.data.seq = end_seq;

    let end_seq_bytes = end_seq.to_ne_bytes();
    let mut seq_reader = VmReader::from(&end_seq_bytes[..]).to_fallible();
    let _ = state.vvar_frame.write(VDSO_DATA_OFFSET, &mut seq_reader);
}
