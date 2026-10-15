---
name: code-review
description: Review PetraOS code changes for clean file structure, readability, simplicity, loose coupling, clean abstraction architecture, and compilation verification via cargo osdk. Use when completing any code modification or when asked to review changes.
---

# Code Review Skill for PetraOS

A systematic, pragmatic code review runbook to ensure high code quality, simplicity, loose coupling, clean abstractions, and build correctness across all changes to PetraOS.

---

## When to Use

1. **Mandatory Post-Task Review**: Triggered at the end of every task where code in `silica/` or the repository is added or modified.
2. **Explicit Review Requests**: When asked to audit, clean up, or review code quality, architecture, or file organization.

---

## Review Steps

### 1. Diff Inspection & File Structure
Run `git status` and `git diff` to inspect all staged and unstaged modifications.

- [ ] **Proper Location**: Are new modules/files placed in the right subsystem under `silica/src/` (e.g. `fs/`, `drivers/`, `net/`, `vm/`, `proc/`, `sched/`, `ipc/`, `syscall/`)?
- [ ] **Module Hierarchy**: Are new files cleanly registered in their parent `mod.rs` without cluttering the root module?
- [ ] **Minimal Visibility**: Is visibility restricted to `pub(crate)` or private unless specifically required by external modules or traits?

### 2. Simplicity & Readability Audit
- [ ] **Simplicity First**: Is this the simplest solution that works? Eliminate unnecessary wrapper layers, helper traits, or premature abstractions.
- [ ] **Readability**: Are identifiers explicit and self-explanatory? Is control flow easy to follow without convoluted nesting? From logic perspectives including variable name, should be easier to understand. it's fine little longer variable name at least user when read understand proper logic
- [ ] **Cleanliness**:
  - No leftover debugging statements (`println!`, temporary logs).
  - No commented-out dead code.
  - No unused imports or compiler warnings introduced.
- [ ] **Meaningful Comments**: Doc comments focus on design rationale and safety invariants (`# Safety`), not repeating obvious function signatures.

### 3. Decoupling & Abstraction Architecture
- [ ] **Loose Coupling**: Subsystems interact via well-defined traits or APIs, without depending directly on internal struct fields of other subsystems.
- [ ] **No Leaky Abstractions**: Implementation details (e.g. specific driver registers, hardware flags) do not leak into general kernel subsystems (like `fs` or `proc`).
- [ ] **No Circular Dependencies**: Subsystem dependencies flow cleanly in one direction (e.g., `syscall` -> `proc`/`fs` -> `drivers`/`arch`).

### 4. Build & Runtime Verification (Gatekeeper)
The review is only successful if the kernel compiles cleanly:

```bash
export PATH="$HOME/.cargo/bin:$PATH"
cargo osdk check
cargo osdk build
```

Or run the bundled verification script:
```bash
bash .agents/skills/code-review/scripts/verify.sh
```

- [ ] `cargo osdk check` exits with code `0`.
- [ ] `cargo osdk build` exits with code `0`.
- [ ] If runtime behavior changed, verify that the kernel boots or relevant test cases pass (`cargo osdk test` or QEMU boot test where applicable).

---

## Quick Review Checklist

Before declaring any coding task complete, verify:
- **Structure**: Files in right subsystem directories & registered cleanly in `mod.rs`.
- **Readability**: Clear names, readable logic, no dead code or leftover debug logs.
- **Coupling**: Clean abstraction boundaries, minimal visibility, no circular ties.
- **Build**: `cargo osdk check` & `cargo osdk build` pass cleanly.
