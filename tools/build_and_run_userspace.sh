#!/usr/bin/env bash
# ==============================================================================
# PetraOS Userspace Build & Run Script
#
# Central engine for ALL userspace / xbstrap operations:
#   - Initialize the xbstrap workspace
#   - Download / fetch package source code
#   - Verify patches (applied by xbstrap during source prepare)
#   - Compile & install a single package or all packages into the sysroot
#   - Clean build artifacts
#   - Package initramfs, and launch PetraOS in QEMU
#
# Usage:
#   ./tools/build_and_run_userspace.sh                  # Core pipeline (mlibc, bash, coreutils) + QEMU
#   ./tools/build_and_run_userspace.sh --all            # Full pipeline (ALL packages) + QEMU
#   ./tools/build_and_run_userspace.sh <action> [pkg]   # Individual operation, see help
# ==============================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
BUILD_DIR_XBSTRAP="${ROOT_DIR}/build-xbstrap"
SYSROOT="${BUILD_DIR_XBSTRAP}/system-root"
SOURCES_DIR="${ROOT_DIR}/sources"
PACKAGES_DIR="${ROOT_DIR}/packages"

QEMU_EXTRA_FLAGS="${QEMUFLAGS:--m 2G -serial stdio}"

CORE_PACKAGES=(mlibc bash coreutils)

# Colors for terminal output
BOLD="\033[1m"
GREEN="\033[32m"
YELLOW="\033[33m"
BLUE="\033[34m"
RED="\033[31m"
RESET="\033[0m"

log_info() {
    echo -e "${BLUE}[INFO]${RESET} ${1}"
}

log_success() {
    echo -e "${GREEN}[SUCCESS]${RESET} ${1}"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${RESET} ${1}"
}

log_error() {
    echo -e "${RED}[ERROR]${RESET} ${1}" >&2
}

ensure_xbstrap() {
    if ! command -v xbstrap &>/dev/null; then
        log_error "'xbstrap' command was not found in PATH."
        log_error "Please ensure xbstrap is installed (e.g. pip install xbstrap)."
        exit 1
    fi
}

xbstrap_in_workspace() {
    (cd "${BUILD_DIR_XBSTRAP}" && xbstrap "${@}")
}

# ------------------------------------------------------------------------------
# xbstrap operations
# ------------------------------------------------------------------------------

cmd_init() {
    ensure_xbstrap
    if [ ! -d "${BUILD_DIR_XBSTRAP}" ] || [ ! -f "${BUILD_DIR_XBSTRAP}/bootstrap.link" ]; then
        log_info "Initializing xbstrap workspace in ${BUILD_DIR_XBSTRAP}..."
        mkdir -p "${BUILD_DIR_XBSTRAP}"
        (cd "${BUILD_DIR_XBSTRAP}" && xbstrap init ..)
        log_success "xbstrap workspace initialized."
    else
        log_info "xbstrap workspace already initialized."
    fi
}

cmd_fetch() {
    ensure_xbstrap
    cmd_init
    local pkg="${1:-all}"

    if [ "${pkg}" = "all" ] || [ "${pkg}" = "--all" ] || [ "${pkg}" = "-a" ]; then
        log_info "Downloading all sources defined in bootstrap.yml..."
        xbstrap_in_workspace fetch --all
        log_success "All package sources fetched."
    else
        # Check if source is already downloaded and non-empty
        if [ -d "${SOURCES_DIR}/${pkg}" ] && [ "$(ls -A "${SOURCES_DIR}/${pkg}" 2>/dev/null)" ]; then
            log_info "Source for '${pkg}' already exists at sources/${pkg}."
        else
            log_info "Downloading / fetching source for '${pkg}'..."
            xbstrap_in_workspace fetch "${pkg}"
            log_success "Source for '${pkg}' fetched."
        fi
    fi
}

cmd_patch() {
    ensure_xbstrap
    cmd_init
    local pkg="${1:-}"
    if [ -z "${pkg}" ]; then
        log_error "Please specify a package name to patch."
        exit 1
    fi

    local pkg_dir="${PACKAGES_DIR}/${pkg}"
    if [ ! -d "${pkg_dir}" ]; then
        log_error "Unknown package '${pkg}' (no packages/${pkg} directory)."
        exit 1
    fi

    local patch_count
    patch_count=$(find "${pkg_dir}" -maxdepth 1 -name "*.patch" | wc -l)
    if [ "${patch_count}" -gt 0 ]; then
        log_info "Package '${pkg}' has ${patch_count} patch file(s)."
        find "${pkg_dir}" -maxdepth 1 -name "*.patch" -printf "  - %f\n"
        log_info "Patches are applied by xbstrap automatically during source prepare."
    else
        log_info "No custom patch files found in packages/${pkg}."
    fi
}

build_package() {
    local pkg="${1}"
    # Fetch source first if missing; patches are applied by xbstrap on prepare.
    cmd_fetch "${pkg}"

    log_info "Building package '${pkg}' into sysroot..."
    xbstrap_in_workspace install "${pkg}"
    log_success "Package '${pkg}' built and installed successfully."
}

