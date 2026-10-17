# Skill: Bug Verification & Anti-Loop Gate

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