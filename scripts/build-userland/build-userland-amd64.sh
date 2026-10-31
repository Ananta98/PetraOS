#!/bin/bash
# ==============================================================================
# PetraOS amd64 Userland & Initramfs Builder (Rocky Linux 9)
# ==============================================================================
# Assembles the PetraOS x86_64 userland from the official Rocky Linux 9 base
# image, provisions standard toolchain packages via DNF, resolves ELF dynamic
# library dependencies for a lightweight initramfs, and packs initramfs.cpio
# and ext4.img.
# ==============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
SUPPORT_DIR="${REPO_ROOT}/scripts/build-support"

# Rocky Linux upstream configuration
ROCKY_VERSION="${ROCKY_VERSION:-9}"
ROCKY_ARCH="${ROCKY_ARCH:-x86_64}"
ROCKY_SHA256="${ROCKY_SHA256:-1210df99dcf0ef4d73940244e6d703935175629598b09c0e430da20e76768402}"
ROCKY_TAR_NAME="Rocky-${ROCKY_VERSION}-Container-Base.latest.${ROCKY_ARCH}.tar.xz"
ROCKY_URL="${ROCKY_URL:-https://dl.rockylinux.org/pub/rocky/${ROCKY_VERSION}/images/${ROCKY_ARCH}/${ROCKY_TAR_NAME}}"

# Build & staging paths
CACHE_DIR="${REPO_ROOT}/build/cache"
BUILD_DIR="${REPO_ROOT}/build/userland-amd64"
STAGING="${BUILD_DIR}/staging"
ROOTFS_STAGING="${BUILD_DIR}/rootfs_staging"
INITRAMFS="${REPO_ROOT}/build/initramfs.cpio"

PETRAOS_VERSION="${PETRAOS_VERSION:-0.1.0}"
DEFAULT_PATH="/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin:/usr/libexec"

FRESH=0
PURGE_CACHE=0
VERIFY_CHECKSUM=1
EXTRA_TOOLS="${EXTRA_INITRAMFS_TOOLS:-}"

# Packages provisioned into the full Rocky Linux rootfs (ext4.img)
DEFAULT_ROOTFS_PACKAGES=(
    gcc gcc-c++ gdb make binutils
    file which findutils diffutils
    procps-ng iproute less nano
    tar gzip xz bzip2
)

# Essential binaries staged into the in-memory boot initramfs (initramfs.cpio)
DEFAULT_INITRAMFS_TOOLS=(
    bash sh coreutils
    mount umount switch_root pivot_root blkid chroot
    cat cp mv rm ln mkdir rmdir chmod chown touch
    echo head tail grep sed awk cut find file which
    uname sleep true false dmesg sync
)

usage() {
    cat << EOF
Usage: $(basename "$0") [OPTIONS]

Build the PetraOS amd64 userland and initramfs from Rocky Linux ${ROCKY_VERSION}.

Options:
  --fresh          Remove existing staging directories and rebuild from base
  --purge-cache    With --fresh, also delete the cached Rocky Linux tarball
  --no-verify      Skip SHA256 checksum verification of the base archive
  --tools "TOOLS"  Space-separated list of extra binaries to stage into initramfs
  -h, --help       Show this help message
EOF
    exit 0
}

parse_args() {
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
}

check_host_dependencies() {
    for tool in curl tar file find readelf cpio; do
        command -v "$tool" >/dev/null 2>&1 || {
            echo "ERROR: required host utility not found: $tool" >&2
            exit 1
        }
    done
}

sha256_file() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    else
        shasum -a 256 "$1" | awk '{print $1}'
    fi
}

fetch_rocky_archive() {
    local archive_path="${CACHE_DIR}/${ROCKY_TAR_NAME}"
    mkdir -p "${CACHE_DIR}" "${BUILD_DIR}"

    if [ ! -f "${archive_path}" ]; then
        echo "==> Downloading Rocky Linux ${ROCKY_VERSION} (${ROCKY_ARCH}) base archive..."
        curl -fL --retry 3 "${ROCKY_URL}" -o "${archive_path}"
    fi

    if [ "${VERIFY_CHECKSUM}" -eq 1 ] && [ -n "${ROCKY_SHA256}" ]; then
        local actual_sha256
        actual_sha256="$(sha256_file "${archive_path}")"
        if [ "${actual_sha256}" != "${ROCKY_SHA256}" ]; then
            echo "ERROR: SHA256 mismatch for ${archive_path}" >&2
            echo "  Expected: ${ROCKY_SHA256}" >&2
            echo "  Actual:   ${actual_sha256}" >&2
            echo "Pass --no-verify if using a newer Rocky Linux image." >&2
            exit 1
        fi
    fi
}