cmd_build() {
    ensure_xbstrap
    cmd_init
    local pkg="${1:-}"
    if [ -z "${pkg}" ]; then
        log_error "Please specify a package name to build."
        exit 1
    fi
    build_package "${pkg}"
}

discover_all_packages() {
    local packages=()
    local yml_file pkg_name

    # Try authoritative list from xbstrap if workspace is initialized.
    if [ -f "${BUILD_DIR_XBSTRAP}/bootstrap.link" ] && command -v xbstrap &>/dev/null; then
        local xb_pkgs
        xb_pkgs="$( (cd "${BUILD_DIR_XBSTRAP}" && xbstrap list-pkgs 2>/dev/null) || true )"
        if [ -n "${xb_pkgs}" ]; then
            # Ensure mlibc (and mlibc-headers) come first
            local ordered=()
            if echo "${xb_pkgs}" | grep -qw "mlibc"; then
                ordered+=("mlibc")
            fi
            local pkg
            while read -r pkg; do
                # xbstrap list-pkgs prints one package per line; handle space-separated too
                for pkg in ${pkg}; do
                    if [ "${pkg}" = "mlibc" ] || [ "${pkg}" = "mlibc-headers" ]; then
                        continue
                    fi
                    # avoid duplicates
                    if [[ " ${ordered[*]} " != *" ${pkg} "* ]]; then
                        ordered+=("${pkg}")
                    fi
                done
            done <<< "${xb_pkgs}"
            # Also ensure mlibc-headers is early if present
            if echo "${xb_pkgs}" | grep -qw "mlibc-headers"; then
                # insert after mlibc
                local tmp=()
                for pkg in "${ordered[@]}"; do
                    tmp+=("${pkg}")
                    if [ "${pkg}" = "mlibc" ]; then
                        tmp+=("mlibc-headers")
                    fi
                done
                # deduplicate mlibc-headers if already there
                ordered=()
                local seen=""
                for pkg in "${tmp[@]}"; do
                    if [[ "${seen}" != *"|${pkg}|"* ]]; then
                        ordered+=("${pkg}")
                        seen="${seen}|${pkg}|"
                    fi
                done
            fi
            echo "${ordered[*]:-}"
            return 0
        fi
    fi

    # Fallback: scan YML files that actually define a packages: section.
    # This avoids tool-only ports (e.g. autoconf before fix) and source-only
    # ports (e.g. gnulib) being treated as installable packages.
    for yml_file in "${PACKAGES_DIR}"/*/*.yml; do
        if ! grep -qE '^[[:space:]]*packages:' "${yml_file}" 2>/dev/null; then
            continue
        fi
        pkg_name="$(basename "${yml_file}" .yml)"
        if [ "${pkg_name}" = "mlibc" ]; then
            continue
        fi
        # Only add if the yml defines a package with that exact name,
        # or at least defines any package (for mlibc which defines mlibc-headers/mlibc)
        if grep -qE "name:[[:space:]]+${pkg_name}([[:space:]]|$)" "${yml_file}" 2>/dev/null || \
           grep -qE "name:[[:space:]]+mlibc" "${yml_file}" 2>/dev/null; then
            packages+=("${pkg_name}")
        else
            # Generic fallback: if file has packages: but name mismatch (e.g. mlibc.yml defines mlibc-headers),
            # extract first package name and use pkg_name if it seems intentional.
            # For now skip ambiguous entries and rely on xbstrap list-pkgs when possible.
            # Keep pkg_name if packages: exists — allows newly added target packages
            # (like autoconf) to be discovered even before xbstrap cache refresh.
            if grep -qE '^[[:space:]]*-[[:space:]]*name:' "${yml_file}" 2>/dev/null; then
                packages+=("${pkg_name}")
            fi
        fi
    done
    if [ -f "${PACKAGES_DIR}/mlibc/mlibc.yml" ]; then
        echo "mlibc ${packages[*]:-}"
    else
        echo "${packages[*]:-}"
    fi
}

cmd_build_all() {
    ensure_xbstrap
    cmd_init

    log_info "Fetching all package sources..."
    xbstrap_in_workspace fetch --all

    local packages
    read -r -a packages <<< "$(discover_all_packages)"
    if [ "${#packages[@]}" -eq 0 ]; then
        log_warn "No package definitions found in ${PACKAGES_DIR}."
        return 0
    fi

    log_info "Recompiling all ${#packages[@]} packages in bootstrap.yml..."
    local pkg
    for pkg in "${packages[@]}"; do
        log_info "==> Building ${pkg}..."
        xbstrap_in_workspace install "${pkg}"
    done
    log_success "All packages built successfully."
}

cmd_clean() {
    local pkg="${1:-}"
    if [ -z "${pkg}" ] || [ "${pkg}" = "all" ] || [ "${pkg}" = "--all" ]; then
        log_info "Cleaning full xbstrap build workspace (${BUILD_DIR_XBSTRAP})..."
        rm -rf "${BUILD_DIR_XBSTRAP}"
        log_success "Cleaned full xbstrap build directory."
    else
        log_info "Cleaning build artifacts for package '${pkg}'..."
        rm -rf "${BUILD_DIR_XBSTRAP}/packages/${pkg}"*
        rm -rf "${BUILD_DIR_XBSTRAP}/pkg-builds/${pkg}"*
        rm -rf "${BUILD_DIR_XBSTRAP}/pkg-stamps/${pkg}"*
        rm -f "${SYSROOT}/etc/xbstrap/${pkg}.installed"
        log_success "Cleaned build artifacts for '${pkg}'."
    fi
}

cmd_status() {
    local pkg="${1:-}"
    echo -e "${BOLD}PetraOS Package Status${RESET}"
    echo "Workspace initialized: $([ -f "${BUILD_DIR_XBSTRAP}/bootstrap.link" ] && echo -e "${GREEN}Yes${RESET}" || echo -e "${RED}No${RESET}")"

    if [ -n "${pkg}" ]; then
        echo -e "\nPackage: ${BOLD}${pkg}${RESET}"
        echo "Source downloaded : $([ -d "${SOURCES_DIR}/${pkg}" ] && echo -e "${GREEN}Yes (sources/${pkg})${RESET}" || echo -e "${YELLOW}No${RESET}")"
        echo "Patches present   : $(find "${PACKAGES_DIR}/${pkg}" -maxdepth 1 -name "*.patch" 2>/dev/null | grep -q . && echo -e "${GREEN}Yes${RESET}" || echo "None")"
        echo "Build directory   : $([ -d "${BUILD_DIR_XBSTRAP}/pkg-builds/${pkg}" ] && echo -e "${GREEN}Present${RESET}" || echo "Not built")"
    fi
}

# ------------------------------------------------------------------------------
# Full pipelines
# ------------------------------------------------------------------------------

run_pipeline() {
    local mode="${1:-core}"

    echo "============================================================"
    echo "          PetraOS Userspace Build & Launch Pipeline         "
    echo "============================================================"

    echo "[1/5] Initializing xbstrap workspace..."
    cmd_init

    if [ "${mode}" = "all" ]; then
        echo "[2/5] Downloading all sources via xbstrap-fetch..."
        cmd_fetch --all
        echo "[3/5] Building all userspace packages..."
        cmd_build_all
    else
        echo "[2/5] Fetching core packages (${CORE_PACKAGES[*]})..."
        local pkg
        for pkg in "${CORE_PACKAGES[@]}"; do
            cmd_fetch "${pkg}"
        done
        echo "[3/5] Building core userspace (mlibc-headers, mlibc, bash, coreutils)..."
        for pkg in "${CORE_PACKAGES[@]}"; do
            build_package "${pkg}"
        done
    fi

    echo "[4/5] Packaging initramfs cpio archive..."
    make initramfs

    echo "[5/5] Launching PetraOS in QEMU..."
    make run QEMUFLAGS="${QEMU_EXTRA_FLAGS}"
}

# ------------------------------------------------------------------------------
# CLI Dispatcher
# ------------------------------------------------------------------------------
show_help() {
    echo -e "${BOLD}Usage:${RESET} $0 [action] [package_name]"
    echo ""
    echo -e "${BOLD}Pipelines:${RESET}"
    echo "  (no action)       Core pipeline: init, fetch, build mlibc/bash/coreutils, initramfs, run QEMU"
    echo "  --all | all       Full pipeline: fetch and recompile ALL packages, initramfs, run QEMU"
    echo ""
    echo -e "${BOLD}Actions:${RESET}"
    echo "  init              Initialize xbstrap build directory"
    echo "  fetch <pkg|--all> Check and download package source code"
    echo "  patch <pkg>       Inspect / verify package patches (applied by xbstrap at prepare)"
    echo "  build <pkg>       Fetch, patch, compile and install package to sysroot"
    echo "  install <pkg>     Alias for build"
    echo "  build-all         Fetch and recompile all userspace packages"
    echo "  clean [pkg]       Clean package build cache or entire xbstrap workspace"
    echo "  status [pkg]      Show package and workspace status"
    echo "  help              Show this help message"
}

ACTION="${1:-}"
PKG_NAME="${2:-}"

case "${ACTION}" in
    ""|run)
        run_pipeline core
        ;;
    --all|all|run-all)
        run_pipeline all
        ;;
    init)
        cmd_init
        ;;
    fetch)
        cmd_fetch "${PKG_NAME:-all}"
        ;;
    patch)
        cmd_patch "${PKG_NAME}"
        ;;
    build|install)
        cmd_build "${PKG_NAME}"
        ;;
    build-all|rebuild-all)
        cmd_build_all
        ;;
    clean)
        cmd_clean "${PKG_NAME}"
        ;;
    status)
        cmd_status "${PKG_NAME}"
        ;;
    help|--help|-h)
        show_help
        ;;
    *)
        # If first argument matches a known package name, assume "build"
        if [ -d "${PACKAGES_DIR}/${ACTION}" ]; then
            cmd_build "${ACTION}"
        else
            log_error "Unknown action or package: '${ACTION}'"
            show_help
            exit 1
        fi
        ;;
esac
