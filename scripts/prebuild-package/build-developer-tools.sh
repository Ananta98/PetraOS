#!/bin/bash
# Stage the PetraOS developer-tools layer: everything the CLI environment
# promises, downloaded as prebuilt Alpine packages with nothing but curl and
# tar. Dependencies are resolved from the repository indexes (the embedded awk
# solver at the bottom), matching a real Alpine install without needing apk,
# root, or a chroot.
#
# Output: build/prebuild-package/developer-tools/staging, overlayed onto the
# userland root by build-userland/build-userland-amd64.sh.
#
# Environment:
#   ALPINE_BRANCH/ALPINE_MIRROR  override the Alpine release/mirror
#   PETRAOS_DEVELOPER_PACKAGES   space separated root package set
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

ALPINE_BRANCH="${ALPINE_BRANCH:-v3.20}"
ALPINE_ARCH="${ALPINE_ARCH:-x86_64}"
ALPINE_MIRROR="${ALPINE_MIRROR:-https://dl-cdn.alpinelinux.org/alpine}"

BUILD_DIR="${REPO_ROOT}/build/prebuild-package/developer-tools"
DOWNLOADS="${BUILD_DIR}/downloads"
STAGING="${BUILD_DIR}/staging"

# Keep this list explicit: it is both the image manifest and the user-facing
# set of tools PetraOS promises. Everything previously shipped as separate
# prebuild layers (bash, gawk, grep, sed, btop, fastfetch, htop, openssl,
# build-base, e2fsprogs, exfatprogs) plus the native development toolchain is
# resolved from the pinned Alpine branch above.
ROOT_PACKAGES=(
    autoconf
    automake
    bash
    btop
    build-base
    ca-certificates-bundle
    cmake
    coreutils
    diffutils
    e2fsprogs
    exfatprogs
    fastfetch
    file
    findutils
    gawk
    gdb
    git
    grep
    htop
    libtool
    m4
    make
    meson
    openssl
    patch
    pkgconf
    samurai
    sed
    strace
    tar
    tmux
    xz
)
root_packages="${PETRAOS_DEVELOPER_PACKAGES:-${ROOT_PACKAGES[*]}}"

for tool in curl tar; do
    command -v "$tool" >/dev/null 2>&1 || {
        echo "ERROR: required command not found: $tool" >&2
        exit 1
    }
done

mkdir -p "${DOWNLOADS}"

echo "==> Fetching Alpine ${ALPINE_BRANCH} ${ALPINE_ARCH} package indexes"
for repo in main community; do
    index="${DOWNLOADS}/${repo}_APKINDEX"
    if [ ! -f "${index}" ]; then
        archive="${DOWNLOADS}/${repo}_APKINDEX.tar.gz"
        curl -fsSL --retry 3 \
            "${ALPINE_MIRROR}/${ALPINE_BRANCH}/${repo}/${ALPINE_ARCH}/APKINDEX.tar.gz" \
            -o "${archive}"
        tar xOf "${archive}" APKINDEX > "${index}"
    fi
done

