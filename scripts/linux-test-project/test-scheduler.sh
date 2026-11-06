#!/bin/bash
# ==============================================================================
# PetraOS LTP Scheduler & Process Conformance & Stress Runner
# Usage:
#   bash scripts/linux-test-project/test-scheduler.sh [OPTIONS]
# Options:
#   --all               Run all LTP scheduler test scenarios
#   --stress            Run multi-threaded scheduler stress tests
#   --test <name>       Run a specific scheduler test binary (e.g. sched_yield01)
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
    echo "  --all              Run full LTP sched scenario"
    echo "  --smoke            Run scheduler smoke tests (default)"
    echo "  --stress           Run fork/clone concurrency stress tests"
    echo "  --test <binary>    Run an individual scheduler test (e.g. sched_yield01, fork01)"
    echo "  -h, --help         Show this help message"
    echo ""
    echo "Logs are automatically saved to: ${LOG_DIR}/scheduler-<timestamp>.log"
    echo "Latest log symlink:               ${LOG_DIR}/scheduler-latest.log"
    exit 0
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --all)
            MODE="all"
            shift
            ;;
        --smoke)
            MODE="smoke"
            shift
            ;;
        --stress)
            MODE="stress"
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

GUEST_ARGS="scheduler"
if [ "${MODE}" = "single" ]; then
    if [ -z "${SPECIFIC_TEST}" ]; then
        echo "ERROR: --test requires a test binary name." >&2
        exit 1
    fi
    GUEST_ARGS="scheduler ${SPECIFIC_TEST}"
elif [ "${MODE}" = "stress" ]; then
    GUEST_ARGS="scheduler fork01 -i 100"
elif [ "${MODE}" = "all" ]; then
    GUEST_ARGS="scheduler"
fi

run_petra_ltp_suite "scheduler" "${GUEST_ARGS}"
