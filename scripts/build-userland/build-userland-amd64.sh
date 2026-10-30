#!/bin/bash
# Assemble the PetraOS amd64 userland from an official Rocky Linux rootfs and
# optional prebuilt package layers, then hand the staging root to the initramfs packer.
#
# Usage: build-userland-amd64.sh [--fresh] [--purge-cache]
#
#   --fresh        remove existing userland artifacts before rebuilding
#   --purge-cache  with --fresh, also drop the download cache and re-fetch Rocky Linux
#
# Environment:
#   PETRAOS_USERLAND_PACKAGES  space separated prebuild-package layer names (default: empty)
#   PETRAOS_VERSION            version string reported by /etc/os-release
#   ROCKY_VERSION/ROCKY_ARCH/ROCKY_SHA256  override the Rocky Linux release
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

ROCKY_VERSION="${ROCKY_VERSION:-9}"
ROCKY_ARCH="x86_64"
ROCKY_SHA256="${ROCKY_SHA256:-1210df99dcf0ef4d73940244e6d703935175629598b09c0e430da20e76768402}"
ROCKY_TAR_NAME="Rocky-${ROCKY_VERSION}-Container-Base.latest.${ROCKY_ARCH}.tar.xz"
ROCKY_URL="${ROCKY_URL:-https://dl.rockylinux.org/pub/rocky/${ROCKY_VERSION}/images/${ROCKY_ARCH}/${ROCKY_TAR_NAME}}"

CACHE_DIR="${REPO_ROOT}/build/cache"
BUILD_DIR="${REPO_ROOT}/build/userland-amd64"
STAGING="${BUILD_DIR}/staging"
ROOTFS_STAGING="${BUILD_DIR}/rootfs_staging"
INITRAMFS="${REPO_ROOT}/build/initramfs.cpio"

PACKAGES="${PETRAOS_USERLAND_PACKAGES:-}"
PETRAOS_VERSION="${PETRAOS_VERSION:-0.1.0}"
SUPPORT_DIR="${REPO_ROOT}/scripts/build-support"
PREBUILD_DIR="${REPO_ROOT}/scripts/prebuild-package"

FRESH=0
PURGE_CACHE=0

for arg in "$@"; do
    case "$arg" in
        --fresh) FRESH=1 ;;
        --purge-cache) PURGE_CACHE=1 ;;
        -h|--help)
            sed -n '2,/^set -/s/^# \{0,1\}//p' "$0"
            exit 0
            ;;
        *)
            echo "ERROR: unknown option: $arg" >&2
            exit 1
            ;;
    esac
done

if [ "${FRESH}" -eq 1 ]; then
    clean_args=(--userland)
    [ "${PURGE_CACHE}" -eq 1 ] && clean_args+=(--cache)
    "${SCRIPT_DIR}/../clean.sh" "${clean_args[@]}"
fi

for tool in curl tar file; do
    command -v "$tool" >/dev/null 2>&1 || {
        echo "ERROR: required command not found: $tool" >&2
        exit 1
    }
done

sha256_file() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    else
        shasum -a 256 "$1" | awk '{print $1}'
    fi
}

merge_layer() {
    local layer="$1"
    local target_dir="${2:-${STAGING}}"
    local source relative destination
    [ -d "$layer" ] || return 0
    while IFS= read -r -d '' source; do
        relative="${source#"$layer/"}"
        destination="${target_dir}/${relative}"
        if [ -L "$destination" ]; then
            rm -f "$destination"
        fi
    done < <(find "$layer" -mindepth 1 -print0)
    cp -a "$layer/." "${target_dir}/"
}

mkdir -p "${CACHE_DIR}" "${BUILD_DIR}"

if [ ! -f "${CACHE_DIR}/${ROCKY_TAR_NAME}" ]; then
    echo "==> Fetching Rocky Linux ${ROCKY_VERSION} ${ROCKY_ARCH} rootfs..."
    curl -fL --retry 3 "${ROCKY_URL}" -o "${CACHE_DIR}/${ROCKY_TAR_NAME}"
fi

