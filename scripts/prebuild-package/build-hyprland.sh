#!/bin/bash
# Stage the PetraOS Hyprland + Quickshell desktop layer into the Void Linux
# glibc target rootfs using xbps-install.static and a cached Void-native
# Hyprland 0.54.3 + Aquamarine 0.12.0 bundle.
#
# Usage: build-hyprland.sh [--root <target_root>]
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

VOID_MIRROR="${VOID_MIRROR:-https://repo-default.voidlinux.org/current}"
VOID_VERSION="${VOID_VERSION:-20250202}"
VOID_ARCH="x86_64"
VOID_TAR_NAME="void-${VOID_ARCH}-ROOTFS-${VOID_VERSION}.tar.xz"
XBPS_STATIC_URL="https://repo-default.voidlinux.org/static/xbps-static-latest.x86_64-musl.tar.xz"

CACHE_DIR="${REPO_ROOT}/build/cache"
BUILD_DIR="${REPO_ROOT}/build/prebuild-package/hyprland"
SYSROOT="${BUILD_DIR}/sysroot"
STAGING="${BUILD_DIR}/staging"
BUNDLE_TAR="${CACHE_DIR}/hyprland-0.54.3-void-bundle.tar.xz"

XBPS_STATIC_DIR="${CACHE_DIR}/xbps_static"
XBPS_INSTALL="${XBPS_STATIC_DIR}/usr/bin/xbps-install.static"
NPROC="${NPROC:-$(getconf _NPROCESSORS_ONLN 2>/dev/null || echo 4)}"

TARGET_ROOT="${STAGING}"

while [[ $# -gt 0 ]]; do
    case "$1" in
        --root)
            TARGET_ROOT="$2"
            shift 2
            ;;
        -h|--help)
            echo "Usage: $0 [--root <dir>]"
            exit 0
            ;;
        *)
            echo "ERROR: unknown option: $1" >&2
            exit 1
            ;;
    esac
done

