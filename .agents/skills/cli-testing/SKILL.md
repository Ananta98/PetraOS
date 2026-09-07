---
name: cli-testing
description: Test CLI mode, shell commands (bash, sh, or other shells), and terminal environments in PetraOS via QEMU (using 'make run' with 4GB memory) or host. Use this skill when asked to test CLI commands, verify shell behavior, run automated command test suites, take screenshots of the QEMU framebuffer display for visual diagnostics, and analyze failure causes when commands fail, hang, or panic.
---

# CLI & Shell Testing Skill (`cli-testing`)

This skill provides automated testing and visual diagnostic tools for evaluating CLI mode, shells (bash, sh, or custom shells), and command execution in **PetraOS** running in QEMU, as well as on host environments.

---

## 1. Overview & Key Capabilities

- **QEMU Integration via `make run`**: Launches PetraOS using `make run` with **4 GB memory** (`-m 4G`), serial console stdio, and a dedicated QEMU monitor socket.
- **Automated Command Execution**: Sends commands into the interactive shell line discipline and parses standard output, standard error, and exit codes (`$?`).
- **Framebuffer Screenshot Capture (`screendump`)**: Interacts with the QEMU monitor to capture full-resolution PNG screenshots of the virtual display.
- **Visual Failure Diagnosis for AI**: Automatically captures screenshots on test failures, timeouts, or kernel panics, enabling the AI to visually inspect screen state using `view_file`.
- **Comprehensive Reporting**: Generates both human-readable Markdown (`test_reports/report.md`) and machine-readable JSON (`test_reports/results.json`).

---

## 2. Test Runner Quick Reference

