#!/bin/bash
# ==============================================================================
# PetraOS LTP Master Runner (Syscalls, Scheduler, IPC)
# Usage:
#   bash scripts/linux-test-project/run-all.sh
# ==============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/common.sh"

echo "============================================================"
echo " Starting Full PetraOS LTP Test Suite Run                  "
echo " Suites: syscalls, scheduler, ipc                           "
echo " Logs:   ${LOG_DIR} (ignored by git)                        "
echo "============================================================"
echo ""

FAILURES=0

echo "[1/3] Executing System Calls Suite..."
if ! bash "${SCRIPT_DIR}/test-syscalls.sh" "$@"; then
    echo "⚠️  Syscalls suite completed with warnings/failures."
    FAILURES=$((FAILURES + 1))
fi

echo ""
echo "[2/3] Executing Scheduler & Process Suite..."
if ! bash "${SCRIPT_DIR}/test-scheduler.sh" "$@"; then
    echo "⚠️  Scheduler suite completed with warnings/failures."
    FAILURES=$((FAILURES + 1))
fi

echo ""
echo "[3/3] Executing Inter-Process Communication (IPC) Suite..."
if ! bash "${SCRIPT_DIR}/test-ipc.sh" "$@"; then
    echo "⚠️  IPC suite completed with warnings/failures."
    FAILURES=$((FAILURES + 1))
fi

echo ""
echo "============================================================"
echo " LTP Master Run Finished"
echo " Total Failed Suites: ${FAILURES}/3"
echo " Logs directory:      ${LOG_DIR}"
echo "============================================================"

if [ "${FAILURES}" -eq 0 ]; then
    echo "🎉 All LTP suites completed cleanly!"
    exit 0
else
    echo "❌ Some LTP suites reported failures. Inspect logs in ${LOG_DIR}."
    exit 1
fi
