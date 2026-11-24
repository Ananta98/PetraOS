# PetraOS Kernel Security Audit & Remediation History

This document tracks security-relevant bugs identified in the PetraOS kernel (`petra/src/`), their root causes, potential escalation paths, and the applied fixes.

---

## Audit Pass — 2026-11-24

**Scope:** Single-pass static security audit of `petra/src/` covering memory safety (`#![forbid(unsafe_code)]`), privilege boundaries, credential/capability transitions, signal authorization, architecture-specific registers, and user-controlled kernel allocations.

**Summary:**
- `petra` enforces `#![forbid(unsafe_code)]` crate-wide (`petra/src/lib.rs:26`), preventing direct raw-pointer memory corruption inside kernel Rust code.
- **7 verifiable security defects** were identified and fixed:
  - **1 Critical** (Local Root Privilege Escalation via unprivileged `mount`/`umount2`)
  - **6 High** (Unauthorized cross-user signaling, capability privilege-regain & retained root capabilities after UID drop, Ring-0 `#GP` kernel panic via `ARCH_SET_FS`, and 3 unprivileged kernel OOM DoS vectors in `mincore`, `keyctl`, and POSIX `mqueue`)

| ID | Severity | Category | Subsystem / File | Status |
| :--- | :--- | :--- | :--- | :--- |
| SEC-001 | Critical | Privilege Escalation | `petra/src/syscall/fs/mount.rs` | Fixed |
| SEC-002 | High | Privilege Escalation / Unauthorized Signaling | `petra/src/syscall/signals/kill.rs` | Fixed |
| SEC-003 | High | Privilege Escalation / Broken Capability Drop | `petra/src/syscall/proc/cap.rs`, `petra/src/syscall/proc/cred.rs` | Fixed |
| SEC-004 | High | Kernel Panic / DoS (`#GP` in Ring 0) | `petra/src/syscall/proc/prctl.rs` | Fixed |
| SEC-005 | High | Kernel OOM Panic / DoS | `petra/src/syscall/mm/mincore.rs` | Fixed |
| SEC-006 | High | Kernel OOM Panic / DoS | `petra/src/syscall/proc/keyctl.rs` | Fixed |
| SEC-007 | High | Kernel OOM Panic / DoS | `petra/src/syscall/ipc/mqueue.rs` | Fixed |

---

### SEC-001 — Missing Superuser Check in `sys_mount` and `sys_umount2`
- **Severity:** Critical
- **Location:** `petra/src/syscall/fs/mount.rs` (`sys_mount`, `sys_umount2`)
- **Root Cause:** Neither `sys_mount` nor `sys_umount2` checked `proc.cred().is_superuser()` before performing mount/unmount operations (unlike the newer `fsopen(2)` family in `mount_api.rs`).
- **Escalation Impact:** Any unprivileged process could mount arbitrary filesystems (`tmpfs`, `devfs`, `procfs`, or `MS_BIND` over `/etc`, `/bin`, `/root`) or unmount critical system filesystems, leading directly to local root privilege escalation and system compromise.
- **Remediation:** Added `if !proc.cred().is_superuser() { return Err(Errno::EPERM); }` at entry in both `sys_mount` and `sys_umount2`.

---

### SEC-002 — Missing Signal Permission Checks in `sys_kill`, `sys_tkill`, and `sys_tgkill`
- **Severity:** High
- **Location:** `petra/src/syscall/signals/kill.rs` (`sys_kill`, `sys_tkill`, `sys_tgkill`)
- **Root Cause:** None of the signal transmission syscalls (or `Proc::send_signal`) validated whether the calling process was authorized to signal the target process. Additionally, `sys_kill` passed `sender = 0` for `pid > 0`, and `sys_tgkill` did not reject non-positive `tgid`/`tid`.
- **Escalation Impact:** Any unprivileged user could terminate (`SIGKILL`), pause (`SIGSTOP`), or inject arbitrary signals into root daemons (UID 0) or other users' processes.
- **Remediation:**
  - Added `can_signal(caller, target, sig)` enforcing POSIX/Linux `kill(2)` rules: allowed if `caller.cred().is_superuser()`, or if caller's real/effective UID matches target's real/saved-set UID, or for `SIGCONT` within the same session.
  - Enforced `can_signal` across `sys_kill`, `sys_tkill`, and `sys_tgkill`, returning `Errno::EPERM` when unauthorized.
  - Fixed `sys_kill` to pass `caller.pid.as_u32()` as `sender` and validated `tgid > 0 && tid > 0` in `sys_tgkill`.

---