bootstrap_xbps_static() {
    if [ -x "${XBPS_INSTALL}" ]; then
        return
    fi
    local archive="${CACHE_DIR}/xbps-static-latest.x86_64-musl.tar.xz"
    echo "==> Fetching xbps-static-latest"
    if [ ! -s "${archive}" ]; then
        curl -fsSL --retry 3 "${XBPS_STATIC_URL}" -o "${archive}"
    fi
    mkdir -p "${XBPS_STATIC_DIR}"
    tar -xJf "${archive}" -C "${XBPS_STATIC_DIR}"
    chmod +x "${XBPS_STATIC_DIR}"/usr/bin/* || true
}

build_hyprland_bundle() {
    if [ -f "${BUNDLE_TAR}" ]; then
        echo "==> Using cached Hyprland bundle: ${BUNDLE_TAR}"
        return
    fi

    echo "==> Building Void-native Hyprland 0.54.3 + Aquamarine 0.12.0 bundle"
    rm -rf "${SYSROOT}"
    mkdir -p "${SYSROOT}"
    tar -xf "${CACHE_DIR}/${VOID_TAR_NAME}" -C "${SYSROOT}"
    chmod -R u+w "${SYSROOT}" 2>/dev/null || true

    mkdir -p "${SYSROOT}/var/db/xbps/keys" "${CACHE_DIR}/xbps-pkgs" "${SYSROOT}/tmp/src" "${SYSROOT}/opt/hypr-bundle"
    cp -a "${XBPS_STATIC_DIR}/var/db/xbps/keys/." "${SYSROOT}/var/db/xbps/keys/"

    local BUILD_PKGS=(
        base-devel clang cmake ninja meson pkgconf git curl tar xz python3
        hyprutils hyprwayland-scanner
        libdisplay-info-devel libjxl-devel MesaLib-devel
        wayland-devel wayland-protocols
        cairo-devel pango-devel pixman-devel
        libinput-devel libxkbcommon-devel libseat-devel
        libliftoff-devel re2-devel
        glslang-devel libuuid-devel
        libxcb-devel xcb-util-wm-devel xcb-util-errors-devel xcb-util-keysyms-devel
        libXcursor-devel udis86-devel
        pugixml-devel hwids eudev-libudev-devel
        libzip-devel librsvg-devel file-devel muparser-devel
    )

    "${XBPS_INSTALL}" \
        -r "${SYSROOT}" \
        -R "${VOID_MIRROR}" \
        -c "${CACHE_DIR}/xbps-pkgs" \
        -S -u -y --ignore-conf-repos \
        xbps || true

    "${XBPS_INSTALL}" \
        -r "${SYSROOT}" \
        -R "${VOID_MIRROR}" \
        -c "${CACHE_DIR}/xbps-pkgs" \
        -S -u -y --ignore-conf-repos || true

    "${XBPS_INSTALL}" \
        -r "${SYSROOT}" \
        -R "${VOID_MIRROR}" \
        -c "${CACHE_DIR}/xbps-pkgs" \
        -S -u -y --ignore-conf-repos \
        "${BUILD_PKGS[@]}"

    local SRC_CACHE="${CACHE_DIR}/hypr-src"
    mkdir -p "${SRC_CACHE}"

    fetch_src() {
        local name="$1" url="$2"
        local archive="${SRC_CACHE}/${name}.tar.gz"
        if [ ! -s "${archive}" ]; then
            echo "    Fetching ${name}..."
            curl -fL --retry 3 -o "${archive}" "${url}"
        fi
        tar -xzf "${archive}" -C "${SYSROOT}/tmp/src"
    }

    fetch_src "tomlplusplus-3.4.0" "https://github.com/marzer/tomlplusplus/archive/refs/tags/v3.4.0.tar.gz"
    fetch_src "glaze-6.0.0" "https://github.com/stephenberry/glaze/archive/refs/tags/v6.0.0.tar.gz"
    fetch_src "hyprlang-0.6.8" "https://github.com/hyprwm/hyprlang/archive/refs/tags/v0.6.8.tar.gz"
    fetch_src "hyprcursor-0.1.14" "https://github.com/hyprwm/hyprcursor/archive/refs/tags/v0.1.14.tar.gz"
    fetch_src "hyprgraphics-0.5.0" "https://github.com/hyprwm/hyprgraphics/archive/refs/tags/v0.5.0.tar.gz"
    fetch_src "hyprland-protocols-0.7.0" "https://github.com/hyprwm/hyprland-protocols/archive/refs/tags/v0.7.0.tar.gz"
    fetch_src "hyprwire-0.3.1" "https://github.com/hyprwm/hyprwire/archive/refs/tags/v0.3.1.tar.gz"
    fetch_src "aquamarine-0.12.0" "https://github.com/hyprwm/aquamarine/archive/refs/tags/v0.12.0.tar.gz"
    fetch_src "Hyprland-0.54.3" "https://github.com/hyprwm/Hyprland/archive/refs/tags/v0.54.3.tar.gz"

    cat > "${SYSROOT}/tmp/build-inside-chroot.sh" << 'CHROOT_EOF'
#!/bin/bash
set -euo pipefail
export PATH=/usr/bin:/bin:/usr/sbin:/sbin
export PKG_CONFIG_PATH=/opt/hypr-bundle/usr/lib/pkgconfig:/opt/hypr-bundle/usr/share/pkgconfig:/usr/lib/pkgconfig:/usr/share/pkgconfig
export CMAKE_PREFIX_PATH=/opt/hypr-bundle/usr:/usr

NPROC="${NPROC:-4}"

build_cmake() {
    local dir="$1"
    shift
    echo "==> Building ${dir}"
    cmake -S "/tmp/src/${dir}" -B "/tmp/src/${dir}/build" -G Ninja \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_INSTALL_PREFIX=/usr \
        -DCMAKE_INSTALL_LIBDIR=lib \
        "$@"
    cmake --build "/tmp/src/${dir}/build" -j "${NPROC}"
    cmake --install "/tmp/src/${dir}/build"
    DESTDIR=/opt/hypr-bundle cmake --install "/tmp/src/${dir}/build"
}

build_cmake "tomlplusplus-3.4.0" -DBUILD_TESTS=OFF
build_cmake "glaze-6.0.0" -Dglaze_DEVELOPER_MODE=OFF -DBUILD_TESTING=OFF
mkdir -p /usr/lib/pkgconfig
cat > /usr/lib/pkgconfig/tomlplusplus.pc << 'PC_EOF'
prefix=/usr
includedir=${prefix}/include
Name: tomlplusplus
Description: Header-only TOML config file parser and serializer for C++17
Version: 3.4.0
Cflags: -I${includedir}
PC_EOF
build_cmake "hyprlang-0.6.8" -DBUILD_TESTING=OFF
build_cmake "hyprcursor-0.1.14" -DBUILD_TESTING=OFF
build_cmake "hyprgraphics-0.5.0" -DBUILD_TESTING=OFF
echo "==> Building hyprland-protocols-0.7.0"
meson setup "/tmp/src/hyprland-protocols-0.7.0/build" "/tmp/src/hyprland-protocols-0.7.0" --prefix=/usr
ninja -C "/tmp/src/hyprland-protocols-0.7.0/build" install
DESTDIR=/opt/hypr-bundle ninja -C "/tmp/src/hyprland-protocols-0.7.0/build" install
python3 - << 'PY_EOF'
import pathlib, re
for p in pathlib.Path("/tmp/src/hyprwire-0.3.1/src").rglob("*.cpp"):
    txt = p.read_text()
    new_txt = re.sub(
        r'([a-zA-Z_][a-zA-Z0-9_.\->]*)\.append_range\(([^;]+)\);',
        r'{ auto&& _rng = (\2); \1.insert(\1.end(), std::begin(_rng), std::end(_rng)); }',
        txt,
    )
    if new_txt != txt:
        p.write_text(new_txt)
PY_EOF
build_cmake "hyprwire-0.3.1" -DBUILD_TESTING=OFF
build_cmake "aquamarine-0.12.0" -DBUILD_TESTING=OFF
sed -i 's/+ filename/+ std::string(filename)/g' /tmp/src/Hyprland-0.54.3/hyprctl/src/main.cpp
sed -i 's/requestedModes.insert_range(requestedModes.end(), sortedModes | std::views::reverse);/for (const auto\& _m : sortedModes | std::views::reverse) requestedModes.push_back(_m);/g' /tmp/src/Hyprland-0.54.3/src/helpers/Monitor.cpp
build_cmake "Hyprland-0.54.3" -DCMAKE_C_COMPILER=clang -DCMAKE_CXX_COMPILER=clang++ -DNO_HYPRPM=ON -DNO_UWSM=ON -DNO_SYSTEMD=ON -DBUILD_TESTING=OFF
CHROOT_EOF
    chmod +x "${SYSROOT}/tmp/build-inside-chroot.sh"

    NPROC="${NPROC}" unshare -r -p -f -m bash -c "mount -t proc proc '${SYSROOT}/proc' && mount --rbind /dev '${SYSROOT}/dev' && chroot '${SYSROOT}' /tmp/build-inside-chroot.sh"

    echo "==> Packaging cached Hyprland bundle: ${BUNDLE_TAR}"
    tar -cJf "${BUNDLE_TAR}" -C "${SYSROOT}/opt/hypr-bundle" .
}

mkdir -p "${CACHE_DIR}" "${BUILD_DIR}" "${TARGET_ROOT}/var/db/xbps/keys" "${CACHE_DIR}/xbps-pkgs"
bootstrap_xbps_static

if [ -d "${XBPS_STATIC_DIR}/var/db/xbps/keys" ]; then
    cp -a "${XBPS_STATIC_DIR}/var/db/xbps/keys/." "${TARGET_ROOT}/var/db/xbps/keys/"
fi

RUNTIME_PACKAGES=(
    quickshell
    seatd
    foot
    mesa-dri
    wayland
    libinput
    libxkbcommon
    pixman
    cairo
    pango
    xkeyboard-config
    dejavu-fonts-ttf
    libdisplay-info
    libjxl
    re2
    glslang
    libuuid
    libxcb
    xcb-util-wm
    xcb-util-errors
    xcb-util-keysyms
    libXcursor
    udis86
    xorg-server-xwayland
    libliftoff
    hyprutils
    pugixml
    eudev-libudev
    libzip
    librsvg
    file
    muparser
)

echo "==> Installing Hyprland/Quickshell runtime dependencies into ${TARGET_ROOT}"
"${XBPS_INSTALL}" \
    -r "${TARGET_ROOT}" \
    -R "${VOID_MIRROR}" \
    -c "${CACHE_DIR}/xbps-pkgs" \
    -S -u -y --ignore-conf-repos \
    "${RUNTIME_PACKAGES[@]}"

build_hyprland_bundle

echo "==> Extracting Hyprland bundle into ${TARGET_ROOT}"
tar -xJf "${BUNDLE_TAR}" -C "${TARGET_ROOT}"

# Stage Hyprland configuration and direct-to-UI session launcher
mkdir -p "${TARGET_ROOT}/etc/hypr" "${TARGET_ROOT}/usr/bin" "${TARGET_ROOT}/usr/share/petra/shell"

cat > "${TARGET_ROOT}/etc/hypr/hyprland.conf" << 'EOF'
monitor=,preferred,auto,1

cursor {
    no_hardware_cursors = true
}

general {
    gaps_in = 6
    gaps_out = 12
    border_size = 2
    col.active_border = rgba(e11d48ee) rgba(fb7185ee) 45deg
    col.inactive_border = rgba(334155aa)
    layout = dwindle
}

decoration {
    rounding = 14
    blur {
        enabled = false
    }
    shadow {
        enabled = false
    }
}

animations {
    enabled = true
}

# Auto-launch PetraOS Quickshell UI
exec-once = quickshell -c /usr/share/petra/shell/shell.qml

$mainMod = SUPER
bind = $mainMod, Return, exec, foot
bind = $mainMod, Q, killactive,
bind = $mainMod, M, exit,
bind = $mainMod, 1, workspace, 1
bind = $mainMod, 2, workspace, 2
bind = $mainMod, 3, workspace, 3
bind = $mainMod, 4, workspace, 4
bind = $mainMod, 5, workspace, 5
EOF

cat > "${TARGET_ROOT}/usr/bin/start-petra-desktop" << 'EOF'
#!/bin/bash
export XDG_RUNTIME_DIR=/run/user/0
export XDG_SESSION_TYPE=wayland
export XDG_CURRENT_DESKTOP=Hyprland
export WLR_NO_HARDWARE_CURSORS=1
export AQ_DRM_DEVICES=/dev/dri/card0
export LIBGL_ALWAYS_SOFTWARE=1
export QT_QPA_PLATFORM=wayland

mkdir -p -m 0700 "${XDG_RUNTIME_DIR}"

if [ ! -e /dev/dri/card0 ]; then
    echo "start-petra-desktop: /dev/dri/card0 not found, skipping GUI launch." >&2
    exit 0
fi

if ! pgrep -x seatd >/dev/null 2>&1; then
    seatd -u root -g root >/var/log/seatd.log 2>&1 &
    sleep 0.2
fi

exec Hyprland -c /etc/hypr/hyprland.conf >/var/log/hyprland.log 2>&1
EOF
chmod 0755 "${TARGET_ROOT}/usr/bin/start-petra-desktop"

chmod -R u+w "${TARGET_ROOT}" 2>/dev/null || true
echo "    hyprland + quickshell layer successfully staged in ${TARGET_ROOT}"
