#!/usr/bin/env bash
# ==============================================================================
# PetraOS Userland Build & Run Script
#
# Central engine for ALL userspace / xbstrap operations:
#   - Initialize the xbstrap workspace
#   - Download / fetch package source code
#   - Verify patches (applied by xbstrap during source prepare)
#   - Compile & install host cross-tools (toolchain bootstrap)
#   - Compile & install target packages into sysroot
#   - Clean build artifacts and sources
#   - Package initramfs, and launch PetraOS in QEMU
#
# Usage:
#   ./tools/build_userland.sh                  # Full pipeline (tools + packages) + QEMU
#   ./tools/build_userland.sh <action> [pkg]   # Individual operation, see help
# ==============================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
BUILD_DIR_XBSTRAP="${ROOT_DIR}/build-xbstrap"
SYSROOT="${BUILD_DIR_XBSTRAP}/system-root"
SOURCES_DIR="${ROOT_DIR}/sources"
PACKAGES_DIR="${ROOT_DIR}/packages"

QEMU_EXTRA_FLAGS="${QEMUFLAGS:--m 4G -serial stdio}"

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
# Workspace operations
# ------------------------------------------------------------------------------

cmd_init() {
    ensure_xbstrap
    if [ ! -L "${ROOT_DIR}/patches" ] && [ ! -d "${ROOT_DIR}/patches" ]; then
        ln -sf packages "${ROOT_DIR}/patches"
    fi
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

# ------------------------------------------------------------------------------
# Build operations
# ------------------------------------------------------------------------------

build_package() {
    local pkg="${1}"
    cmd_fetch "${pkg}"

    log_info "Building package '${pkg}' into sysroot..."
    xbstrap_in_workspace build "${pkg}"
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

cmd_build_tools() {
    ensure_xbstrap
    cmd_init

    log_info "============================================================"
    log_info " [Phase 1/2] Building Cross-Compiler Tools & C Library      "
    log_info "============================================================"

    # 1. Host autotools for configuration generation
    log_info "--> [1/6] Building host-autoconf-v2.69..."
    xbstrap_in_workspace install-tool host-autoconf-v2.69

    log_info "--> [2/6] Building host-automake-v1.16..."
    xbstrap_in_workspace install-tool host-automake-v1.16

    # 2. Host cross-binutils (as, ld for x86_64-petra)
    log_info "--> [3/6] Building host-binutils..."
    xbstrap_in_workspace install-tool host-binutils

    # 3. Target C library headers
    log_info "--> [4/7] Building mlibc-headers..."
    xbstrap_in_workspace build mlibc-headers

    # 4. Target C library runtime (crt0, crti, crtn, libc.a, libc.so)
    log_info "--> [5/7] Building mlibc runtime..."
    xbstrap_in_workspace build mlibc

    # 5. Host cross-GCC and C++ runtime (compiler -> libgcc -> libstdc++)
    log_info "--> [6/7] Building host-gcc and runtime libraries..."
    xbstrap_in_workspace install-tool host-gcc

    # 6. Additional host utilities
    log_info "--> [7/7] Building host utilities (host-zic, host-gnulib)..."
    xbstrap_in_workspace install-tool host-zic
    xbstrap_in_workspace install-tool host-gnulib

    log_success "Host tools and cross-compiler toolchain built successfully."
}

cmd_build_packages() {
    ensure_xbstrap
    cmd_init

    log_info "============================================================"
    log_info " [Phase 2/2] Building Target Userspace Packages            "
    log_info "============================================================"

    local target_pkgs=(
        base-files
        ncurses
        readline
        bash
        coreutils
        sed
        grep
        gawk
        libxcrypt
        shadow
        sudo
        tzdata
        nano
        vim
        fastfetch
        pkg-config
        libtool
        autoconf
        automake
        binutils
        gcc
        curl
    )

    local total="${#target_pkgs[@]}"
    local i=1
    for pkg in "${target_pkgs[@]}"; do
        log_info "--> [${i}/${total}] Building target package: ${pkg}..."
        xbstrap_in_workspace build "${pkg}"
        i=$((i + 1))
    done

    log_success "All target userspace packages built and installed into sysroot."
}

cmd_build_all() {
    ensure_xbstrap
    cmd_init
    cmd_build_tools
    cmd_build_packages
    log_success "Full userspace build (tools + packages) completed successfully."
}

cmd_clean() {
    local target="${1:-}"
    if [ -z "${target}" ] || [ "${target}" = "all" ] || [ "${target}" = "--all" ]; then
        log_info "Cleaning full xbstrap build workspace (${BUILD_DIR_XBSTRAP})..."
        rm -rf "${BUILD_DIR_XBSTRAP}"
        log_info "Cleaning downloaded sources (${SOURCES_DIR})..."
        rm -rf "${SOURCES_DIR}"
        log_success "Cleaned full xbstrap build directory and sources directory (clean slate)."
    elif [ "${target}" = "build" ]; then
        log_info "Cleaning build workspace only (${BUILD_DIR_XBSTRAP})..."
        rm -rf "${BUILD_DIR_XBSTRAP}"
        log_success "Cleaned xbstrap build directory."
    elif [ "${target}" = "sources" ]; then
        log_info "Cleaning sources directory (${SOURCES_DIR})..."
        rm -rf "${SOURCES_DIR}"
        log_success "Cleaned sources directory."
    else
        log_info "Cleaning build artifacts for package '${target}'..."
        rm -rf "${BUILD_DIR_XBSTRAP}/packages/${target}"*
        rm -rf "${BUILD_DIR_XBSTRAP}/pkg-builds/${target}"*
        rm -rf "${BUILD_DIR_XBSTRAP}/pkg-stamps/${target}"*
        rm -f "${SYSROOT}/etc/xbstrap/${target}.installed"
        log_success "Cleaned build artifacts for '${target}'."
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
# Full pipeline
# ------------------------------------------------------------------------------

run_pipeline() {
    echo "============================================================"
    echo "          PetraOS Userland Build & Launch Pipeline          "
    echo "============================================================"

    echo "[1/4] Initializing xbstrap workspace..."
    cmd_init

    echo "[2/4] Building full userspace (tools + packages)..."
    cmd_build_all

    echo "[3/4] Packaging initramfs cpio archive..."
    make initramfs

    echo "[4/4] Launching PetraOS in QEMU..."
    make run QEMUFLAGS="${QEMU_EXTRA_FLAGS}"
}

# ------------------------------------------------------------------------------
# CLI Dispatcher
# ------------------------------------------------------------------------------
show_help() {
    echo -e "${BOLD}Usage:${RESET} $0 [action] [package_name]"
    echo ""
    echo -e "${BOLD}Pipeline:${RESET}"
    echo "  (no args) | run   Full pipeline: init, build tools, build packages, initramfs, run QEMU"
    echo ""
    echo -e "${BOLD}Actions:${RESET}"
    echo "  init              Initialize xbstrap build directory"
    echo "  fetch [pkg]       Download package source code (all packages by default)"
    echo "  patch <pkg>       Inspect / verify package patches"
    echo "  build-tools       Build host cross-compiler tools & mlibc runtime (Phase 1)"
    echo "  build-packages    Build all target userspace packages into sysroot (Phase 2)"
    echo "  build-all         Full userspace build: build-tools then build-packages"
    echo "  build <pkg>       Build and install a single package"
    echo "  install <pkg>     Alias for build"
    echo "  clean [target]    Clean workspace ('all' or empty wipes build + sources; 'build', 'sources', or <pkg>)"
    echo "  status [pkg]      Show package and workspace status"
    echo "  help              Show this help message"
}

ACTION="${1:-}"
PKG_NAME="${2:-}"

case "${ACTION}" in
    ""|run|run-all|--all|all)
        run_pipeline
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
    build-tools|tools)
        cmd_build_tools
        ;;
    build-packages|packages)
        cmd_build_packages
        ;;
    build-all|rebuild-all)
        cmd_build_all
        ;;
    build|install)
        cmd_build "${PKG_NAME}"
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
        if [ -d "${PACKAGES_DIR}/${ACTION}" ]; then
            cmd_build "${ACTION}"
        else
            log_error "Unknown action or package: '${ACTION}'"
            show_help
            exit 1
        fi
        ;;
esac
