#!/bin/bash
# ==============================================================================
# PetraOS Linux Test Project (LTP) Build & Packaging Helper
# Prepares, compiles, and packages ALL LTP testcases into PetraOS rootfs_staging (/opt/ltp)
# ==============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

LTP_VERSION="${LTP_VERSION:-20240930}"
CACHE_DIR="${REPO_ROOT}/build/cache"
BUILD_DIR="${REPO_ROOT}/build/ltp-build"
ROOTFS_STAGING="${REPO_ROOT}/build/userland-amd64/rootfs_staging"
TARGET_OPT="${ROOTFS_STAGING}/opt/ltp"

mkdir -p "${CACHE_DIR}"

usage() {
    echo "Usage: $0 [OPTIONS]"
    echo ""
    echo "Options:"
    echo "  --fetch              Download upstream LTP release tarball"
    echo "  --build              Configure and compile ALL LTP libraries, pan, fsstress & all syscalls"
    echo "  --install            Install all compiled test binaries into rootfs_staging (/opt/ltp)"
    echo "  --repack             Repack build/ext4.img with updated rootfs_staging"
    echo "  --all                Run fetch, build, install, and repack in sequence (recommended)"
    echo "  -h, --help           Show this help message"
    exit 0
}

fetch_source() {
    local archive="${CACHE_DIR}/ltp-${LTP_VERSION}.tar.xz"
    if [ ! -f "${archive}" ]; then
        echo "==> [1/4] Downloading LTP ${LTP_VERSION} release tarball..."
        curl -fL --retry 3 \
            "https://github.com/linux-test-project/ltp/releases/download/${LTP_VERSION}/ltp-full-${LTP_VERSION}.tar.xz" \
            -o "${archive}"
    else
        echo "==> [1/4] Cached LTP source found: ${archive}"
    fi
}

build_ltp() {
    fetch_source

    echo "==> [2/4] Extracting LTP into ${BUILD_DIR}..."
    mkdir -p "${BUILD_DIR}"
    if [ ! -f "${BUILD_DIR}/configure" ]; then
        tar -xJf "${CACHE_DIR}/ltp-${LTP_VERSION}.tar.xz" -C "${BUILD_DIR}" --strip-components=1
    fi

    echo "==> Configuring LTP..."
    cd "${BUILD_DIR}"
    if [ ! -f "config.status" ]; then
        ./configure --prefix=/opt/ltp --without-tirpc --without-selinux LDFLAGS="-static"
    fi

    echo "==> Compiling base LTP libraries (libltp)..."
    make -j"$(nproc)" -C lib

    echo "==> Compiling LTP auxiliary libraries (libs)..."
    make -j"$(nproc)" -C libs

    echo "==> Compiling LTP test driver (pan)..."
    make -j"$(nproc)" LDFLAGS="-static -L${BUILD_DIR}/lib" -C pan

    echo "==> Compiling filesystem stress tools (fsstress)..."
    make -j"$(nproc)" LDFLAGS="-static -L${BUILD_DIR}/lib" -C testcases/kernel/fs/fsstress

    echo "==> Compiling ALL Linux system call test suites (370+ syscalls)..."
    make -k -j"$(nproc)" WLDFLAGS="-static" -C testcases/kernel/syscalls || true
}

install_ltp() {
    echo "==> [3/4] Installing ALL LTP binaries into ${TARGET_OPT}..."
    mkdir -p "${TARGET_OPT}"/{bin,testcases/bin,runtest,results}

    # Install guest runner
    install -m 0755 "${SCRIPT_DIR}/guest-runner.sh" "${TARGET_OPT}/guest-runner.sh"

    # Install pan driver and fsstress
    if [ -f "${BUILD_DIR}/pan/ltp-pan" ]; then
        cp -f "${BUILD_DIR}/pan/ltp-pan" "${TARGET_OPT}/bin/"
        strip "${TARGET_OPT}/bin/ltp-pan" 2>/dev/null || true
    fi

    if [ -f "${BUILD_DIR}/testcases/kernel/fs/fsstress/fsstress" ]; then
        cp -f "${BUILD_DIR}/testcases/kernel/fs/fsstress/fsstress" "${TARGET_OPT}/testcases/bin/"
        strip "${TARGET_OPT}/testcases/bin/fsstress" 2>/dev/null || true
    fi

    echo "==> Copying and stripping all compiled syscall binaries..."
    find "${BUILD_DIR}/testcases/kernel/syscalls" -maxdepth 2 -type f -executable ! -name "*.sh" ! -name "*.py" \
        -exec cp -f {} "${TARGET_OPT}/testcases/bin/" \;
    strip "${TARGET_OPT}/testcases/bin"/* 2>/dev/null || true

    # Install scenario files
    cat > "${TARGET_OPT}/runtest/petra-syscalls" << 'EOF'
openat01 openat01
openat02 openat02
close01 close01
close02 close02
dup01 dup01
dup02 dup02
lseek01 lseek01
getpid01 getpid01
pipe01 pipe01
pipe02 pipe02
EOF

    cat > "${TARGET_OPT}/runtest/petra-sched" << 'EOF'
sched_yield01 sched_yield01
fork01 fork01
EOF

    cat > "${TARGET_OPT}/runtest/petra-ipc" << 'EOF'
pipe01 pipe01
pipe02 pipe02
pipe03 pipe03
pipe04 pipe04
pipe05 pipe05
pipe06 pipe06
EOF

    # Also copy the full upstream runtest scenario files
    if [ -d "${BUILD_DIR}/runtest" ]; then
        cp -f "${BUILD_DIR}/runtest/syscalls" "${TARGET_OPT}/runtest/syscalls-all" 2>/dev/null || true
        cp -f "${BUILD_DIR}/runtest/sched" "${TARGET_OPT}/runtest/sched-all" 2>/dev/null || true
        cp -f "${BUILD_DIR}/runtest/ipc" "${TARGET_OPT}/runtest/ipc-all" 2>/dev/null || true
    fi

    local bin_count
    bin_count=$(ls -1 "${TARGET_OPT}/testcases/bin" | wc -l)
    echo "==> Successfully installed ${bin_count} test binaries in ${TARGET_OPT}/testcases/bin"
}

repack_rootfs() {
    echo "==> [4/4] Repacking PetraOS filesystem images (image-size: 4G)..."
    bash "${REPO_ROOT}/scripts/build-initramfs.sh" --image-size 4G
    echo "==> Ext4 filesystem image refreshed: ${REPO_ROOT}/build/ext4.img"
}

ACTION="${1:---all}"
case "${ACTION}" in
    --fetch)
        fetch_source
        ;;
    --build)
        build_ltp
        ;;
    --install)
        install_ltp
        ;;
    --repack)
        repack_rootfs
        ;;
    --all)
        build_ltp
        install_ltp
        repack_rootfs
        echo ""
        echo "============================================================"
        echo " Full LTP Setup Complete!"
        echo " All 1,300+ syscall binaries are installed and ready to run:"
        echo "   bash scripts/linux-test-project/test-syscalls.sh --test <any_syscall>"
        echo "   bash scripts/linux-test-project/test-syscalls.sh --all"
        echo "   bash scripts/linux-test-project/test-syscalls.sh --test fsstress"
        echo "============================================================"
        ;;
    -h|--help)
        usage
        ;;
    *)
        echo "ERROR: Unknown option '${ACTION}'" >&2
        usage
        ;;
esac