The primary test harness is located at [`.agents/skills/cli-testing/scripts/test_cli.py`](file:///home/ananta/PetraOS/.agents/skills/cli-testing/scripts/test_cli.py) and can also be invoked via the root launcher [`tools/test_cli.py`](file:///home/ananta/PetraOS/tools/test_cli.py).

### 2.1 Run a Single CLI Command in PetraOS
```bash
# Test command and check for expected substring
python3 tools/test_cli.py --cmd "echo Hello PetraOS" --expect "Hello PetraOS"

# Test directory listing
python3 tools/test_cli.py --cmd "ls /bin" --expect "bash"

# Test environment variable expansion
python3 tools/test_cli.py --cmd "export MY_VAR=Petra; echo \$MY_VAR" --expect "Petra"
```

### 2.2 Run the Default CLI Test Suite
```bash
# Execute the standard core CLI test suite in QEMU
python3 tools/test_cli.py --test-file .agents/skills/cli-testing/scripts/default_tests.json
```

### 2.3 Standalone Display Screenshot
```bash
# Boot PetraOS in QEMU, take a snapshot of the display screen, and exit
python3 tools/test_cli.py --screenshot-only
```

### 2.4 Test Host Bash Mode (Sanity Checking)
```bash
# Run tests directly in host bash pseudo-terminal
python3 tools/test_cli.py --target host --cmd "echo 'Host CLI OK'" --expect "Host CLI OK"
python3 tools/test_cli.py --target host --test-file .agents/skills/cli-testing/scripts/default_tests.json
```

---

## 3. Command-Line Arguments Reference

| Flag | Default | Description |
| :--- | :--- | :--- |
| `--target` | `qemu` | Target environment (`qemu` for PetraOS VM, `host` for local shell). |
| `--cmd "<cmd>"` | `None` | Single command to execute and test. |
| `--expect "<str>"` | `None` | Expected substring in stdout for single command mode. |
| `--test-file <path>` | `default_tests.json` | Path to JSON test suite file. |
| `--memory <mem>` | `4G` | RAM allocated to QEMU via `make run QEMUFLAGS="-m 4G ..."`. |
| `--boot-timeout <sec>`| `45.0` | Seconds to wait for kernel boot and initial shell prompt. |
| `--cmd-timeout <sec>` | `15.0` | Per-command timeout in seconds before marking test as timed out. |
| `--headless` | `True` | Run QEMU with `-display none` (fast, leaves display buffer active). |
| `--gui` | `False` | Run QEMU with graphical window display enabled. |
| `--screenshot-on-fail`| `True` | Automatically take a PNG screenshot on any test failure or timeout. |
| `--screenshot-always` | `False` | Capture a screenshot after every single command. |
| `--screenshot-only` | `False` | Boot VM, capture screenshot to `test_reports/screenshots/`, and exit. |
| `--reports-dir <path>`| `test_reports` | Directory where markdown reports and screenshots are saved. |

---

## 4. Automated AI Failure Diagnostic Protocol

When an AI agent is testing CLI mode and encounters a failure, hang, or error, follow this 5-step diagnostic procedure:

```
+---------------------------+
| 1. Check Test Results    | --> Read report.md / results.json
+---------------------------+
              |
              v
+---------------------------+
| 2. Read Serial Output     | --> Check panic, page fault, or bash error
+---------------------------+
              |
              v
+---------------------------+
| 3. View Screenshot (.png) | --> Use view_file on captured screenshot
+---------------------------+
              |
              v
+---------------------------+
| 4. Visual Analysis        | --> Correlate screen state with kernel logs
+---------------------------+
              |
              v
+---------------------------+
| 5. Root Cause & Fix       | --> Modify kernel/driver/sysroot (userspace) code
+---------------------------+
```

### Step 1: Check Test Results
Open and review [`test_reports/report.md`](file:///home/ananta/PetraOS/test_reports/report.md) or [`test_reports/results.json`](file:///home/ananta/PetraOS/test_reports/results.json). Identify the failing command, expected vs actual output, and exit code.

### Step 2: Read Serial Console Output
Inspect the serial console log printed during test execution:
- Look for **Kernel Panic**, **Double Fault**, **Page Fault Exception**, or **General Protection Fault**.
- Look for shell warnings: `bash: command not found`, `No such file or directory`, or dynamic linker errors (`cannot load library`).

### Step 3: View the Failure Screenshot
The runner automatically dumps a PNG screenshot to `test_reports/screenshots/fail_<test_name>_<timestamp>.png`.
Use the `view_file` tool on this PNG file to visually inspect the exact framebuffer state:
```markdown
view_file(AbsolutePath="/home/ananta/PetraOS/test_reports/screenshots/fail_test_name_YYYYMMDD_HHMMSS.png")
```

### Step 4: Visual Diagnosis Taxonomy
Evaluate what is rendered on screen:
1. **Limine Boot Menu Stuck**: Bootloader did not load the kernel or configuration was malformed.
2. **Early Kernel Splash / Blank Screen**: Kernel failed before framebuffer console (`flanterm`) or TTY initialization.
3. **Flanterm Active, Prompt Visible, No Command Response**: Shell line discipline (`LineDiscipline::accept_input_byte`) or serial polling (`poll_input`) failed to pass stdin characters to the process.
4. **Command Printed but Hung**: The command spawned a child process that blocked on a pipe, futex, or I/O syscall without returning.
5. **Red Kernel Panic Screen / Stack Dump**: An unhandled page fault or assertion panic occurred in kernel space. Note the Faulting Address (`CR2`), Instruction Pointer (`RIP`), and Error Code.

### Step 5: Formulate Root Cause & Apply Fix
- If an interrupt or stack pointer issue: check [`tss.rs`](file:///home/ananta/PetraOS/kernel/src/arch/x86_64/cpu/tss.rs) and [`stack.rs`](file:///home/ananta/PetraOS/kernel/src/arch/x86_64/cpu/stack.rs).
- If a memory allocation / page table fault: check [`table.rs`](file:///home/ananta/PetraOS/kernel/src/arch/x86_64/paging/table.rs) or [`brk.rs`](file:///home/ananta/PetraOS/kernel/src/syscalls/mm/brk.rs).
- If a terminal / character echo fault: check [`console.rs`](file:///home/ananta/PetraOS/kernel/src/drivers/tty/console.rs) and [`termios.rs`](file:///home/ananta/PetraOS/kernel/src/drivers/tty/termios.rs).
- If a missing userspace binary: check [`packages/`](file:///home/ananta/PetraOS/packages/) and [`tools/create_initramfs.sh`](file:///home/ananta/PetraOS/tools/create_initramfs.sh).

---

## 5. Test Suite Schema (`default_tests.json`)

Custom test suites can be defined in JSON format:

```json
{
  "suite_name": "My Custom CLI Test Suite",
  "description": "Tests for shell commands and utilities",
  "tests": [
    {
      "name": "test_unique_id",
      "command": "echo 'Testing 1 2 3'",
      "expect_contains": "Testing 1 2 3",
      "expect_not_contains": ["error", "panic"],
      "expect_regex": "^Testing",
      "expect_exit_code": 0,
      "timeout": 10.0
    }
  ]
}
```

### Test Fields:
- **`name`** (string): Unique identifier for the test case.
- **`command`** (string): Command string sent to the shell.
- **`expect_contains`** (string or list of strings): Substrings that MUST appear in stdout.
- **`expect_not_contains`** (string or list of strings): Substrings that MUST NOT appear in stdout.
- **`expect_regex`** (string, optional): Regular expression pattern that must match the output.
- **`expect_exit_code`** (integer, optional): Expected exit code (`0` for success).
- **`timeout`** (float, optional): Command timeout in seconds (default: 15.0s).
