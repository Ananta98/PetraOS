#!/usr/bin/env bash
# ==============================================================================
# PetraOS xbstrap Package Manager & Orchestrator
#
# Central engine for managing, downloading, patching, building, and cleaning
# userspace packages in PetraOS.
# ==============================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
BUILD_DIR_XBSTRAP="${ROOT_DIR}/build-xbstrap"
SYSROOT="${BUILD_DIR_XBSTRAP}/system-root"
SOURCES_DIR="${ROOT_DIR}/sources"
PACKAGES_DIR="${ROOT_DIR}/packages"

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
    
    if [ "${pkg}" = "all" ] || [ "${pkg}" = "--all" ]; then
        log_info "Fetching all sources defined in bootstrap.yml..."
        (cd "${BUILD_DIR_XBSTRAP}" && xbstrap fetch --all)
        log_success "All package sources fetched."
    else
        # Check if source is already downloaded and non-empty
        if [ -d "${SOURCES_DIR}/${pkg}" ] && [ "$(ls -A "${SOURCES_DIR}/${pkg}" 2>/dev/null)" ]; then
            log_info "Source for '${pkg}' already exists at sources/${pkg}."
        else
            log_info "Downloading / fetching source for '${pkg}'..."
            (cd "${BUILD_DIR_XBSTRAP}" && xbstrap fetch "${pkg}")
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
    if [ -d "${pkg_dir}" ]; then
        local patch_count
        patch_count=$(find "${pkg_dir}" -maxdepth 1 -name "*.patch" | wc -l)
        if [ "${patch_count}" -gt 0 ]; then
            log_info "Package '${pkg}' has ${patch_count} patch file(s). xbstrap applies them during source prepare."
        else
            log_info "No custom patch files found in packages/${pkg}."
        fi
    fi
}

cmd_build() {
    ensure_xbstrap
    cmd_init
    local pkg="${1:-}"
    if [ -z "${pkg}" ]; then
        log_error "Please specify a package name to build."
        exit 1
    fi

    # Check if package source is downloaded; if not, fetch it first
    cmd_fetch "${pkg}"

    log_info "Building package '${pkg}' into sysroot..."
    (cd "${BUILD_DIR_XBSTRAP}" && xbstrap install "${pkg}")
    log_success "Package '${pkg}' built and installed successfully."
}

cmd_build_all() {
    ensure_xbstrap
    cmd_init
    log_info "Fetching all package sources..."
    (cd "${BUILD_DIR_XBSTRAP}" && xbstrap fetch --all)
    
    log_info "Building all packages in bootstrap.yml..."
    local packages=(mlibc ncurses readline bash automake libtool binutils gcc coreutils tzdata pkg-config)
    if [ -f "${PACKAGES_DIR}/nano/nano.yml" ]; then
        packages+=(nano)
    fi

    for pkg in "${packages[@]}"; do
        log_info "==> Building ${pkg}..."
        (cd "${BUILD_DIR_XBSTRAP}" && xbstrap install "${pkg}")
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
        rm -f "${BUILD_DIR_XBSTRAP}/system-root/etc/xbstrap/${pkg}.installed"
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
# CLI Dispatcher
# ------------------------------------------------------------------------------
show_help() {
    echo -e "${BOLD}Usage:${RESET} $0 <action> [package_name]"
    echo ""
    echo -e "${BOLD}Actions:${RESET}"
    echo "  init              Initialize xbstrap build directory"
    echo "  fetch <pkg|--all> Check and download package source code"
    echo "  patch <pkg>       Inspect / verify package patches"
    echo "  build <pkg>       Fetch, patch, compile and install package to sysroot"
    echo "  install <pkg>     Alias for build"
    echo "  build-all         Fetch and build all userspace packages"
    echo "  clean [pkg]       Clean package build cache or entire xbstrap workspace"
    echo "  status [pkg]      Show package and workspace status"
    echo "  help              Show this help message"
}

ACTION="${1:-help}"
PKG_NAME="${2:-}"

case "${ACTION}" in
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
    build-all|all)
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
