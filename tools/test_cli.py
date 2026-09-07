#!/usr/bin/env python3
"""
PetraOS CLI Test Launcher
=========================
Convenience launcher for the PetraOS CLI testing harness.
Delegates to .agents/skills/cli-testing/scripts/test_cli.py.
"""

import os
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
SKILL_SCRIPT = REPO_ROOT / ".agents" / "skills" / "cli-testing" / "scripts" / "test_cli.py"

if not SKILL_SCRIPT.exists():
    print(f"[ERROR] Test runner script not found at {SKILL_SCRIPT}", file=sys.stderr)
    sys.exit(1)

# Execute the test runner replacing current process
os.execv(sys.executable, [sys.executable, str(SKILL_SCRIPT)] + sys.argv[1:])
