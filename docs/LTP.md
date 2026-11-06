# Linux Test Project (LTP) Conformance & Stress Testing Guide

This document describes how to execute the Linux Test Project (LTP) test suites against PetraOS to validate Linux ABI compatibility, verify error semantics (`errno`), and stress test kernel subsystems (`fs`, `vm`, `proc`, `sched`, `ipc`).

---

## 1. Quick Start: Running Tests

The test suite contains **1,398 compiled, statically linked LTP test binaries** staged in `/opt/ltp/testcases/bin` and packed inside `build/ext4.img` (4 GB image).

### A. Run Any Individual Syscall Test
You can test any of the 1,398 compiled syscall binaries directly by name:

```bash
# File operations
bash scripts/linux-test-project/test-syscalls.sh --test openat01
bash scripts/linux-test-project/test-syscalls.sh --test close01
bash scripts/linux-test-project/test-syscalls.sh --test dup01
bash scripts/linux-test-project/test-syscalls.sh --test lseek01
bash scripts/linux-test-project/test-syscalls.sh --test read01
bash scripts/linux-test-project/test-syscalls.sh --test write01

# Process & Scheduling
bash scripts/linux-test-project/test-syscalls.sh --test getpid01
bash scripts/linux-test-project/test-syscalls.sh --test fork01
bash scripts/linux-test-project/test-scheduler.sh --test sched_yield01

# IPC & Memory
bash scripts/linux-test-project/test-syscalls.sh --test pipe01
bash scripts/linux-test-project/test-syscalls.sh --test mmap01
bash scripts/linux-test-project/test-syscalls.sh --test clock_gettime01
```

---

### B. Run the Full Upstream Syscall Scenario (`--all`)
To run through the full suite of system calls:

```bash
bash scripts/linux-test-project/test-syscalls.sh --all
```

---

### C. Run the Curated Core Syscall Suite
Runs the curated core set (`openat`, `close`, `dup`, `lseek`, `getpid`, `pipe`):

```bash
bash scripts/linux-test-project/test-syscalls.sh
```

---

### D. Run the Filesystem Torture Stress Test (`fsstress`)
`fsstress` executes concurrent, randomized filesystem operations (creates, writes, truncates, renames, unlinks, and directories) across worker processes. Verbose mode (`-v`) and target directory initialization are automated:

```bash
bash scripts/linux-test-project/test-syscalls.sh --test fsstress
```

---

### E. Run Scheduler & IPC Suites
```bash
# Scheduler and concurrency tests
bash scripts/linux-test-project/test-scheduler.sh

# IPC primitives (pipes and buffers)
bash scripts/linux-test-project/test-ipc.sh

# Run all suites sequentially
bash scripts/linux-test-project/run-all.sh
```

---

## 2. Inspecting Saved Logs (Git-Excluded)

Every run automatically streams output to stdout and saves a complete log to:
- Timestamped logs: `scripts/linux-test-project/logs/<suite>-<YYYYMMDD-HHMMSS>.log`
- Symlink to latest run: `scripts/linux-test-project/logs/<suite>-latest.log`

Inspect results with:
```bash
cat scripts/linux-test-project/logs/syscalls-latest.log
cat scripts/linux-test-project/logs/scheduler-latest.log
cat scripts/linux-test-project/logs/ipc-latest.log
```

---

## 3. Rebuilding & Repackaging LTP (`build-ltp.sh`)

If you ever need to rebuild all test binaries and repack `build/ext4.img`:

```bash
bash scripts/linux-test-project/build-ltp.sh --all
```
*(The scripts in `scripts/linux-test-project/` also automatically detect missing binaries or updated scripts and repack `build/ext4.img` on demand).*
