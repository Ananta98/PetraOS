#!/bin/bash
# ==============================================================================
# PetraOS Linux Test Project (LTP) Build & Packaging Helper
# Prepares, compiles, and packages LTP testcases into PetraOS rootfs_staging (/opt/ltp)
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
    echo "  --build              Configure and compile core LTP libraries & stress tools"
    echo "  --install            Install compiled test binaries into rootfs_staging (/opt/ltp)"
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

    echo "==> Compiling LTP test driver (pan)..."
    make -j"$(nproc)" LDFLAGS="-static -L${BUILD_DIR}/lib" -C pan

    echo "==> Compiling filesystem stress tools (fsstress)..."
    make -j"$(nproc)" LDFLAGS="-static -L${BUILD_DIR}/lib" -C testcases/kernel/fs/fsstress

    echo "==> Compiling core system call & scheduler testcases..."
    for testdir in sched_yield pipe; do
        make -j"$(nproc)" LDFLAGS="-static -L${BUILD_DIR}/lib" -C "testcases/kernel/syscalls/${testdir}"
    done
}

install_ltp() {
    echo "==> [3/4] Installing LTP binaries into ${TARGET_OPT}..."
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

    # Copy compiled test binaries
    for bin_path in "${BUILD_DIR}"/testcases/kernel/syscalls/sched_yield/sched_yield* \
                    "${BUILD_DIR}"/testcases/kernel/syscalls/pipe/pipe*; do
        if [ -f "${bin_path}" ] && [ -x "${bin_path}" ]; then
            cp -f "${bin_path}" "${TARGET_OPT}/testcases/bin/"
            strip "${TARGET_OPT}/testcases/bin/$(basename "${bin_path}")" 2>/dev/null || true
        fi
    done

    # Install scenario files
    cat > "${TARGET_OPT}/runtest/petra-syscalls" << 'EOF'
pipe01 pipe01
pipe02 pipe02
EOF

    cat > "${TARGET_OPT}/runtest/petra-sched" << 'EOF'
sched_yield01 sched_yield01
EOF

    cat > "${TARGET_OPT}/runtest/petra-ipc" << 'EOF'
pipe01 pipe01
pipe02 pipe02
EOF

    echo "Installed binaries in ${TARGET_OPT}/testcases/bin:"
    ls -lh "${TARGET_OPT}/testcases/bin"
}

repack_rootfs() {
    echo "==> [4/4] Repacking PetraOS filesystem images..."
    bash "${REPO_ROOT}/scripts/build-initramfs.sh"
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
        echo " LTP Setup Complete!"
        echo " You can now run:"
        echo "   bash scripts/linux-test-project/test-syscalls.sh --test fsstress"
        echo "   bash scripts/linux-test-project/test-scheduler.sh"
        echo "   bash scripts/linux-test-project/test-ipc.sh"
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
