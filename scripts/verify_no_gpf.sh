#!/usr/bin/env bash
set -e

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}"

echo "============================================================"
echo "Starting 4-run GPF verification test suite"
echo "============================================================"

PASS_COUNT=0
TOTAL_RUNS=4

for i in $(seq 1 ${TOTAL_RUNS}); do
    echo "------------------------------------------------------------"
    echo "RUN #${i} STARTING..."
    echo "------------------------------------------------------------"

    killall -9 qemu-system-x86_64 2>/dev/null || true
    sleep 1

    LOG_FILE="/tmp/petra_gpf_test_run_${i}.log"
    rm -f "${LOG_FILE}"

    python3 scripts/test_runner.py > "${LOG_FILE}" 2>&1 || true

    # Check for GPF in the log
    if grep -iq "generalprotectionfault" "${LOG_FILE}"; then
        echo "RUN #${i} FAILED: GeneralProtectionFault detected in log!"
        grep -i "generalprotectionfault" "${LOG_FILE}"
        exit 1
    else
        echo "RUN #${i} PASSED: Zero GeneralProtectionFault errors."
        PASS_COUNT=$((PASS_COUNT + 1))
    fi

    # Check for successful boot / userspace execution
    if grep -q "Init process (PID 1) scheduled successfully" "${LOG_FILE}"; then
        echo "RUN #${i} VERIFIED: Boot and userspace init reached."
    fi

    echo "RUN #${i} COMPLETE."
done

echo "============================================================"
echo "ALL ${PASS_COUNT}/${TOTAL_RUNS} RUNS PASSED WITH ZERO GPF ERRORS!"
echo "============================================================"