### SEC-003 — Capability Privilege Regain in `sys_capset` & Retained Root Capabilities on UID Drop
- **Severity:** High
- **Location:** `petra/src/syscall/proc/cap.rs` (`sys_capset`), `petra/src/syscall/proc/cred.rs` (`transition_single`, `transition_re`, `transition_res`)
- **Root Cause:**
  1. `sys_capset` bypassed the `(new_perm & !current_perm) != 0` check whenever `cr_uid == 0`, allowing a root process that had intentionally dropped capabilities (e.g., inside a sandbox) to regain full capabilities.
  2. `sys_capset` never validated `new_inh`, allowing unprivileged processes to set arbitrary bits in `cap_inheritable` without `CAP_SETPCAP` or holding them in `cap_permitted`.
  3. `sys_capset` checked `current_perm` and updated `cap_*` under two separate lock acquisitions (TOCTOU race).
  4. `cred.rs` UID transitions (`setuid`, `setreuid`, `setresuid`) never cleared `cap_effective`/`cap_permitted` when transitioning from UID 0 to a non-zero UID, leaving dropped-privilege processes with root capabilities (`CAP_SYS_ADMIN`, `CAP_SETPCAP`, `CAP_SYS_RESOURCE`) still active in `seccomp.rs`, `prctl.rs`, and `oom.rs`.
- **Escalation Impact:** Sandboxed processes or services that drop root via `setuid()`/`capset()` could retain or regain privileged kernel capabilities.
- **Remediation:**
  - In `sys_capset`: removed the `is_root` bypass on `cap_permitted`, enforced that `new_inh` cannot raise bits outside `current_inh | current_perm` without `CAP_SETPCAP`, and performed validation and assignment atomically under a single `caller_proc.inner.lock()`.
  - In `cred.rs`: added `sync_caps_on_uid_change` (emulating Linux `cap_emulate_setxuid`) to clear `cap_effective` and `cap_permitted` when all of `(ruid, euid, svuid)` become non-zero, and clear `cap_effective` when `euid` transitions from 0 to non-zero.

---

### SEC-004 — Ring-0 `#GP` Kernel Panic via Unvalidated `arch_prctl(ARCH_SET_FS)`
- **Severity:** High
- **Location:** `petra/src/syscall/proc/prctl.rs` (`sys_arch_prctl`)
- **Root Cause:** `ARCH_SET_FS` passed the raw user-supplied `addr` directly to `FsBase::new(addr).load()` without checking that it is a valid canonical userspace address (`addr < MAX_USERSPACE_VADDR`).
- **Escalation Impact:** On x86_64, loading a non-canonical address into `IA32_FS_BASE` raises a General Protection Fault (`#GP`) in Ring 0, causing an immediate unprivileged kernel panic (DoS).
- **Remediation:** Added `if addr >= ostd::mm::MAX_USERSPACE_VADDR { return Err(Errno::EPERM); }` before updating `fs_base`.

---

### SEC-005 — Unprivileged Kernel OOM Panic & Missing Bounds Checks in `sys_mincore`
- **Severity:** High
- **Location:** `petra/src/syscall/mm/mincore.rs` (`sys_mincore`)
- **Root Cause:** `sys_mincore` allocated `alloc::vec![0u8; num_pages]` directly from user-supplied `len` before verifying page alignment, address overflow, or `MAX_USERSPACE_VADDR`, with no upper bound on kernel buffer size.
- **Escalation Impact:** Any unprivileged process passing a large `len` (e.g., `1 << 44`) triggered a multi-gigabyte kernel heap allocation and immediate OOM panic.
- **Remediation:** Enforced page-alignment (`EINVAL`), checked `addr + page_len <= MAX_USERSPACE_VADDR` (`ENOMEM`), and processed residency queries in bounded 4096-page (4 KiB) chunks.

---

### SEC-006 — Unprivileged Kernel OOM Panic in `sys_keyctl(KEYCTL_UPDATE)`
- **Severity:** High
- **Location:** `petra/src/syscall/proc/keyctl.rs` (`sys_keyctl`)
- **Root Cause:** Unlike `sys_add_key`, the `KEYCTL_UPDATE` branch allocated `alloc::vec![0u8; arg4]` without enforcing `arg4 <= KEY_PAYLOAD_MAX` (32,767 bytes) or validating non-null `arg3` when `arg4 > 0`.
- **Escalation Impact:** Any unprivileged process calling `keyctl(KEYCTL_UPDATE, key, ptr, huge_len)` caused an immediate kernel OOM panic.
- **Remediation:** Added `arg4 > KEY_PAYLOAD_MAX` (`EINVAL`) and `arg4 > 0 && arg3 == 0` (`EFAULT`) guards prior to allocation.

---

### SEC-007 — Unprivileged Kernel OOM Panic in `sys_mq_timedsend` and `sys_mq_timedreceive`
- **Severity:** High
- **Location:** `petra/src/syscall/ipc/mqueue.rs` (`sys_mq_timedsend`, `sys_mq_timedreceive`)
- **Root Cause:** Both syscalls executed `alloc::vec![0u8; msg_len]` directly from the user-supplied `msg_len` before validating against the queue's `mq_msgsize` limit.
- **Escalation Impact:** Any unprivileged process with an open POSIX message queue descriptor could pass an arbitrarily large `msg_len` to either syscall and trigger an immediate kernel OOM panic.
- **Remediation:**
  - `sys_mq_timedsend`: reject `msg_len > mq_msgsize` with `Errno::EMSGSIZE` before allocating.
  - `sys_mq_timedreceive`: reject `msg_len < mq_msgsize` with `Errno::EMSGSIZE` and allocate a buffer of size `mq_msgsize` (bounded by `MQ_MAX_MSGSIZE`), rather than the unbounded user-supplied `msg_len`.

