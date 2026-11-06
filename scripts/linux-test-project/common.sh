#!/bin/bash
# ==============================================================================
# PetraOS Linux Test Project (LTP) Common Test Harness
# ==============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

LOG_DIR="${SCRIPT_DIR}/logs"
EXT4_IMG="${REPO_ROOT}/build/ext4.img"
INITRAMFS="${REPO_ROOT}/build/initramfs.cpio"

mkdir -p "${LOG_DIR}"

export PATH="${HOME}/.cargo/bin:${PATH}"

check_prerequisites() {
    if ! command -v cargo-osdk >/dev/null 2>&1 && ! command -v cargo >/dev/null 2>&1; then
        echo "ERROR: cargo-osdk or cargo not found in PATH." >&2
        echo "       Ensure ~/.cargo/bin is in PATH and cargo-osdk is installed." >&2
        exit 1
    fi

    if [ ! -f "${INITRAMFS}" ]; then
        echo "ERROR: Initramfs not found: ${INITRAMFS}" >&2
        echo "       Run 'bash scripts/build-userland/build-userland-amd64.sh' first." >&2
        exit 1
    fi

    local rootfs_staging="${REPO_ROOT}/build/userland-amd64/rootfs_staging"

    # Auto-provision LTP test binaries into rootfs_staging if missing
    if [ ! -f "${rootfs_staging}/opt/ltp/guest-runner.sh" ] || [ ! -f "${rootfs_staging}/opt/ltp/testcases/bin/fsstress" ]; then
        echo "==> LTP binaries or guest-runner missing from rootfs_staging."
        echo "    Auto-provisioning LTP and repacking ext4.img..."
        bash "${SCRIPT_DIR}/build-ltp.sh" --install
        bash "${SCRIPT_DIR}/build-ltp.sh" --repack
    elif [ ! -f "${EXT4_IMG}" ] || [ "${SCRIPT_DIR}/guest-runner.sh" -nt "${EXT4_IMG}" ]; then
        echo "==> guest-runner.sh updated or ext4.img missing. Repacking ext4.img..."
        cp -f "${SCRIPT_DIR}/guest-runner.sh" "${rootfs_staging}/opt/ltp/guest-runner.sh"
        chmod +x "${rootfs_staging}/opt/ltp/guest-runner.sh"
        bash "${SCRIPT_DIR}/build-ltp.sh" --repack
    fi

    if [ ! -f "${EXT4_IMG}" ]; then
        echo "ERROR: Rootfs disk image not found: ${EXT4_IMG}" >&2
        echo "       Run 'bash scripts/build-userland/build-userland-amd64.sh' first." >&2
        exit 1
    fi
}

# Runs a test suite inside PetraOS via QEMU and OSDK
# Usage: run_petra_ltp_suite <suite_name> <command_args...>
run_petra_ltp_suite() {
    local suite_name="$1"
    shift
    local guest_cmd="$*"

    check_prerequisites

    local timestamp
    timestamp="$(date +%Y%m%d-%H%M%S)"
    local log_file="${LOG_DIR}/${suite_name}-${timestamp}.log"
    local latest_symlink="${LOG_DIR}/${suite_name}-latest.log"

    echo "============================================================"
    echo " PetraOS LTP Test Runner: ${suite_name}"
    echo "============================================================"
    echo "Suite:      ${suite_name}"
    echo "Command:    ${guest_cmd}"
    echo "Log file:   ${log_file}"
    echo "Timestamp:  ${timestamp}"
    echo "============================================================"
    echo ""

    # Ensure guest runner is synced into rootfs staging if staging exists
    local rootfs_staging="${REPO_ROOT}/build/userland-amd64/rootfs_staging"
    if [ -d "${rootfs_staging}/opt/ltp" ] && [ -f "${SCRIPT_DIR}/guest-runner.sh" ]; then
        cp -f "${SCRIPT_DIR}/guest-runner.sh" "${rootfs_staging}/opt/ltp/guest-runner.sh"
        chmod +x "${rootfs_staging}/opt/ltp/guest-runner.sh"
    fi

    # Note: OSDK.toml already provides -serial stdio and -device isa-debug-exit.
    # We only supply -display none for headless execution.
    local qemu_extra_args="-display none"

    # Launch kernel in QEMU headlessly and pipe logs
    local exit_code=0
    (
        cd "${REPO_ROOT}"
        cargo osdk run \
            --qemu-args="${qemu_extra_args}" \
            --init-args="/opt/ltp/guest-runner.sh ${guest_cmd}" 2>&1
    ) | tee "${log_file}" || exit_code=$?

    ln -sfn "${log_file}" "${latest_symlink}"

    echo ""
    echo "============================================================"
    echo " Post-Execution Analysis: ${suite_name}"
    echo "============================================================"
    echo "Log saved at: ${log_file}"

    # Extract LTP metrics if present in the log
    local tpass tfail tconf tbrok
    tpass=$(grep -c "TPASS" "${log_file}" 2>/dev/null) || tpass=0
    tfail=$(grep -c "TFAIL" "${log_file}" 2>/dev/null) || tfail=0
    tconf=$(grep -c "TCONF" "${log_file}" 2>/dev/null) || tconf=0
    tbrok=$(grep -c "TBROK" "${log_file}" 2>/dev/null) || tbrok=0

    echo "Results Summary:"
    echo "  - Passed (TPASS):   ${tpass}"
    echo "  - Failed (TFAIL):   ${tfail}"
    echo "  - Skipped (TCONF):  ${tconf}"
    echo "  - Broken (TBROK):   ${tbrok}"
    echo "============================================================"

    if [ "${tfail}" -gt 0 ] || [ "${tbrok}" -gt 0 ]; then
        echo "❌ Status: FAILED (${tfail} failures, ${tbrok} broken)"
        return 1
    elif [ "${tpass}" -gt 0 ]; then
        echo "✅ Status: PASSED (${tpass} passed, ${tconf} skipped)"
        return 0
    else
        # If exit_code indicates QEMU isa-debug-exit success (e.g. 1 or 33) or 0
        if [ "${exit_code}" -eq 0 ] || [ "${exit_code}" -eq 1 ] || [ "${exit_code}" -eq 33 ]; then
            echo "ℹ️  Run completed with QEMU exit code ${exit_code}."
            return 0
        else
            echo "⚠️  Run exited with non-zero code ${exit_code}."
            return "${exit_code}"
        fi
    fi
}
