#!/bin/bash
# Assemble the PetraOS amd64 userland from an official Rocky Linux rootfs,
# automatically resolve binary dependencies for a lightweight initramfs,
# and hand both rootfs and initramfs to the filesystem packer.
#
# Usage: build-userland-amd64.sh [--fresh] [--purge-cache] [--no-verify] [--tools "tool1 tool2"]
#
#   --fresh        Remove existing userland staging before rebuilding
#   --purge-cache  With --fresh, also purge the download cache and re-fetch base rootfs
#   --no-verify    Skip SHA256 checksum verification of the downloaded archive
#   --tools        Space-separated list of additional tools to include in the initramfs
#
# Environment:
#   EXTRA_INITRAMFS_TOOLS  Additional tools to include in the initramfs
#   PETRAOS_VERSION        Version string reported by /etc/os-release (default: 0.1.0)
#   ROCKY_VERSION          Rocky Linux release version (default: 9)
#   ROCKY_URL              Direct URL to Rocky Linux base container / rootfs tarball
#   ROCKY_SHA256           Expected SHA256 checksum of base archive (optional)
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

ROCKY_VERSION="${ROCKY_VERSION:-9}"
ROCKY_ARCH="${ROCKY_ARCH:-x86_64}"
ROCKY_SHA256="${ROCKY_SHA256:-1210df99dcf0ef4d73940244e6d703935175629598b09c0e430da20e76768402}"
ROCKY_TAR_NAME="Rocky-${ROCKY_VERSION}-Container-Base.latest.${ROCKY_ARCH}.tar.xz"
ROCKY_URL="${ROCKY_URL:-https://dl.rockylinux.org/pub/rocky/${ROCKY_VERSION}/images/${ROCKY_ARCH}/${ROCKY_TAR_NAME}}"

CACHE_DIR="${REPO_ROOT}/build/cache"
BUILD_DIR="${REPO_ROOT}/build/userland-amd64"
STAGING="${BUILD_DIR}/staging"
ROOTFS_STAGING="${BUILD_DIR}/rootfs_staging"
INITRAMFS="${REPO_ROOT}/build/initramfs.cpio"

PETRAOS_VERSION="${PETRAOS_VERSION:-0.1.0}"
SUPPORT_DIR="${REPO_ROOT}/scripts/build-support"

FRESH=0
PURGE_CACHE=0
VERIFY_CHECKSUM=1
EXTRA_TOOLS="${EXTRA_INITRAMFS_TOOLS:-${PETRAOS_USERLAND_PACKAGES:-}}"

usage() {
    cat << EOF
Usage: $(basename "$0") [OPTIONS]

Assemble the PetraOS amd64 userland and initramfs from official Rocky Linux.

Options:
  --fresh          Remove existing staging artifacts and rebuild from base
  --purge-cache    Purge downloaded cache and re-fetch Rocky Linux archive
  --no-verify      Skip SHA256 checksum verification of base archive
  --tools "TOOLS"  Additional tools to include into the initramfs
  -h, --help       Show this help message
EOF
    exit 0
}

while [ $# -gt 0 ]; do
    case "$1" in
        --fresh) FRESH=1 ;;
        --purge-cache) PURGE_CACHE=1 ;;
        --no-verify) VERIFY_CHECKSUM=0 ;;
        --tools)
            EXTRA_TOOLS="${EXTRA_TOOLS} $2"
            shift
            ;;
        -h|--help) usage ;;
        *)
            echo "ERROR: unknown option: $1" >&2
            usage
            ;;
    esac
    shift
done

if [ "${FRESH}" -eq 1 ]; then
    clean_args=(--userland)
    [ "${PURGE_CACHE}" -eq 1 ] && clean_args+=(--cache)
    "${SCRIPT_DIR}/../clean.sh" "${clean_args[@]}"
fi

