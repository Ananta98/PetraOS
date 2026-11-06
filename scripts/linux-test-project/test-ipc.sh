#!/bin/bash
# ==============================================================================
# PetraOS LTP Inter-Process Communication (IPC) Runner
# Usage:
#   bash scripts/linux-test-project/test-ipc.sh [OPTIONS]
# Options:
#   --all               Run all LTP IPC test scenarios
#   --pipes             Run pipe and FIFO tests
#   --shm               Run shared memory tests (SysV & POSIX)
#   --sem               Run semaphore tests
#   --msg               Run message queue tests
#   --test <name>       Run a specific IPC test binary (e.g. pipe01, shmget01)
#   -h, --help          Show this help message
# ==============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/common.sh"

MODE="default"
SPECIFIC_TEST=""

usage() {
    echo "Usage: $0 [OPTIONS]"
    echo ""
    echo "Options:"
    echo "  --all              Run full LTP ipc scenario"
    echo "  --pipes            Run pipe and FIFO tests"
    echo "  --shm              Run shared memory tests (shmget, shmat)"
    echo "  --sem              Run semaphore tests (semget, semop)"
    echo "  --msg              Run message queue tests (msgget, msgsnd, msgrcv)"
    echo "  --test <binary>    Run an individual IPC test (e.g. pipe01, shmget01)"
    echo "  -h, --help         Show this help message"
    echo ""
    echo "Logs are automatically saved to: ${LOG_DIR}/ipc-<timestamp>.log"
    echo "Latest log symlink:               ${LOG_DIR}/ipc-latest.log"
    exit 0
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --all)
            MODE="all"
            shift
            ;;
        --pipes)
            MODE="pipes"
            shift
            ;;
        --shm)
            MODE="shm"
            shift
            ;;
        --sem)
            MODE="sem"
            shift
            ;;
        --msg)
            MODE="msg"
            shift
            ;;
        --test)
            MODE="single"
            SPECIFIC_TEST="$2"
            shift 2
            ;;
        -h|--help)
            usage
            ;;
        *)
            echo "ERROR: Unknown option '$1'" >&2
            usage
            ;;
    esac
done

GUEST_ARGS="ipc"
if [ "${MODE}" = "single" ]; then
    if [ -z "${SPECIFIC_TEST}" ]; then
        echo "ERROR: --test requires a test binary name." >&2
        exit 1
    fi
    GUEST_ARGS="ipc ${SPECIFIC_TEST}"
elif [ "${MODE}" = "pipes" ]; then
    GUEST_ARGS="ipc pipe01"
elif [ "${MODE}" = "shm" ]; then
    GUEST_ARGS="ipc shmget01"
elif [ "${MODE}" = "sem" ]; then
    GUEST_ARGS="ipc semget01"
elif [ "${MODE}" = "msg" ]; then
    GUEST_ARGS="ipc msgget01"
elif [ "${MODE}" = "all" ]; then
    GUEST_ARGS="ipc"
fi

run_petra_ltp_suite "ipc" "${GUEST_ARGS}"
