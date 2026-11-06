#!/bin/bash
# ==============================================================================
# PetraOS LTP In-Guest Test Driver
# Runs directly inside PetraOS userspace (/sysroot/opt/ltp) as the init command
# ==============================================================================

export PATH=/opt/ltp/testcases/bin:/opt/ltp/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
export LTP_COLORIZE=0
export LTPROOT=/opt/ltp

SUITE="${1:-syscalls}"
shift || true
EXTRA_ARGS="$*"

echo ""
echo "============================================================"
echo "    PetraOS Guest LTP Execution: ${SUITE}                   "
echo "============================================================"
echo "Date:         $(date 2>/dev/null || echo 'unknown')"
echo "Kernel:       $(uname -r 2>/dev/null || echo 'PetraOS')"
echo "Architecture: $(uname -m 2>/dev/null || echo 'x86_64')"
echo "Command Args: ${EXTRA_ARGS:-none}"
echo "============================================================"
echo ""

mkdir -p /tmp/ltp-scratch /opt/ltp/results 2>/dev/null || true
cd /tmp/ltp-scratch 2>/dev/null || cd /tmp

RUNLTP="/opt/ltp/runltp"
BIN_DIR="/opt/ltp/testcases/bin"
STATUS=0

execute_single_test() {
    local test_bin="$1"
    shift
    if [ -x "${BIN_DIR}/${test_bin}" ]; then
        echo "--> Running binary: ${test_bin} $*"
        "${BIN_DIR}/${test_bin}" "$@"
    elif command -v "${test_bin}" >/dev/null 2>&1; then
        echo "--> Running command: ${test_bin} $*"
        "${test_bin}" "$@"
    else
        echo "ERROR: Test binary not found: ${test_bin} in ${BIN_DIR}"
        return 1
    fi
}

case "${SUITE}" in
    syscalls)
        if [ -n "${EXTRA_ARGS}" ]; then
            execute_single_test ${EXTRA_ARGS} || STATUS=$?
        elif [ -f "/opt/ltp/runtest/petra-syscalls" ]; then
            ${RUNLTP} -f /opt/ltp/runtest/petra-syscalls || STATUS=$?
        elif [ -f "/opt/ltp/runtest/syscalls" ]; then
            ${RUNLTP} -f syscalls || STATUS=$?
        else
            echo "Running core syscall smoke tests..."
            for test in openat01 read01 write01 close01 stat01 mmap01 fork01 pipe01; do
                if [ -x "${BIN_DIR}/${test}" ]; then
                    "${BIN_DIR}/${test}" || STATUS=$?
                fi
            done
        fi
        ;;

    scheduler|sched)
        if [ -n "${EXTRA_ARGS}" ]; then
            execute_single_test ${EXTRA_ARGS} || STATUS=$?
        elif [ -f "/opt/ltp/runtest/sched" ]; then
            ${RUNLTP} -f sched || STATUS=$?
        else
            echo "Running scheduler smoke tests..."
            for test in sched_yield01 sched_getparam01 sched_setaffinity01 sched_setscheduler01 fork01 clone01; do
                if [ -x "${BIN_DIR}/${test}" ]; then
                    "${BIN_DIR}/${test}" || STATUS=$?
                fi
            done
        fi
        ;;

    ipc)
        if [ -n "${EXTRA_ARGS}" ]; then
            execute_single_test ${EXTRA_ARGS} || STATUS=$?
        elif [ -f "/opt/ltp/runtest/ipc" ]; then
            ${RUNLTP} -f ipc || STATUS=$?
        else
            echo "Running IPC smoke tests..."
            for test in pipe01 pipe02 pipe10 fifo01 msgget01 msgsnd01 msgrcv01 semget01 semop01 shmget01 shmat01; do
                if [ -x "${BIN_DIR}/${test}" ]; then
                    "${BIN_DIR}/${test}" || STATUS=$?
                fi
            done
        fi
        ;;

    single)
        execute_single_test ${EXTRA_ARGS} || STATUS=$?
        ;;

    *)
        echo "ERROR: Unknown suite '${SUITE}'"
        echo "Supported suites: syscalls, scheduler, ipc, single"
        STATUS=1
        ;;
esac

echo ""
echo "============================================================"
echo "    PetraOS Guest LTP Run Completed (Exit code: ${STATUS})  "
echo "============================================================"
echo ""

# Terminate VM cleanly
sync
if [ -x /sbin/poweroff ]; then
    /sbin/poweroff -f 2>/dev/null || true
fi
if [ -x /sbin/reboot ]; then
    /sbin/reboot -f 2>/dev/null || true
fi

# Fallback python exit trigger if available
if command -v python3 >/dev/null 2>&1; then
    python3 -c "import os; os.system('poweroff -f')" 2>/dev/null || true
fi

exit ${STATUS}