extract_rocky_rootfs() {
    local archive_path="${CACHE_DIR}/${ROCKY_TAR_NAME}"
    if [ -d "${ROOTFS_STAGING}/usr/bin" ] && [ "${FRESH}" -eq 0 ]; then
        return 0
    fi

    echo "==> Extracting Rocky Linux rootfs into ${ROOTFS_STAGING}..."
    chmod -R u+rwX "${STAGING}" "${ROOTFS_STAGING}" 2>/dev/null || true
    rm -rf "${STAGING}" "${ROOTFS_STAGING}"
    mkdir -p "${STAGING}" "${ROOTFS_STAGING}"

    local tmp_extract
    tmp_extract="$(mktemp -d "${BUILD_DIR}/.rocky_extract.XXXXXX")"
    tar -xJf "${archive_path}" -C "${tmp_extract}"

    local layer_blob=""
    if [ -d "${tmp_extract}/blobs/sha256" ]; then
        layer_blob="$(find "${tmp_extract}/blobs/sha256" -type f -size +10M | head -n 1)"
    fi

    if [ -n "${layer_blob}" ] && [ -f "${layer_blob}" ]; then
        tar -xzf "${layer_blob}" -C "${ROOTFS_STAGING}"
    else
        local direct_layer
        direct_layer="$(find "${tmp_extract}" -maxdepth 2 \( -name "*.tar" -o -name "layer.tar" \) | head -n 1)"
        if [ -n "${direct_layer}" ] && [ -f "${direct_layer}" ]; then
            tar -xf "${direct_layer}" -C "${ROOTFS_STAGING}"
        else
            echo "ERROR: unable to find rootfs tar layer inside ${archive_path}" >&2
            chmod -R u+rwX "${tmp_extract}" 2>/dev/null || true
            rm -rf "${tmp_extract}"
            exit 1
        fi
    fi

    chmod -R u+rwX "${tmp_extract}" 2>/dev/null || true
    rm -rf "${tmp_extract}"
    chmod -R u+rwX "${ROOTFS_STAGING}" 2>/dev/null || true
}

install_rootfs_packages() {
    local need_install=0
    for bin_name in gcc g++ gdb make file which find diff ps ip less nano; do
        if [ ! -x "${ROOTFS_STAGING}/usr/bin/${bin_name}" ] && [ ! -x "${ROOTFS_STAGING}/usr/sbin/${bin_name}" ]; then
            need_install=1
            break
        fi
    done

    if [ "${need_install}" -eq 0 ]; then
        return 0
    fi

    if [ ! -x "${ROOTFS_STAGING}/usr/bin/dnf" ] || ! command -v unshare >/dev/null 2>&1; then
        echo "WARNING: skipping DNF package installation (dnf or unshare unavailable)." >&2
        return 0
    fi

    echo "==> Provisioning Rocky Linux toolchain and utilities via DNF..."
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
    if [ -x "${ROOTFS_STAGING}/usr/sbin/ldconfig" ]; then
        unshare -r chroot "${ROOTFS_STAGING}" /usr/sbin/ldconfig 2>/dev/null || true
    fi
    chmod -R u+rwX "${ROOTFS_STAGING}" 2>/dev/null || true
}