---

## Audit Pass 2 — 2026-11-24 (Linux Syscall Parity, VM, Process & Scheduler)

**Scope:** Single-pass audit of virtual memory (`vm/vmar/ops/`, `syscall/mm/`), process resource limits (`syscall/proc/prlimit.rs`), and scheduler syscalls (`syscall/sched/`), verified against `claude-bughunter` (`triage-validation`, `evidence-hygiene`, `bb-methodology`, `hunt-auth-bypass`, `hunt-race-condition`, `hunt-exceptional-conditions`, `hunt-business-logic`).

| ID | Severity | Category | Subsystem / File | Status |
| :--- | :--- | :--- | :--- | :--- |
| SEC-008 | High | TOCTOU Permission Bypass (Write to Read-Only File) | `petra/src/vm/vmar/ops/mprotect.rs` | Fixed |
| SEC-009 | High | Unauthorized Read of `O_WRONLY` File & ABI Mismatches | `petra/src/syscall/mm/mmap.rs` | Fixed |
| SEC-010 | High | Cross-Process Resource Limit Tampering | `petra/src/syscall/proc/prlimit.rs` | Fixed |
| SEC-011 | High | Unchecked `addr + len` Integer Overflow Panics | `petra/src/vm/vmar/ops/mmap.rs`, `petra/src/syscall/mm/munmap.rs` | Fixed |
| SEC-012 | High | Integer Overflow Panics & TOCTOU `total_vm` Underflow | `petra/src/vm/vmar/ops/mremap.rs` | Fixed |
| SEC-013 | Medium | TOCTOU Concurrent Mapping Overwrite on Heap Growth | `petra/src/syscall/mm/brk.rs` | Fixed |
| SEC-014 | High | Unbounded Kernel Loop / Overflow Panic (CPU Lockup DoS) | `petra/src/syscall/sched/affinity.rs`, `petra/src/syscall/sched/attr.rs` | Fixed |
| SEC-015 | Medium | Unprivileged RT Priority Escalation & Invalid `SCHED_DEADLINE` | `petra/src/syscall/sched/param.rs` | Fixed |

### Key Remediation Details
- **SEC-008 (`Vmar::mprotect`)**: Merged the read-lock permission check and write-lock `carve` into a single `self.inner.write()` critical section so concurrent `mmap(MAP_FIXED)` cannot swap in a read-only shared mapping between check and apply.
- **SEC-009 (`sys_mmap`)**: Rejected `fd < 0` when `MAP_ANONYMOUS` is not set (`EBADF`), enforced `OpenFlags::READ` for all file-backed mappings (`EACCES`) to match Linux and prevent reading `O_WRONLY` files via `mmap`, and replaced `offset.saturating_add(len)` with `checked_add` (`EOVERFLOW`).
- **SEC-010 (`sys_prlimit64`)**: Added `can_access_prlimit` enforcing Linux `check_prlimit_permission` (`is_superuser()` or matching real/effective/saved UIDs and GIDs) when targeting another PID (`EPERM`), and rejected `resource >= RLIM_NLIMITS` (16) with `EINVAL`.
- **SEC-011 (`Vmar::mmap_inner`, `sys_munmap`)**: Replaced unchecked `addr + len` with `checked_add` (`ENOMEM`/`EINVAL`), enforced page alignment, `len != 0`, page-rounding, and `MAX_USERSPACE_VADDR` bounds in `sys_munmap`.
- **SEC-012 (`Vmar::mremap`)**: Replaced unchecked `old_addr + old_size`, `new_addr + new_size`, and `old_addr + new_size` with `checked_add`, and moved `is_fully_mapped(&old_range)` inside `self.inner.write()` to prevent TOCTOU `total_vm` underflow.
- **SEC-013 (`sys_brk`)**: Replaced non-atomic `has_overlap` + `MmapFlags::FIXED` with atomic `MmapFlags::FIXED_NOREPLACE`.
- **SEC-014 (`sys_sched_setaffinity`, `sys_sched_getaffinity`, `sys_sched_getattr`)**: Removed unbounded `cpusetsize` loops in `affinity.rs` (matching Linux by capping reads/writes at `CPU_BYTES` and requiring 8-byte alignment in `getaffinity`), and added `MAX_ATTR_SIZE` (`E2BIG`) + `checked_add` (`EFAULT`) in `sys_sched_getattr`.
- **SEC-015 (`sys_sched_setparam`, `sys_sched_setscheduler`)**: Required superuser privilege to raise `SCHED_FIFO`/`SCHED_RR` priority in `sys_sched_setparam` (`EPERM`), and rejected `SCHED_DEADLINE` in `sys_sched_setscheduler` (`EINVAL`, Linux only permits `SCHED_DEADLINE` via `sched_setattr`).