actual_sha256="$(sha256_file "${CACHE_DIR}/${ROCKY_TAR_NAME}")"
if [ -n "${ROCKY_SHA256}" ] && [ "${actual_sha256}" != "${ROCKY_SHA256}" ]; then
    echo "ERROR: Rocky Linux rootfs checksum mismatch." >&2
    echo "Expected: ${ROCKY_SHA256}" >&2
    echo "Actual:   ${actual_sha256}" >&2
    exit 1
fi

echo "==> Extracting Rocky Linux userland into ${STAGING}"
chmod -R u+rwX "${STAGING}" "${ROOTFS_STAGING}" 2>/dev/null || true
rm -rf "${STAGING}" "${ROOTFS_STAGING}"
mkdir -p "${STAGING}" "${ROOTFS_STAGING}"

TMP_EXTRACT="$(mktemp -d "${BUILD_DIR}/.rocky_extract.XXXXXX")"
tar -xJf "${CACHE_DIR}/${ROCKY_TAR_NAME}" -C "${TMP_EXTRACT}"

# Find the rootfs layer (the large tar+gzip blob in the OCI archive)
LAYER_BLOB="$(find "${TMP_EXTRACT}/blobs/sha256" -type f -size +10M | head -n 1)"
if [ -z "${LAYER_BLOB}" ] || [ ! -f "${LAYER_BLOB}" ]; then
    echo "ERROR: failed to locate rootfs layer blob in Rocky Linux image." >&2
    chmod -R u+rwX "${TMP_EXTRACT}" 2>/dev/null || true
    rm -rf "${TMP_EXTRACT}"
    exit 1
fi

tar -xzf "${LAYER_BLOB}" -C "${STAGING}"
chmod -R u+rwX "${TMP_EXTRACT}" 2>/dev/null || true
rm -rf "${TMP_EXTRACT}"

# Ensure read, write, and execute permissions across staging
chmod -R u+rwX "${STAGING}" 2>/dev/null || true

# Seed the rootfs staging with base files
cp -a "${STAGING}/." "${ROOTFS_STAGING}/"

# Stage optional prebuilt package layers if requested
if [ -n "${PACKAGES}" ]; then
    echo "==> Staging prebuilt package layers into rootfs (${ROOTFS_STAGING})"
    for package in ${PACKAGES}; do
        layer_script="${PREBUILD_DIR}/build-${package}.sh"
        if [ ! -x "${layer_script}" ]; then
            echo "ERROR: no prebuild-package script for '${package}': ${layer_script}" >&2
            exit 1
        fi
        echo "    ${package}"
        "${layer_script}"
        merge_layer "${REPO_ROOT}/build/prebuild-package/${package}/staging" "${ROOTFS_STAGING}"
        merge_layer "${REPO_ROOT}/build/prebuild-package/${package}/staging" "${STAGING}"
    done
fi

echo "==> Configuring the userlands (initramfs and rootfs)"
for target_dir in "${STAGING}" "${ROOTFS_STAGING}"; do
    chmod -R u+w "${target_dir}" 2>/dev/null || true
    mkdir -p "${target_dir}"/{dev/pts,dev/shm,proc,sys,tmp,run,root,mnt,media,var/log,var/run,var/tmp}
    chmod 1777 "${target_dir}/tmp" "${target_dir}/var/tmp"
    chmod 0750 "${target_dir}/root"
    install -m0755 "${SUPPORT_DIR}/init-amd64/init" "${target_dir}/init"
done

for target_dir in "${STAGING}" "${ROOTFS_STAGING}"; do
    if [ ! -f "${target_dir}/etc/passwd" ]; then
        cat > "${target_dir}/etc/passwd" << 'EOF'
root:x:0:0:root:/root:/bin/bash
bin:x:1:1:bin:/bin:/sbin/nologin
daemon:x:2:2:daemon:/sbin:/sbin/nologin
nobody:x:65534:65534:nobody:/:/sbin/nologin
EOF
    else
        sed -i 's|^root:.*|root:x:0:0:root:/root:/bin/bash|' "${target_dir}/etc/passwd"
    fi

    if [ ! -f "${target_dir}/etc/shells" ]; then
        cat > "${target_dir}/etc/shells" << 'EOF'
