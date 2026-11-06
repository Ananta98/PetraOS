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

mkdir -p /tmp/ltp-scratch /tmp/fsstress /opt/ltp/results 2>/dev/null || true
cd /tmp/ltp-scratch 2>/dev/null || cd /tmp

BIN_DIR="/opt/ltp/testcases/bin"
STATUS=0

execute_single_test() {
    local test_bin="$1"
    shift || true

    # Special handling for fsstress: ensure destination directory exists and verbose is enabled
    if [ "${test_bin}" = "fsstress" ]; then
        local target_dir="/tmp/fsstress"
        mkdir -p "${target_dir}" 2>/dev/null || true
        local args="$*"
        # Add default directory if not specified
        if ! echo "${args}" | grep -q -- "-d"; then
            args="-d ${target_dir} ${args}"
        fi
        # Add verbose mode if not specified so operations are printed
        if ! echo "${args}" | grep -q -- "-v"; then
            args="-v ${args}"
        fi
        # Default processes and operations if not specified
        if ! echo "${args}" | grep -q -- "-p"; then
            args="-p 4 -n 100 -l 1 ${args}"
        fi

        echo "--> Running fsstress torture test: ${BIN_DIR}/fsstress ${args}"
        if "${BIN_DIR}/fsstress" ${args}; then
            echo ""
            echo "fsstress: TPASS: Filesystem stress test completed successfully without kernel panic."
            return 0
        else
            local ret=$?
            echo ""
            echo "fsstress: TFAIL: fsstress exited with error code ${ret}"
            return ${ret}
        fi
    fi

    if [ -x "${BIN_DIR}/${test_bin}" ]; then
        echo "--> Running test binary: ${test_bin} $*"
        "${BIN_DIR}/${test_bin}" "$@"
    elif command -v "${test_bin}" >/dev/null 2>&1; then
        echo "--> Running system command: ${test_bin} $*"
        "${test_bin}" "$@"
    else
        echo "ERROR: Test binary not found: ${test_bin} in ${BIN_DIR}"
        return 1
    fi
}

run_scenario_tests() {
    local scenario_file="$1"
    local default_list="$2"

    if [ -f "${scenario_file}" ]; then
        echo "Running tests from scenario file: ${scenario_file}"
        while read -r tag cmd rest; do
            # Skip empty lines and comments
            [ -z "$tag" ] && continue
            case "$tag" in \#*) continue ;; esac

            cmd="${cmd:-$tag}"
            if [ -x "${BIN_DIR}/${cmd}" ]; then
                echo "------------------------------------------------------------"
                echo "--> [${tag}] Executing: ${cmd} ${rest}"
                "${BIN_DIR}/${cmd}" ${rest} || STATUS=$?
            else
                echo "--> [${tag}] Skipped: binary '${cmd}' not installed in ${BIN_DIR}"
            fi
        done < "${scenario_file}"
    else
        echo "Running default test list..."
        for test in ${default_list}; do
            if [ -x "${BIN_DIR}/${test}" ]; then
                echo "------------------------------------------------------------"
                echo "--> Executing: ${test}"
                "${BIN_DIR}/${test}" || STATUS=$?
            fi
        done
    fi
}

case "${SUITE}" in
    syscalls)
        if [ -n "${EXTRA_ARGS}" ]; then
            execute_single_test ${EXTRA_ARGS} || STATUS=$?
        else
            run_scenario_tests "/opt/ltp/runtest/petra-syscalls" \
                "openat01 openat02 close01 close02 dup01 dup02 lseek01 getpid01 fork01 pipe01 pipe02"
        fi
        ;;

    syscalls-all)
        echo "Running full upstream system calls scenario (/opt/ltp/runtest/syscalls-all)..."
        run_scenario_tests "/opt/ltp/runtest/syscalls-all" ""
        ;;

    scheduler|sched)
        if [ -n "${EXTRA_ARGS}" ]; then
            execute_single_test ${EXTRA_ARGS} || STATUS=$?
        else
            run_scenario_tests "/opt/ltp/runtest/petra-sched" \
                "sched_yield01 fork01"
        fi
        ;;

    ipc)
        if [ -n "${EXTRA_ARGS}" ]; then
            execute_single_test ${EXTRA_ARGS} || STATUS=$?
        else
            run_scenario_tests "/opt/ltp/runtest/petra-ipc" \
                "pipe01 pipe02 pipe03 pipe04 pipe05 pipe06"
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

exit ${STATUS}
