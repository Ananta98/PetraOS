# Linux Test Project (LTP) Conformance & Stress Testing Guide

This document provides a step-by-step walkthrough for building, packaging, and running the **Linux Test Project (LTP)** test suites and stress tests against **PetraOS**.

---

## 1. Quick Start: Running the Stress Test

LTP is distributed as upstream C source code. To make execution seamless, [`scripts/linux-test-project/build-ltp.sh`](file:///home/ananta/PetraOS/scripts/linux-test-project/build-ltp.sh) automates downloading the official release tarball, configuring and compiling statically linked binaries, installing them into the rootfs staging area (`/opt/ltp`), and packaging the bootable `build/ext4.img` disk image.

### Step 1: Download, Build, and Package LTP into PetraOS Rootfs

Run the automated one-shot setup command:
```bash
bash scripts/linux-test-project/build-ltp.sh --all
```
This performs:
1. **Download**: Fetches the official `ltp-full-20240930.tar.xz` into `build/cache/`.
2. **Compile**: Configures LTP with static linking (`LDFLAGS="-static"`) and compiles the core library, the `ltp-pan` test engine, the `fsstress` filesystem stress tool, and syscall/scheduler/IPC test binaries.
3. **Install**: Strips debugging symbols and installs the binaries into `build/userland-amd64/rootfs_staging/opt/ltp/testcases/bin/`.
4. **Repack**: Repackages `build/ext4.img` with the updated rootfs.

---

### Step 2: Run Filesystem Stress Testing (`fsstress`)

`fsstress` executes concurrent, randomized filesystem operations (file creation, writes, truncates, renames, unlinks, and directories) across multiple worker threads to stress test the ext4/VFS layer:

```bash
# Run 4 concurrent worker processes performing 500 operations each
bash scripts/linux-test-project/test-syscalls.sh --test "fsstress -d /tmp/fsstress -p 4 -n 500 -l 1"
```

---

### Step 3: Run the Individual Subsystem Suites

#### A. System Calls Conformance & Stress (`test-syscalls.sh`)
```bash
# Run pipe syscall test
bash scripts/linux-test-project/test-syscalls.sh --test pipe01

# Run curated syscall suite
bash scripts/linux-test-project/test-syscalls.sh --tier1
```

#### B. Scheduler & Process Conformance (`test-scheduler.sh`)
Tests scheduling, priority yielding, and thread management:
```bash
# Run scheduler smoke test (sched_yield01)
bash scripts/linux-test-project/test-scheduler.sh

# Run specific scheduler test
bash scripts/linux-test-project/test-scheduler.sh --test sched_yield01
```

#### C. Inter-Process Communication Suite (`test-ipc.sh`)
Tests pipes, FIFOs, and message buffers:
```bash
# Run IPC smoke tests
bash scripts/linux-test-project/test-ipc.sh

# Run pipe test
bash scripts/linux-test-project/test-ipc.sh --pipes
```

#### D. Run All Test Suites Sequentially (`run-all.sh`)
```bash
bash scripts/linux-test-project/run-all.sh
```

---

## 2. Viewing Saved Logs (Git Excluded)

Every execution saves full serial and console output to:
- Timestamped logs: `scripts/linux-test-project/logs/<suite>-<YYYYMMDD-HHMMSS>.log`
- Latest symlink: `scripts/linux-test-project/logs/<suite>-latest.log`

To inspect the latest test results:
```bash
cat scripts/linux-test-project/logs/syscalls-latest.log
cat scripts/linux-test-project/logs/scheduler-latest.log
cat scripts/linux-test-project/logs/ipc-latest.log
```

> [!NOTE]
> All files in `scripts/linux-test-project/logs/` and `*.log` are added to [`.gitignore`](file:///home/ananta/PetraOS/.gitignore) and are never tracked or committed to git.

---

## 3. Script Reference (`scripts/linux-test-project/`)

| Script | Responsibility |
| :--- | :--- |
| **`build-ltp.sh`** | Downloads, compiles, strips, and installs LTP binaries into `/opt/ltp`, and repacks `build/ext4.img`. |
| **`test-syscalls.sh`** | Dedicated runner for system call tests and `fsstress`. |
| **`test-scheduler.sh`** | Dedicated runner for scheduler and concurrency tests. |
| **`test-ipc.sh`** | Dedicated runner for IPC tests (pipes, shared memory, semaphores). |
| **`run-all.sh`** | Master script executing syscalls, scheduler, and IPC back-to-back. |
| **`common.sh`** | Headless QEMU launcher, serial piping, and TPASS/TFAIL result metric parser. |
| **`guest-runner.sh`** | In-guest driver invoked inside PetraOS userspace by `/init`. |