# Check required host utilities
for tool in curl tar file find readelf cpio; do
    command -v "$tool" >/dev/null 2>&1 || {
        echo "ERROR: required command not found on host: $tool" >&2
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

mkdir -p "${CACHE_DIR}" "${BUILD_DIR}"

# ----------------------------------------------------------------------------
# 1. Fetch & Extract Rocky Linux Base Rootfs
# ----------------------------------------------------------------------------
ARCHIVE_PATH="${CACHE_DIR}/${ROCKY_TAR_NAME}"
if [ ! -f "${ARCHIVE_PATH}" ]; then
    echo "==> Fetching Rocky Linux ${ROCKY_VERSION} (${ROCKY_ARCH}) base image..."
    curl -fL --retry 3 "${ROCKY_URL}" -o "${ARCHIVE_PATH}"
fi

if [ "${VERIFY_CHECKSUM}" -eq 1 ] && [ -n "${ROCKY_SHA256}" ]; then
    actual_sha256="$(sha256_file "${ARCHIVE_PATH}")"
    if [ "${actual_sha256}" != "${ROCKY_SHA256}" ]; then
        echo "ERROR: Checksum mismatch for ${ARCHIVE_PATH}" >&2
        echo "  Expected: ${ROCKY_SHA256}" >&2
        echo "  Actual:   ${actual_sha256}" >&2
        echo "Use --no-verify to bypass if using an updated upstream release." >&2
        exit 1
    fi
fi

if [ ! -d "${ROOTFS_STAGING}/usr/bin" ] || [ "${FRESH}" -eq 1 ]; then
    echo "==> Extracting Rocky Linux rootfs into ${ROOTFS_STAGING}..."
    chmod -R u+rwX "${STAGING}" "${ROOTFS_STAGING}" 2>/dev/null || true
    rm -rf "${STAGING}" "${ROOTFS_STAGING}"
    mkdir -p "${STAGING}" "${ROOTFS_STAGING}"

    TMP_EXTRACT="$(mktemp -d "${BUILD_DIR}/.rocky_extract.XXXXXX")"
    tar -xJf "${ARCHIVE_PATH}" -C "${TMP_EXTRACT}"

    # Locate rootfs layer blob inside OCI container archive or direct rootfs tarball
    LAYER_BLOB=""
    if [ -d "${TMP_EXTRACT}/blobs/sha256" ]; then
        LAYER_BLOB="$(find "${TMP_EXTRACT}/blobs/sha256" -type f -size +10M | head -n 1)"
    fi

    if [ -n "${LAYER_BLOB}" ] && [ -f "${LAYER_BLOB}" ]; then
        tar -xzf "${LAYER_BLOB}" -C "${ROOTFS_STAGING}"
    else
        # Fallback: check if direct layer tar exists in archive
        DIRECT_LAYER="$(find "${TMP_EXTRACT}" -maxdepth 2 -name "*.tar" -o -name "layer.tar" | head -n 1)"
        if [ -n "${DIRECT_LAYER}" ] && [ -f "${DIRECT_LAYER}" ]; then
            tar -xf "${DIRECT_LAYER}" -C "${ROOTFS_STAGING}"
        else
            echo "ERROR: failed to locate rootfs layer inside ${ARCHIVE_PATH}" >&2
            chmod -R u+rwX "${TMP_EXTRACT}" 2>/dev/null || true
            rm -rf "${TMP_EXTRACT}"
            exit 1
        fi
    fi

    chmod -R u+rwX "${TMP_EXTRACT}" 2>/dev/null || true
    rm -rf "${TMP_EXTRACT}"
    chmod -R u+rwX "${ROOTFS_STAGING}" 2>/dev/null || true
fi

# Provision essential Rocky Linux packages (compilers, debugger, CLI utilities)
# into rootfs_staging so tools in /usr/share (gcc, g++, gdb, file, etc.) are runnable.
DEFAULT_ROOTFS_PACKAGES=(
    gcc gcc-c++ gdb make file which findutils diffutils
    procps-ng iproute less nano tar gzip xz bzip2 binutils
)
NEED_ROOTFS_PKGS=0
for bin_check in gcc g++ gdb make file which find diff ps ip less nano; do
    if [ ! -x "${ROOTFS_STAGING}/usr/bin/${bin_check}" ] && [ ! -x "${ROOTFS_STAGING}/usr/sbin/${bin_check}" ]; then
        NEED_ROOTFS_PKGS=1
        break
    fi
done

if [ "${NEED_ROOTFS_PKGS}" -eq 1 ] && [ -x "${ROOTFS_STAGING}/usr/bin/dnf" ] && command -v unshare >/dev/null 2>&1; then
    echo "==> Installing Rocky Linux toolchain & utilities into ${ROOTFS_STAGING}..."
    mkdir -p "${ROOTFS_STAGING}/etc" "${ROOTFS_STAGING}/var/cache/dnf" "${ROOTFS_STAGING}/tmp"
    if [ -f /etc/resolv.conf ]; then
        cp -L /etc/resolv.conf "${ROOTFS_STAGING}/etc/resolv.conf" 2>/dev/null || true
    fi
    if ! grep -q "nameserver" "${ROOTFS_STAGING}/etc/resolv.conf" 2>/dev/null; then
        cat > "${ROOTFS_STAGING}/etc/resolv.conf" << 'EOF'
nameserver 1.1.1.1
nameserver 8.8.8.8
EOF
    fi
    unshare -r chroot "${ROOTFS_STAGING}" /usr/bin/dnf install -y \
        --setopt=tsflags=nodocs \
        --setopt=install_weak_deps=False \
        "${DEFAULT_ROOTFS_PACKAGES[@]}"
    unshare -r chroot "${ROOTFS_STAGING}" /usr/bin/dnf clean all >/dev/null 2>&1 || true
    if [ -x "${ROOTFS_STAGING}/sbin/ldconfig" ] || [ -x "${ROOTFS_STAGING}/usr/sbin/ldconfig" ]; then
        unshare -r chroot "${ROOTFS_STAGING}" /sbin/ldconfig 2>/dev/null || true
    fi
    chmod -R u+rwX "${ROOTFS_STAGING}" 2>/dev/null || true
fi

# ----------------------------------------------------------------------------
# 2. Automated Dependency Resolution & Initramfs Assembly
# ----------------------------------------------------------------------------
echo "==> Assembling lightweight initramfs staging in ${STAGING}..."
rm -rf "${STAGING}"
mkdir -p "${STAGING}"/{usr/bin,usr/sbin,usr/lib64,usr/lib,dev/pts,dev/shm,proc,sys,tmp,run,root,mnt,sysroot,etc}
ln -sfn usr/bin "${STAGING}/bin"
ln -sfn usr/sbin "${STAGING}/sbin"
ln -sfn usr/lib64 "${STAGING}/lib64"
ln -sfn usr/lib "${STAGING}/lib"

# Recursively copy all shared library dependencies (DT_NEEDED) of an ELF file
copy_elf_deps() {
    local file="$1"
    [ -f "$file" ] || return 0

    local needed
    needed="$(readelf -d "$file" 2>/dev/null | awk -F'[][]' '/\(NEEDED\)/ {print $2}')"
    for lib in $needed; do
        if [ ! -e "${STAGING}/usr/lib64/${lib}" ] && [ ! -e "${STAGING}/usr/lib/${lib}" ]; then
            local found=""
            for search_dir in "${ROOTFS_STAGING}/usr/lib64" "${ROOTFS_STAGING}/usr/lib" "${ROOTFS_STAGING}/lib64" "${ROOTFS_STAGING}/lib"; do
                if [ -e "${search_dir}/${lib}" ]; then
                    found="${search_dir}/${lib}"
                    break
                fi
            done

            if [ -n "$found" ]; then
                local target_rel="${found#"${ROOTFS_STAGING}/"}"
                mkdir -p "${STAGING}/$(dirname "${target_rel}")"
                cp -a "$found" "${STAGING}/${target_rel}"

                if [ -L "$found" ]; then
                    local real_target
                    real_target="$(readlink -f "$found")"
                    if [ -e "$real_target" ]; then
                        local real_rel="${real_target#"${ROOTFS_STAGING}/"}"
                        mkdir -p "${STAGING}/$(dirname "${real_rel}")"
                        cp -a "$real_target" "${STAGING}/${real_rel}"
                        copy_elf_deps "$real_target"
                    fi
                else
                    copy_elf_deps "$found"
                fi
            fi
        fi
    done
}

# Stage a binary, resolving symlinks, shebang interpreters, and ELF dependencies
stage_tool() {
    local name="$1"
    local src=""
    for dir in "${ROOTFS_STAGING}/usr/bin" "${ROOTFS_STAGING}/usr/sbin" "${ROOTFS_STAGING}/bin" "${ROOTFS_STAGING}/sbin"; do
        if [ -e "${dir}/${name}" ]; then
            src="${dir}/${name}"
            break
        fi
    done

    [ -n "$src" ] || return 1

    local rel_path="${src#"${ROOTFS_STAGING}/"}"
    mkdir -p "${STAGING}/$(dirname "${rel_path}")"
    cp -a "$src" "${STAGING}/${rel_path}"

    if [ "$(dirname "${rel_path}")" = "usr/sbin" ] && [ ! -e "${STAGING}/usr/bin/${name}" ]; then
        ln -sfn "../sbin/${name}" "${STAGING}/usr/bin/${name}"
    fi

    # Handle symlinks
    if [ -L "$src" ]; then
        local real_target
        real_target="$(readlink -f "$src")"
        if [ -e "$real_target" ]; then
            local real_rel="${real_target#"${ROOTFS_STAGING}/"}"
            mkdir -p "${STAGING}/$(dirname "${real_rel}")"
            cp -a "$real_target" "${STAGING}/${real_rel}"
            src="$real_target"
        fi
    fi

    # Check file type
    local magic
    magic="$(head -c 4 "$src" 2>/dev/null || true)"
    if [ "${magic:0:2}" = "#!" ]; then
        local shebang interp interp_bin
        read -r shebang < "$src"
        interp="$(echo "${shebang#\#!}" | awk '{print $1}')"
        interp_bin="$(basename "$interp")"
        if [ -n "$interp_bin" ] && [ "$interp_bin" != "$name" ]; then
            stage_tool "$interp_bin" || true
        fi
    elif [ "$magic" = $'\x7fELF' ]; then
        copy_elf_deps "$src"
    fi

    return 0
}

# Standard essential tools for initramfs boot and recovery
DEFAULT_INITRAMFS_TOOLS=(
    # Core shell & multi-call coreutils
    bash sh coreutils
    # Filesystem & rootfs pivot
    mount umount switch_root pivot_root blkid
    # Core manipulation tools
    cat cp mv rm ln mkdir rmdir chmod chown touch
    # Text processing & searching
    echo head tail grep sed awk cut find file which
    # System info & process control
    uname sleep true false dmesg sync clear
    # Userspace transition
    chroot
)

ALL_TOOLS=("${DEFAULT_INITRAMFS_TOOLS[@]}")
if [ -n "${EXTRA_TOOLS}" ]; then
    read -r -a EXTRA_TOOLS_ARR <<< "${EXTRA_TOOLS}"
    ALL_TOOLS+=("${EXTRA_TOOLS_ARR[@]}")
fi

echo "==> Staging tools and resolving dynamic dependencies:"
for tool in "${ALL_TOOLS[@]}"; do
    if stage_tool "$tool"; then
        printf "    staged: %s\n" "$tool"
    else
        printf "    (skipped, not found): %s\n" "$tool"
    fi
done

# Dynamic Linker
if [ -e "${ROOTFS_STAGING}/usr/lib64/ld-linux-x86-64.so.2" ]; then
    cp -a "${ROOTFS_STAGING}/usr/lib64/ld-linux-x86-64.so.2" "${STAGING}/usr/lib64/"
    if [ -L "${ROOTFS_STAGING}/usr/lib64/ld-linux-x86-64.so.2" ]; then
        real_loader="$(readlink -f "${ROOTFS_STAGING}/usr/lib64/ld-linux-x86-64.so.2")"
        cp -a "$real_loader" "${STAGING}/usr/lib64/"
    fi
fi

# Fallback POSIX which implementation if which is not a binary
if [ ! -e "${STAGING}/usr/bin/which" ]; then
    cat > "${STAGING}/usr/bin/which" << 'EOF'
#!/bin/sh
command -v "$@"
EOF
    chmod 0755 "${STAGING}/usr/bin/which"
fi

# Ensure sh points to bash
[ -e "${STAGING}/usr/bin/sh" ] || ln -sfn bash "${STAGING}/usr/bin/sh"

# Terminfo data for interactive terminals (linux, vt100, xterm)
for term in l/linux v/vt100 v/vt220 x/xterm x/xterm-256color; do
    if [ -f "${ROOTFS_STAGING}/usr/share/terminfo/${term}" ]; then
        mkdir -p "${STAGING}/usr/share/terminfo/$(dirname "${term}")"
        cp -a "${ROOTFS_STAGING}/usr/share/terminfo/${term}" "${STAGING}/usr/share/terminfo/${term}"
    fi
done

# Locale support for glibc tools (prevent setlocale warnings)
if [ -d "${ROOTFS_STAGING}/usr/lib/locale" ]; then
    cp -a "${ROOTFS_STAGING}/usr/lib/locale" "${STAGING}/usr/lib/"
fi
if [ -d "${ROOTFS_STAGING}/usr/share/locale" ]; then
    mkdir -p "${STAGING}/usr/share"
    cp -a "${ROOTFS_STAGING}/usr/share/locale" "${STAGING}/usr/share/"
fi

# ----------------------------------------------------------------------------
# 3. Environment & OS Configuration
# ----------------------------------------------------------------------------
echo "==> Configuring system files in initramfs and rootfs..."

# Ensure root shell is /bin/bash in rootfs
if [ -f "${ROOTFS_STAGING}/etc/passwd" ]; then
    sed -i 's|^root:.*|root:x:0:0:root:/root:/bin/bash|' "${ROOTFS_STAGING}/etc/passwd"
fi

# Copy user/group definitions from rootfs to initramfs
for f in passwd group shells; do
    if [ -f "${ROOTFS_STAGING}/etc/${f}" ]; then
        cp -a "${ROOTFS_STAGING}/etc/${f}" "${STAGING}/etc/${f}"
    fi
done

for target_dir in "${STAGING}" "${ROOTFS_STAGING}"; do
    chmod -R u+w "${target_dir}" 2>/dev/null || true
    mkdir -p "${target_dir}"/{dev/pts,dev/shm,proc,sys,tmp,run,root,mnt,media,var/log,var/run,var/tmp}
    chmod 1777 "${target_dir}/tmp" "${target_dir}/var/tmp"
    chmod 0750 "${target_dir}/root"
    install -m0755 "${SUPPORT_DIR}/init-amd64/init" "${target_dir}/init"

    # Locale aliases
    if [ -d "${target_dir}/usr/lib/locale/C.utf8" ]; then
        ln -sfn C.utf8 "${target_dir}/usr/lib/locale/C.UTF-8"
    fi

    # Convenience aliases
    if [ -x "${target_dir}/usr/bin/vi" ] && [ ! -e "${target_dir}/usr/bin/vim" ]; then
        ln -sfn vi "${target_dir}/usr/bin/vim"
    fi
    if [ -x "${target_dir}/usr/bin/python3" ] && [ ! -e "${target_dir}/usr/bin/python" ]; then
        ln -sfn python3 "${target_dir}/usr/bin/python"
    fi
    if [ -x "${target_dir}/usr/bin/dnf-3" ] && [ ! -e "${target_dir}/usr/bin/dnf" ]; then
        ln -sfn dnf-3 "${target_dir}/usr/bin/dnf"
    fi
    if [ -x "${target_dir}/usr/bin/dnf-3" ] && [ ! -e "${target_dir}/usr/bin/yum" ]; then
        ln -sfn dnf-3 "${target_dir}/usr/bin/yum"
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
LANG=C.utf8
LC_ALL=C.utf8
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
export PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin:/usr/libexec
export HOME=/root
export USER=root
export LOGNAME=root
export SHELL=/bin/bash
export TERM=linux
export XDG_CONFIG_HOME=/root/.config
export CHARSET=UTF-8
export LANG=C.utf8
export LC_ALL=C.utf8
export PS1='petraos:\w# '
alias ll='ls -la --color=auto'
alias l='ls -lh'
alias clear='printf "\033[H\033[2J\033[3J"'
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
export PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin:/usr/libexec
export SHELL=/bin/bash
export TERM=linux
export HOME=/root
export USER=root
export LOGNAME=root
export XDG_CONFIG_HOME=/root/.config
export CHARSET=UTF-8
export LANG=C.utf8
export LC_ALL=C.utf8
alias ll='ls -la --color=auto'
alias l='ls -lh'
alias clear='printf "\033[H\033[2J\033[3J"'
EOF

    cat > "${target_dir}/root/.bash_profile" << 'EOF'
if [ -f ~/.bashrc ]; then
    . ~/.bashrc
fi
export PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin:/usr/libexec
export PS1='petraos:\w# '
EOF

    cat > "${target_dir}/root/.bashrc" << 'EOF'
if [ -f /etc/bashrc ]; then
    . /etc/bashrc
fi
export PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin:/usr/libexec
export HOME=/root
export USER=root
export LOGNAME=root
export SHELL=/bin/bash
export TERM=linux
export CHARSET=UTF-8
export LANG=C.utf8
export LC_ALL=C.utf8
export PS1='petraos:\w# '
alias ll='ls -la --color=auto'
alias l='ls -lh'
alias clear='printf "\033[H\033[2J\033[3J"'
EOF

    # Best-effort device nodes
    mknod -m 666 "${target_dir}/dev/null" c 1 3 2>/dev/null || true
    mknod -m 666 "${target_dir}/dev/zero" c 1 5 2>/dev/null || true
    mknod -m 666 "${target_dir}/dev/tty" c 5 0 2>/dev/null || true
    mknod -m 600 "${target_dir}/dev/console" c 5 1 2>/dev/null || true
    mknod -m 666 "${target_dir}/dev/ptmx" c 5 2 2>/dev/null || true
done

# Verification
for path in "${STAGING}/usr/bin/bash" "${STAGING}/usr/lib64/ld-linux-x86-64.so.2" "${STAGING}/bin/sh" "${STAGING}/init"; do
    if [ ! -e "$path" ]; then
        echo "ERROR: Initramfs staging is incomplete, missing ${path}" >&2
        exit 1
    fi
done

echo "==> Packaging the initramfs and disk images..."
"${SCRIPT_DIR}/../build-initramfs.sh" --staging "${STAGING}" --rootfs-staging "${ROOTFS_STAGING}" --output "${INITRAMFS}"

echo "==> Userland build completed successfully."