echo "==> Resolving the dependency closure"
packages_list="${DOWNLOADS}/packages"
awk -v main_index="${DOWNLOADS}/main_APKINDEX" \
    -v community_index="${DOWNLOADS}/community_APKINDEX" \
    -v root_packages="${root_packages}" '
    BEGIN {
        npkg = 0
        read_index(main_index, "main")
        read_index(community_index, "community")
        n_roots = split(root_packages, root_list, / /)
        for (_entry = 1; _entry <= n_roots; _entry++) {
            if (root_list[_entry] != "")
                want(root_list[_entry])
        }
        if (npkg == 0) {
            print "ERROR: no root packages to resolve" > "/dev/stderr"
            exit 1
        }
        process_queue()
        for (_entry = 1; _entry <= npkg; _entry++)
            print repo[queue[_entry]] "\t" queue[_entry] "-" ver[queue[_entry]] ".apk"
    }

    # A package record is a set of "key:value" lines closed by a blank line.
    # Only the fields the solver needs are kept.
    function read_index(path, name,    line) {
        current_repo = name
        while ((getline line < path) > 0) {
            if (line == "")
                flush_record()
            else
                feed_field(line)
        }
        close(path)
        flush_record()
    }

    function feed_field(line) {
        if (substr(line, 1, 2) == "P:")
            pname = substr(line, 3)
        else if (substr(line, 1, 2) == "V:")
            pver = substr(line, 3)
        else if (substr(line, 1, 2) == "D:")
            pdeps = substr(line, 3)
        else if (substr(line, 1, 2) == "k:")
            pk = substr(line, 3) + 0
        else if (substr(line, 1, 2) == "p:")
            pprov = substr(line, 3)
    }

    # Drop any ">=/>/=/~<constraint>" suffix, keeping the leading "so:"/"cmd:"
    # namespace so virtual keys stay distinct from package names.
    function strip_constraint(tok) {
        sub(/[=<>~].*/, "", tok)
        return tok
    }

    # Record one complete APKINDEX record. Packages listed in more than one
    # index are taken from the first index (main).
    function flush_record(    n, raw, i, tok) {
        if (pname == "")
            return
        if (!(pname in ver)) {
            ver[pname] = pver
            repo[pname] = current_repo
            deps[pname] = (pdeps == "" ? "" : pdeps)
            prio[pname] = (pk == "" ? 0 : pk + 0)
            if (pprov != "") {
                n = split(pprov, raw, / /)
                for (i = 1; i <= n; i++) {
                    tok = strip_constraint(raw[i])
                    if (tok == "")
                        continue
                    if (tok in candidates)
                        candidates[tok] = candidates[tok] " " pname
                    else
                        candidates[tok] = pname
                }
            }
        }
        pname = pver = pdeps = pprov = pk = ""
    }

    # Compare two Alpine versions by splitting into segments and comparing
    # each one (numerically when both are numeric, lexicographically
    # otherwise); missing trailing segments count as 0. Returns >0 if a is
    # newer, <0 if b is newer, 0 on a tie.
    function newer(a, b,    na, nb, i, x, y) {
        na = split(a, sa, /[._-]/)
        nb = split(b, sb, /[._-]/)
        for (i = 1; ; i++) {
            x = (i <= na) ? sa[i] : "0"
            y = (i <= nb) ? sb[i] : "0"
            if (x == y) {
                if (i > na && i > nb)
                    return 0
                continue
            }
            if (x ~ /^[0-9]+$/ && y ~ /^[0-9]+$/) {
                x += 0
                y += 0
            }
            if (x > y) return 1
            if (x < y) return -1
        }
    }

    # Resolve one dependency token to a concrete package name. A plain package
    # name is used as-is; anything else (pure virtuals, "so:", "cmd:") is
    # looked up among the provides listed in the indexes.
    function resolve_dependency(tok, from_pkg,    canon) {
        canon = strip_constraint(tok)
        if (canon == "" || substr(canon, 1, 1) == "!")
            return ""
        if (index(canon, ":") == 0 && (canon in ver))
            return canon
        return pick_provider(canon, from_pkg)
    }

    function pick_provider(canon, from_pkg,    i, n, list, pkg, best, best_prio, best_v) {
        if (!(canon in candidates)) {
            printf "ERROR: %s: dependency '"'"'%s'"'"' not found in the package indexes\n", from_pkg, canon > "/dev/stderr"
            exit 1
        }
        # apk'"'"'s tie-break for interchangeable providers: higher provider
        # priority ("k:" in the index) wins, then the higher version, then
        # index order.
        n = split(candidates[canon], list, / /)
        best = list[1]
        best_prio = prio[best]
        best_v = ver[best]
        for (i = 2; i <= n; i++) {
            pkg = list[i]
            if (prio[pkg] < best_prio)
                continue
            if (prio[pkg] == best_prio && newer(ver[pkg], best_v) <= 0)
                continue
            best = pkg
            best_prio = prio[pkg]
            best_v = ver[pkg]
        }
        return best
    }

    function want(p) {
        if (!(p in ver)) {
            print "ERROR: package not found in the package indexes: " p > "/dev/stderr"
            exit 1
        }
        if (p in seen)
            return
        seen[p] = 1
        queue[++npkg] = p
    }

    # Breadth-first expansion of the queue; want() appends during the loop, so
    # transitive dependencies are visited as well.
    function process_queue(    i, pkg, n, j, raw, target) {
        for (i = 1; i <= npkg; i++) {
            pkg = queue[i]
            if (deps[pkg] == "")
                continue
            n = split(deps[pkg], raw, / /)
            for (j = 1; j <= n; j++) {
                target = resolve_dependency(raw[j], pkg)
                if (target != "")
                    want(target)
            }
        }
    }
' > "${packages_list}"
echo "    $(wc -l < "${packages_list}" | tr -d ' ') packages"

echo "==> Downloading and staging packages"
rm -rf "${STAGING}"
mkdir -p "${STAGING}"
while IFS=$'\t' read -r repo filename; do
    archive="${DOWNLOADS}/${filename}"
    if [ ! -f "${archive}" ]; then
        curl -fsSL --retry 3 \
            "${ALPINE_MIRROR}/${ALPINE_BRANCH}/${repo}/${ALPINE_ARCH}/${filename}" \
            -o "${archive}"
    fi
    echo "    ${filename}"
    # An apk file is a concatenation of control and payload tar streams. GNU
    # tar extracts both but can report the trailing stream as garbage, so
    # errors are ignored here and verified below.
    tar xzf "${archive}" -C "${STAGING}" 2>/dev/null || true
    rm -f "${STAGING}/.PKGINFO" "${STAGING}/.SIGN"* "${STAGING}/.trigger"* \
        "${STAGING}/.pre-"* "${STAGING}/.post-"*
done < "${packages_list}"

# Sanity checking: every promised tool must exist in the staged tree.
echo "==> Verifying the staged developer tools"
check_binary() {
    local name="$1" dir rel
    for dir in bin usr/bin sbin usr/sbin; do
        rel="${dir}/${name}"
        if [ -e "${STAGING}/${rel}" ] || [ -L "${STAGING}/${rel}" ]; then
            echo "    ${rel}"
            return 0
        fi
    done
    echo "ERROR: missing staged binary: ${name}" >&2
    exit 1
}
for binary in bash gawk grep sed btop fastfetch htop gcc g++ make cmake \
    gdb git meson ninja pkg-config mkfs.ext2 mkfs.exfat openssl; do
    check_binary "${binary}"
done
if [ ! -e "${STAGING}/lib/ld-musl-${ALPINE_ARCH}.so.1" ]; then
    echo "ERROR: missing staged musl loader lib/ld-musl-${ALPINE_ARCH}.so.1" >&2
    exit 1
fi

echo "    layer staged at ${STAGING} ($(du -sh "${STAGING}" | cut -f1))"