# Recursively copy all shared library dependencies (DT_NEEDED) of an ELF binary
copy_elf_deps() {
    local elf_file="$1"
    [ -f "${elf_file}" ] || return 0

    local needed_libs
    needed_libs="$(readelf -d "${elf_file}" 2>/dev/null | awk -F'[][]' '/\(NEEDED\)/ {print $2}')"

    for lib in ${needed_libs}; do
        if [ -e "${STAGING}/usr/lib64/${lib}" ] || [ -e "${STAGING}/usr/lib/${lib}" ]; then
            continue
        fi

        local found_lib=""
        for lib_dir in "${ROOTFS_STAGING}/usr/lib64" "${ROOTFS_STAGING}/usr/lib"; do
            if [ -e "${lib_dir}/${lib}" ]; then
                found_lib="${lib_dir}/${lib}"
                break
            fi
        done

        [ -n "${found_lib}" ] || continue

        local rel_lib="${found_lib#"${ROOTFS_STAGING}/"}"
        mkdir -p "${STAGING}/$(dirname "${rel_lib}")"
        cp -a "${found_lib}" "${STAGING}/${rel_lib}"

        if [ -L "${found_lib}" ]; then
            local real_lib
            real_lib="$(readlink -f "${found_lib}")"
            if [ -e "${real_lib}" ]; then
                local real_rel="${real_lib#"${ROOTFS_STAGING}/"}"
                mkdir -p "${STAGING}/$(dirname "${real_rel}")"
                cp -a "${real_lib}" "${STAGING}/${real_rel}"
                copy_elf_deps "${real_lib}"
            fi
        else
            copy_elf_deps "${found_lib}"
        fi
    done
}

