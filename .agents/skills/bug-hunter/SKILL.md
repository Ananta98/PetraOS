---
name: bug-hunter
description: To find bugs in PetraOS code, run a single pass of static analysis and dynamic testing to identify reproducible defects. Use when asked to find bugs or when a defect is reported. Also to stop looping bug scans that generate false positives.
---

## Intent
Enforce deterministic reproduction for any reported defect. Prevent open-ended 
or recursive bug scans that generate false positives.

## Strict Rules
1. Ground Truth Requirement:
   - Every candidate bug must have an executable reproduction script, unit test, 
     or clear trace showing actual execution failure against project specs.
   - Theoretical flaws, missing optimizations, and linter-level style suggestions 
     are NOT bugs.

2. Loop Termination:
   - Perform a maximum of ONE scanning pass.
   - If no reproducible test failure exists after the initial pass, output:
     "SCAN COMPLETE: No verifiable functional defects found."
   - Immediately terminate. Do NOT run further inspection tools or re-scan code.

3. Candidate Triage (Skeptic Gate):
   - For every finding, attempt to refute it using existing guards/types/validations.
   - If refuted or unverifiable, discard silently without reporting.

4. Mandatory `claude-bughunter` Verification:
   - Always cross-verify findings and fixes using the installed `claude-bughunter` skills before finalizing:
     - **`triage-validation`**: Run every candidate bug through the 7-Question Gate and false-positive/always-rejected checks to confirm real impact and avoid speculative findings.
     - **`evidence-hygiene`**: Ensure every reported defect and fix has concrete, verifiable code traces, failing conditions, or test evidence.
     - **`bb-methodology`**: Apply the critical-thinking checks (developer assumptions, anomaly detection, What-If edge cases) during the single scanning pass.
     - **Targeted `hunt-*` checks** (as applicable to the modified/audited subsystem):
       - `hunt-auth-bypass` (permission, credential, capability, and privilege boundary enforcement)
       - `hunt-race-condition` (TOCTOU, lock splitting, concurrent state mutations)
       - `hunt-exceptional-conditions` (integer overflow, unbounded allocations/OOM, malformed inputs, error paths)
       - `hunt-business-logic` (state-machine and POSIX/Linux invariant violations)
       - `hunt-lfi` (VFS path traversal and symlink/mount boundary handling)
   - After applying fixes, re-verify that the defect is fully closed (no partial guards or bypassable checks) and run `cargo osdk check` / `cargo osdk build`.
