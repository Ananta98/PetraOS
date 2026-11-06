#!/bin/bash
# ==============================================================================
# PetraOS LTP System Calls Conformance & Stress Runner
# Usage:
#   bash scripts/linux-test-project/test-syscalls.sh [OPTIONS]
# Options:
#   --all               Run all LTP syscall test scenarios
#   --test <name>       Run a specific syscall test binary (e.g., openat01)
#   --filter <pattern>  Run tests matching pattern (e.g., mmap*)
#   -h, --help          Show this help message
# ==============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/common.sh"

MODE="tier1"
SPECIFIC_TEST=""

usage() {
    echo "Usage: $0 [OPTIONS]"
    echo ""
    echo "Options:"
    echo "  --all              Run full LTP syscalls suite"
    echo "  --tier1            Run curated core syscalls suite (default)"
    echo "  --test <binary>    Run an individual syscall test (e.g. openat01, mmap01)"
    echo "  --filter <pattern> Run tests matching pattern"
    echo "  -h, --help         Show this help message"
    echo ""
    echo "Logs are automatically saved to: ${LOG_DIR}/syscalls-<timestamp>.log"
    echo "Latest log symlink:               ${LOG_DIR}/syscalls-latest.log"
    exit 0
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --all)
            MODE="all"
            shift
            ;;
        --tier1)
            MODE="tier1"
            shift
            ;;
        --test)
            MODE="single"
            SPECIFIC_TEST="$2"
            shift 2
            ;;
        --filter)
            MODE="filter"
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

GUEST_ARGS="syscalls"
if [ "${MODE}" = "single" ]; then
    if [ -z "${SPECIFIC_TEST}" ]; then
        echo "ERROR: --test requires a test binary name." >&2
        exit 1
    fi
    GUEST_ARGS="syscalls ${SPECIFIC_TEST}"
elif [ "${MODE}" = "filter" ]; then
    GUEST_ARGS="syscalls ${SPECIFIC_TEST}"
elif [ "${MODE}" = "all" ]; then
    GUEST_ARGS="syscalls-all"
fi

run_petra_ltp_suite "syscalls" "${GUEST_ARGS}"