# Copy a tool from rootfs_staging into initramfs staging along with its interpreter/libraries
stage_tool() {
    local name="$1"
    local src=""

    for bin_dir in "${ROOTFS_STAGING}/usr/bin" "${ROOTFS_STAGING}/usr/sbin"; do
        if [ -e "${bin_dir}/${name}" ]; then
            src="${bin_dir}/${name}"
            break
        fi
    done

    [ -n "${src}" ] || return 1

    local rel_path="${src#"${ROOTFS_STAGING}/"}"
    mkdir -p "${STAGING}/$(dirname "${rel_path}")"
    cp -a "${src}" "${STAGING}/${rel_path}"

    if [ "$(dirname "${rel_path}")" = "usr/sbin" ] && [ ! -e "${STAGING}/usr/bin/${name}" ]; then
        ln -sfn "../sbin/${name}" "${STAGING}/usr/bin/${name}"
    fi

    if [ -L "${src}" ]; then
        local real_target
        real_target="$(readlink -f "${src}")"
        if [ -e "${real_target}" ]; then
            local real_rel="${real_target#"${ROOTFS_STAGING}/"}"
            mkdir -p "${STAGING}/$(dirname "${real_rel}")"
            cp -a "${real_target}" "${STAGING}/${real_rel}"
            src="${real_target}"
        fi
    fi

    local magic
    magic="$(head -c 4 "${src}" 2>/dev/null || true)"
    if [ "${magic:0:2}" = "#!" ]; then
        local shebang interp interp_bin
        read -r shebang < "${src}"
        interp="$(echo "${shebang#\#!}" | awk '{print $1}')"
        interp_bin="$(basename "${interp}")"
        if [ -n "${interp_bin}" ] && [ "${interp_bin}" != "${name}" ]; then
            stage_tool "${interp_bin}" || true
        fi
    elif [ "${magic}" = $'\x7fELF' ]; then
        copy_elf_deps "${src}"
    fi

    return 0
}

assemble_initramfs() {
    echo "==> Assembling initramfs staging directory in ${STAGING}..."
    rm -rf "${STAGING}"
    mkdir -p "${STAGING}"/{usr/bin,usr/sbin,usr/lib64,usr/lib,dev/pts,dev/shm,proc,sys,tmp,run,root,mnt,sysroot,etc}

    # Rocky Linux UsrMerge top-level symlinks
    ln -sfn usr/bin "${STAGING}/bin"
    ln -sfn usr/sbin "${STAGING}/sbin"
    ln -sfn usr/lib64 "${STAGING}/lib64"
    ln -sfn usr/lib "${STAGING}/lib"

    local all_tools=("${DEFAULT_INITRAMFS_TOOLS[@]}")
    if [ -n "${EXTRA_TOOLS}" ]; then
        local extra_arr=()
        read -r -a extra_arr <<< "${EXTRA_TOOLS}"
        all_tools+=("${extra_arr[@]}")
    fi

    echo "==> Staging initramfs tools and resolving ELF dependencies:"
    for tool in "${all_tools[@]}"; do
        if stage_tool "${tool}"; then
            printf "    staged: %s\n" "${tool}"
        else
            printf "    (skipped, not found): %s\n" "${tool}"
        fi
    done

    # Stage glibc dynamic linker (ld-linux-x86-64.so.2)
    if [ -e "${ROOTFS_STAGING}/usr/lib64/ld-linux-x86-64.so.2" ]; then
        cp -a "${ROOTFS_STAGING}/usr/lib64/ld-linux-x86-64.so.2" "${STAGING}/usr/lib64/"
        if [ -L "${ROOTFS_STAGING}/usr/lib64/ld-linux-x86-64.so.2" ]; then
            local real_loader
            real_loader="$(readlink -f "${ROOTFS_STAGING}/usr/lib64/ld-linux-x86-64.so.2")"
            cp -a "${real_loader}" "${STAGING}/usr/lib64/"
        fi
    fi

    [ -e "${STAGING}/usr/bin/sh" ] || ln -sfn bash "${STAGING}/usr/bin/sh"

    # Copy terminal capabilities (terminfo)
    for term in l/linux v/vt100 v/vt220 x/xterm x/xterm-256color; do
        if [ -f "${ROOTFS_STAGING}/usr/share/terminfo/${term}" ]; then
            mkdir -p "${STAGING}/usr/share/terminfo/$(dirname "${term}")"
            cp -a "${ROOTFS_STAGING}/usr/share/terminfo/${term}" "${STAGING}/usr/share/terminfo/${term}"
        fi
    done

    # Copy C.utf8 locale definitions
    if [ -d "${ROOTFS_STAGING}/usr/lib/locale" ]; then
        cp -a "${ROOTFS_STAGING}/usr/lib/locale" "${STAGING}/usr/lib/"
    fi
}

configure_target_tree() {
    local target_dir="$1"

    chmod -R u+w "${target_dir}" 2>/dev/null || true
    mkdir -p "${target_dir}"/{dev/pts,dev/shm,proc,sys,tmp,run,root,mnt,media,var/log,var/run,var/tmp,etc/fastfetch,root/.config/fastfetch}
    chmod 1777 "${target_dir}/tmp" "${target_dir}/var/tmp"
    chmod 0750 "${target_dir}/root"

    install -m 0755 "${SUPPORT_DIR}/init-amd64/init" "${target_dir}/init"

    if [ -f "${SUPPORT_DIR}/petraos-logo.txt" ]; then
        install -m 0644 "${SUPPORT_DIR}/petraos-logo.txt" "${target_dir}/etc/fastfetch/petraos.txt"
    fi
    if [ -f "${SUPPORT_DIR}/fastfetch.jsonc" ]; then
        install -m 0644 "${SUPPORT_DIR}/fastfetch.jsonc" "${target_dir}/root/.config/fastfetch/config.jsonc"
    fi

    if [ -d "${target_dir}/usr/lib/locale/C.utf8" ]; then
        ln -sfn C.utf8 "${target_dir}/usr/lib/locale/C.UTF-8"
    fi

    # Standard command symlinks
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

    cat > "${target_dir}/etc/hosts" << 'EOF'
127.0.0.1 localhost petraos
::1       localhost ip6-localhost ip6-loopback
EOF

    cat > "${target_dir}/etc/resolv.conf" << 'EOF'
nameserver 10.0.2.3
nameserver 1.1.1.1
nameserver 8.8.8.8
EOF

    cat > "${target_dir}/etc/gai.conf" << 'EOF'
# Prefer IPv4 over IPv6 in PetraOS
precedence ::ffff:0:0/96 100
scopev4 ::ffff:0.0.0.0/96 14
EOF

    printf 'petraos\n' > "${target_dir}/etc/hostname"
    printf '%s\n' "${PETRAOS_VERSION}" > "${target_dir}/etc/petraos-release"

    cat > "${target_dir}/etc/environment" << 'EOF'
LANG=C.utf8
LC_ALL=C.utf8
EOF

    mkdir -p "${target_dir}/usr/lib"
    cat > "${target_dir}/usr/lib/os-release" << EOF
NAME="PetraOS"
PRETTY_NAME="PetraOS ${PETRAOS_VERSION}"
ID=petraos
ID_LIKE="rocky rhel fedora"
VERSION_ID="${PETRAOS_VERSION}"
VERSION="${PETRAOS_VERSION}"
EOF
    rm -f "${target_dir}/etc/os-release"
    ln -sf ../usr/lib/os-release "${target_dir}/etc/os-release"

    # Remove unneeded debuginfod startup scripts and configurations
    rm -f "${target_dir}"/etc/profile.d/debuginfod.*
    rm -rf "${target_dir}/etc/debuginfod"

    cat > "${target_dir}/etc/profile" << EOF
export PATH=${DEFAULT_PATH}
export HOME=/root
export USER=root
export LOGNAME=root
export SHELL=/bin/bash
export TERM=linux
export XDG_CONFIG_HOME=/root/.config
export LANG=C.utf8
export LC_ALL=C.utf8
export PS1='petraos:\w# '
alias ll='ls -la --color=auto'
alias l='ls -lh'
alias clear='printf "\033[H\033[2J\033[3J"'
umask 022

for script in /etc/profile.d/*.sh ; do
    if [ -r "\$script" ] ; then
        . "\$script"
    fi
done
unset script
EOF

    cat > "${target_dir}/root/.profile" << EOF
export PATH=${DEFAULT_PATH}
export HOME=/root
export USER=root
export LOGNAME=root
export SHELL=/bin/bash
export TERM=linux
export XDG_CONFIG_HOME=/root/.config
export LANG=C.utf8
export LC_ALL=C.utf8
export PS1='petraos:\w# '
alias ll='ls -la --color=auto'
alias l='ls -lh'
alias clear='printf "\033[H\033[2J\033[3J"'
EOF

    cat > "${target_dir}/root/.bash_profile" << EOF
if [ -f ~/.bashrc ]; then
    . ~/.bashrc
fi
export PATH=${DEFAULT_PATH}
export PS1='petraos:\w# '
EOF

    cat > "${target_dir}/root/.bashrc" << EOF
if [ -f /etc/bashrc ]; then
    . /etc/bashrc
fi
export PATH=${DEFAULT_PATH}
export HOME=/root
export USER=root
export LOGNAME=root
export SHELL=/bin/bash
export TERM=linux
export LANG=C.utf8
export LC_ALL=C.utf8
export PS1='petraos:\w# '
alias ll='ls -la --color=auto'
alias l='ls -lh'
alias clear='printf "\033[H\033[2J\033[3J"'
EOF
}

configure_system_files() {
    echo "==> Configuring system files in initramfs and rootfs..."

    if [ -f "${ROOTFS_STAGING}/etc/passwd" ]; then
        sed -i 's|^root:.*|root:x:0:0:root:/root:/bin/bash|' "${ROOTFS_STAGING}/etc/passwd"
    fi

    for account_file in passwd group shells; do
        if [ -f "${ROOTFS_STAGING}/etc/${account_file}" ]; then
            cp -a "${ROOTFS_STAGING}/etc/${account_file}" "${STAGING}/etc/${account_file}"
        fi
    done

    configure_target_tree "${STAGING}"
    configure_target_tree "${ROOTFS_STAGING}"
}

verify_staging() {
    for required_path in \
        "${STAGING}/usr/bin/bash" \
        "${STAGING}/usr/lib64/ld-linux-x86-64.so.2" \
        "${STAGING}/bin/sh" \
        "${STAGING}/init"; do
        if [ ! -e "${required_path}" ]; then
            echo "ERROR: initramfs staging is missing required file: ${required_path}" >&2
            exit 1
        fi
    done
}

main() {
    parse_args "$@"

    if [ "${FRESH}" -eq 1 ]; then
        local clean_args=(--userland)
        [ "${PURGE_CACHE}" -eq 1 ] && clean_args+=(--cache)
        "${SCRIPT_DIR}/../clean.sh" "${clean_args[@]}"
    fi

    check_host_dependencies
    fetch_rocky_archive
    extract_rocky_rootfs
    install_rootfs_packages
    assemble_initramfs
    configure_system_files
    verify_staging

    echo "==> Packaging initramfs and disk images..."
    "${SCRIPT_DIR}/../build-initramfs.sh" \
        --staging "${STAGING}" \
        --rootfs-staging "${ROOTFS_STAGING}" \
        --output "${INITRAMFS}"

    echo "==> Userland build completed successfully."
}

main "$@"