/bin/sh
/bin/bash
/usr/bin/sh
/usr/bin/bash
EOF
    fi

    if [ ! -f "${target_dir}/etc/hosts" ]; then
        cat > "${target_dir}/etc/hosts" << 'EOF'
127.0.0.1 localhost petraos
::1       localhost ip6-localhost ip6-loopback
EOF
    fi

    cat > "${target_dir}/etc/resolv.conf" << 'EOF'
nameserver 10.0.2.3
nameserver 1.1.1.1
nameserver 8.8.8.8
EOF

    cat > "${target_dir}/etc/hostname" << 'EOF'
petraos
EOF

    cat > "${target_dir}/etc/environment" << 'EOF'
CHARSET=UTF-8
LANG=C.UTF-8
LC_ALL=C.UTF-8
EOF

    printf '%s\n' "${PETRAOS_VERSION}" > "${target_dir}/etc/petraos-release"

    mkdir -p "${target_dir}/usr/lib"
    cat > "${target_dir}/usr/lib/os-release" << EOF
NAME="PetraOS"
PRETTY_NAME="PetraOS Silica"
ID=petraos
ID_LIKE="rocky rhel fedora"
VERSION_ID="${PETRAOS_VERSION}"
VERSION="${PETRAOS_VERSION}"
EOF
    rm -f "${target_dir}/etc/os-release"
    ln -sf ../usr/lib/os-release "${target_dir}/etc/os-release"

    cat > "${target_dir}/etc/profile" << 'EOF'
export PATH=/bin:/sbin:/usr/bin:/usr/sbin:/usr/local/bin:/usr/local/sbin
export HOME=/root
export SHELL=/bin/bash
export TERM=linux
export XDG_CONFIG_HOME=/root/.config
export CHARSET=UTF-8
export LANG=C.UTF-8
export LC_ALL=C.UTF-8
export PS1='petraos:\w# '
alias ll='ls -la --color=auto'
alias l='ls -lh'
umask 022

for script in /etc/profile.d/*.sh ; do
    if [ -r "$script" ] ; then
        . "$script"
    fi
done
unset script
EOF

    cat > "${target_dir}/root/.profile" << 'EOF'
export PS1='petraos:\w# '
export PATH=/bin:/sbin:/usr/bin:/usr/sbin:/usr/local/bin:/usr/local/sbin
export SHELL=/bin/bash
export TERM=linux
export HOME=/root
export XDG_CONFIG_HOME=/root/.config
export CHARSET=UTF-8
export LANG=C.UTF-8
export LC_ALL=C.UTF-8
alias ll='ls -la --color=auto'
alias l='ls -lh'
EOF

    # Best-effort device nodes; skipped when the build runs unprivileged.
    mknod -m 666 "${target_dir}/dev/null" c 1 3 2>/dev/null || true
    mknod -m 666 "${target_dir}/dev/zero" c 1 5 2>/dev/null || true
    mknod -m 666 "${target_dir}/dev/tty" c 5 0 2>/dev/null || true
    mknod -m 600 "${target_dir}/dev/console" c 5 1 2>/dev/null || true
    mknod -m 666 "${target_dir}/dev/ptmx" c 5 2 2>/dev/null || true
done

for path in "${STAGING}/usr/bin/bash" "${STAGING}/usr/lib64/ld-linux-x86-64.so.2" "${STAGING}/bin/sh"; do
    if [ ! -e "$path" ]; then
        echo "ERROR: Rocky Linux userland is incomplete, missing ${path}" >&2
        exit 1
    fi
done

echo "==> Packaging the initramfs and disk images"
"${SCRIPT_DIR}/../build-initramfs.sh" --staging "${STAGING}" --rootfs-staging "${ROOTFS_STAGING}" --output "${INITRAMFS}"

file "${STAGING}/usr/bin/bash"
file "${STAGING}/usr/lib64/ld-linux-x86-64.so.2"
