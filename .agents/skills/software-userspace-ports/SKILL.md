---
name: software-userspace-ports
description: Port, patch, and rebuild userspace packages with xbstrap for PetraOS (mlibc + sysdeps/petra). Use this skill when creating or editing packages/**/*.yml, managing *.patch files, cross-compiling with --host=x86_64-petra, or rebuilding sysroot/initramfs and testing in QEMU. Covers xbstrap init/fetch/patch/build, patch creation, patch application, and incremental/full rebuild workflows.
---

# Software Userspace Ports Skill (`software-userspace-ports`)

Central skill for **all xbstrap userspace operations** in PetraOS: porting new software, patching existing sources, creating/applying patches, and recompiling/rebuilding the sysroot + initramfs.

> **Primary engine:** [`tools/build_and_run_userspace.sh`](file:///home/ananta/PetraOS/tools/build_and_run_userspace.sh) — all xbstrap calls are wrapped here. Prefer it over raw `xbstrap`. Complementary packaging: [`tools/create_initramfs.sh`](file:///home/ananta/PetraOS/tools/create_initramfs.sh) and top-level [`GNUmakefile`](file:///home/ananta/PetraOS/GNUmakefile) / [`bootstrap.yml`](file:///home/ananta/PetraOS/bootstrap.yml).

---

## 1. When to Use This Skill

* Adding a new port under `packages/<name>/` (`*.yml` + `*.patch`)
* Fixing cross-compile failures (`configure: error: cannot run test program`, missing `config.sub`, `petra` host rejected)
* Editing sources in `sources/<pkg>/` and turning the diff into a `*.patch`
* Verifying patches are applied (xbstrap prepare stage)
* Rebuilding a single package or all packages after a change
* Syncing `build-xbstrap/system-root` → `build/initramfs_root` → `build/initramfs.cpio` → `make run`

---

## 2. Repository Layout (Ports Relevant)

```
petraos/
├── bootstrap.yml              # General + imports: list of packages/*.yml
├── packages/<pkg>/            # One dir per port
│   ├── <pkg>.yml              # xbstrap manifest (sources/tools/packages)
│   └── 0001-*.patch           # PetraOS patches (patch_path_strip: 1)
├── patches -> packages/       # Symlink expected by xbstrap workspace
├── cross-files/
│   ├── petra-x86_64.ini       # Meson cross file (--cross-file=...)
│   └── petra-x86_64.cmake     # CMake toolchain (-DCMAKE_TOOLCHAIN_FILE=...)
├── mlibc/                     # Submodule: sysdeps/petra (see §7)
├── sources/                   # Fetched + extracted upstream sources (git-ignored)
├── build-xbstrap/             # xbstrap workspace (init, pkg-builds, sysroot)
│   └── system-root/           # SYSROOT — installed userspace tree (DESTDIR)
├── build/initramfs_root/      # Staging FHS tree for CPIO
├── build/initramfs.cpio       # SVR4 newc archive consumed by kernel
└── tools/
    ├── build_and_run_userspace.sh  # Unified xbstrap CLI
    └── create_initramfs.sh         # Sync sysroot + package CPIO
```

**Key xbstrap directories (inside `build-xbstrap/`):**
`pkg-builds/<pkg>` build trees, `packages/<pkg>` stamps, `tools/<tool>` host tools, `sources/<pkg>` via `../sources`.

### 2.1 `bootstrap.yml` Pattern

```yaml
general:
  patch_author: "Ananta"
  patch_email: "kusumaananta042@gmail.com"
imports:
  - file: packages/mlibc/mlibc.yml
  - file: packages/bash/bash.yml
  # add new ports here — order matters: mlibc* first, then deps
```

> **Authorship rule (AGENTS.md:1):** Never use AI identities. `patch_author`/`patch_email` and patch `From:` headers must match `git config user.name/email`.

---

## 3. xbstrap YML Anatomy

Three top-level keys — `sources`, `tools`, `packages`. Most ports define `sources` + `packages`; host-built helpers define `tools`.

```yaml
sources:
  - name: foo
    subdir: sources               # relative to repo root → sources/foo
    url: https://ftpmirror.gnu.org/gnu/foo/foo-1.0.tar.gz
    format: tar.gz                # or tar.xz
    extract_path: foo-1.0         # top dir inside archive to strip
    version: '1.0'
    patch_path_strip: 1           # patches are -p1 (xbstrap applies at source root)
    # git alternative:
    # git: https://github.com/org/foo.git
    # tag: 'v1.0'  # or branch: master
    # sources_required: ['gnulib']  # inject another source
    # tools_required: [host-autoconf-v2.69]
    # regenerate:                 # run after patch, before configure
    #   - args: ['cp', '@BUILD_ROOT@/tools/host-automake-v1.16/share/automake-1.16/config.sub', '@THIS_SOURCE_DIR@/']

packages:
  - name: foo
    architecture: x86_64
    from_source: foo
    tools_required: [host-gcc]
    pkgs_required: [mlibc, ncurses]  # runtime deps — built first
    configure:
      - args: ['@THIS_SOURCE_DIR@/configure', '--host=x86_64-petra', '--prefix=/usr', '--disable-nls']
      # Meson: ['meson', 'setup', '@BUILD_ROOT@/pkg-builds/foo', '@THIS_SOURCE_DIR@', '--cross-file=@SOURCE_ROOT@/cross-files/petra-x86_64.ini', '--prefix=/usr', ...]
      # CMake: ['cmake', '-B', '@THIS_BUILD_DIR@', '-S', '@THIS_SOURCE_DIR@', '-DCMAKE_TOOLCHAIN_FILE=@SOURCE_ROOT@/cross-files/petra-x86_64.cmake', '-DCMAKE_SYSROOT=@SYSROOT_DIR@', ...]
    build:
      - args: ['make', '-j@PARALLELISM@']
    install:
      - args: ['make', 'DESTDIR=@SYSROOT_DIR@', 'install-strip']
      # optional compat symlinks:
      - args: ['ln', '-sf', '/usr/bin/foo', '@SYSROOT_DIR@/bin/foo']
```

### 3.1 Variable Substitutions

| Variable | Meaning |
|---|---|
| `@THIS_SOURCE_DIR@` | Extracted source root (`sources/<pkg>` or `build-xbstrap/pkg-builds/<pkg>/src`) |
| `@THIS_BUILD_DIR@` | Per-package build dir (`build-xbstrap/pkg-builds/<pkg>`) |
| `@BUILD_ROOT@` | Workspace root (`build-xbstrap`) |
| `@SOURCE_ROOT@` | Repo root |
| `@SYSROOT_DIR@` | `build-xbstrap/system-root` |
| `@PREFIX@` | Host tool prefix (`build-xbstrap/tools/<tool>`) |
| `@PARALLELISM@` | `nproc` |
| `DESTDIR` / `SYSROOT_DIR` env | Respect for `make`/`cmake`/`ninja` install |

### 3.2 Tool vs Package

* **tool** (`tools:`) — built for **host** (`--prefix=@PREFIX@`, no `DESTDIR`), provides `host-<name>` for later cross builds (e.g., `host-autoconf-v2.69`, `host-binutils`, `host-gcc`).
* **package** (`packages:`) — cross-compiled for **petra** (`--host=x86_64-petra --prefix=/usr DESTDIR=@SYSROOT_DIR@`). Installed into sysroot, later packed into initramfs.

---

## 4. Quick Reference — Commands

All commands delegate to `tools/build_and_run_userspace.sh`. Raw xbstrap must run **inside** `build-xbstrap` (`cd build-xbstrap && xbstrap ...`).

| Goal | Script (preferred) | Raw xbstrap / Make |
|---|---|---|
| Init workspace | `bash tools/build_and_run_userspace.sh init` | `mkdir -p build-xbstrap && (cd build-xbstrap && xbstrap init ..)` |
| Fetch one | `bash tools/build_and_run_userspace.sh fetch <pkg>` | `(cd build-xbstrap && xbstrap fetch <pkg>)` |
| Fetch all | `bash tools/build_and_run_userspace.sh fetch --all` / `make fetch-userspace` | `(cd build-xbstrap && xbstrap fetch --all)` |
| Inspect patches | `bash tools/build_and_run_userspace.sh patch <pkg>` | `ls packages/<pkg>/*.patch` |
| Build one | `bash tools/build_and_run_userspace.sh build <pkg>` | `(cd build-xbstrap && xbstrap install <pkg>)` |
| Build all | `bash tools/build_and_run_userspace.sh build-all` / `make build-userspace` | loop `xbstrap install <pkg>` in bootstrap order |
| Clean one | `bash tools/build_and_run_userspace.sh clean <pkg>` | `rm -rf build-xbstrap/pkg-builds/<pkg>* build-xbstrap/pkg-stamps/<pkg>*` |
| Clean all | `bash tools/build_and_run_userspace.sh clean --all` / `make clean-userspace` | `rm -rf build-xbstrap` |
| Status | `bash tools/build_and_run_userspace.sh status [pkg]` | — |
| Sync initramfs only | `make sync-initramfs` | `./tools/create_initramfs.sh --sync-only build/initramfs_root build-xbstrap/system-root` |
| Package CPIO only | — | `./tools/create_initramfs.sh --package-only build/initramfs_root build/initramfs.cpio` |
| Full sync+package | `make initramfs` | `./tools/create_initramfs.sh build/initramfs_root build/initramfs.cpio build-xbstrap/system-root` |
| Core pipeline + QEMU | `bash tools/build_and_run_userspace.sh` | — |
| Full pipeline + QEMU | `bash tools/build_and_run_userspace.sh --all` | — |
| Run QEMU | `make run` / `make run QEMUFLAGS="-m 4G -serial stdio"` | — |

**Core packages** built by default pipeline: `mlibc`, `bash`, `coreutils`. Full pipeline discovers all `packages:` entries in `bootstrap.yml` (mlibc ordered first).

---

## 5. Patch Workflow — Modify, Create, Apply

Patches are **automatically applied by xbstrap during the prepare/fetch stage** (before `regenerate`/`configure`). Do **not** manually `patch -p1` inside `build-xbstrap`.

### 5.1 Inspect Existing Patches

```bash
bash tools/build_and_run_userspace.sh patch bash
# → lists packages/bash/*.patch
cat packages/bash/0001-petra-port.patch
cat packages/mlibc/0001-petra-port.patch | head -n 50
```

Check `patch_path_strip: 1` in the YML — patches must be `-p1` relative to the source root.

### 5.2 Patching Flow (Edit → Diff → New Patch)

**Recommended — edit upstream source, then export diff:**

```bash
# 1. Ensure source is fetched (patches already applied to the copy xbstrap uses)
bash tools/build_and_run_userspace.sh fetch <pkg>
ls sources/<pkg>/          # extracted upstream (patched view)
# If source is a git repo, xbstrap keeps it in sources/<pkg> as a git checkout

# 2. Make edits in sources/<pkg>/  (the canonical place to hack)
# e.g. fix configure, add petra case, stub missing syscall
nano sources/bash/support/config.sub
# or: sources/ncurses/configure  — add petra) branch

# 3a. If sources/<pkg> is git — generate patch via git
cd sources/<pkg>
git diff > /tmp/myfix.patch
# or for committed style:
git add -A && git commit -m "feat(petra): fix ..."
git format-patch -1 HEAD -o /tmp/
# then copy & rename:
cp /tmp/0001-*.patch ../../packages/<pkg>/0002-my-fix.patch

# 3b. If sources/<pkg> is tarball — use diff -u
# Save pristine copy first, or re-fetch to a temp dir:
bash tools/build_and_run_userspace.sh clean <pkg>
bash tools/build_and_run_userspace.sh fetch <pkg>  # pristine
cp -a sources/<pkg> /tmp/<pkg>.orig
# ... edit sources/<pkg>/ ...
diff -ruN /tmp/<pkg>.orig sources/<pkg> > packages/<pkg>/0002-my-fix.patch

# 3c. Alternative: edit inside build-xbstrap/pkg-builds/<pkg>/ and diff against sources/
# Less preferred — use sources/ as source of truth.

# 4. Ensure patch header attribution
head packages/<pkg>/0002-my-fix.patch
# Must contain:
# From: <git config user.name> <git config user.email>
# Date: ...
# Subject: [PATCH] feat(petra): ...
# Never AI identities.

# 5. Clean and rebuild to verify patch applies cleanly
bash tools/build_and_run_userspace.sh clean <pkg>
bash tools/build_and_run_userspace.sh build <pkg>
# If patch fails: xbstrap will error during prepare — fix offsets/fuzz, ensure -p1.
```

**Regenerate considerations:**
* Autotools ports often need `regenerate:` to refresh `config.sub`/`configure` after patching. Example pattern from [`packages/ncurses/ncurses.yml`](file:///home/ananta/PetraOS/packages/ncurses/ncurses.yml):
  ```yaml
  regenerate:
    - args: ['cp', '@BUILD_ROOT@/tools/host-automake-v1.16/share/automake-1.16/config.sub', '@THIS_SOURCE_DIR@/']
  ```
* For `configure`-patched files (e.g., `ncurses/configure` petra shared-lib block), patch the generated `configure`, not `configure.ac`, unless you also add autoreconf steps.
* `gnulib` example ([`packages/coreutils/coreutils.yml`](file:///home/ananta/PetraOS/packages/coreutils/coreutils.yml)) copies gnulib into source tree in `regenerate`.

### 5.3 Patch Application Verification

```bash
# After adding patch, force re-prepare:
bash tools/build_and_run_userspace.sh clean <pkg>
bash tools/build_and_run_userspace.sh fetch <pkg>
# xbstrap extracts, then applies packages/<pkg>/*.patch in lexical order
# Check no rejects:
find build-xbstrap/pkg-builds/<pkg> -name "*.rej" 2>/dev/null
grep -r "petra" sources/<pkg>/support/config.sub  # verify patched content visible in build tree
```

### 5.4 Patch Style Conventions in This Repo

* File name: `0001-petra-port.patch`, `0002-*.patch` — lexical order matters.
* Header: `From: Ananta <kusumaananta042@gmail.com>` (human maintainer), `Subject: [PATCH] feat(petra): add PetraOS support to <pkg> <ver>`
* Minimal, upstream-friendly hunks; include `Signed-off-by` when appropriate.
* Keep `patch_path_strip: 1` consistent; patches generated with `git format-patch` are `-p1` by default.

---

## 6. Rebuild Workflows — After Modifications

### 6.1 Single Package Incremental Rebuild (Fast)

Use when you edited `packages/<pkg>.yml` or `packages/<pkg>/*.patch`:

```bash
bash tools/build_and_run_userspace.sh clean <pkg>
bash tools/build_and_run_userspace.sh build <pkg>
# Verify installed:
ls -l build-xbstrap/system-root/usr/bin/<binary>
file build-xbstrap/system-root/usr/bin/<binary>  # should be ELF 64-bit LSB, x86-64, for PetraOS
# Optional: check linked against Petra sysroot
readelf -d build-xbstrap/system-root/usr/bin/<binary> | head

# Sync into initramfs + repackage (required before QEMU sees changes)
make initramfs
# or fine-grained:
make sync-initramfs && ./tools/create_initramfs.sh --package-only

# Test in QEMU (CLI harness or manual)
make run QEMUFLAGS="-m 4G -serial stdio"
# or via skill cli-testing:
python3 tools/test_cli.py --cmd "<binary> --version" --expect "<version>"
```

### 6.2 Full Rebuild (All Packages)

Needed after `mlibc` changes (sysdeps/petra) or toolchain (`gcc`, `binutils`) updates — everything depends on mlibc:

```bash
bash tools/build_and_run_userspace.sh build-all
# or
make build-userspace
# then
make initramfs && make run
```

> **Order:** `mlibc-headers` → `mlibc` → `binutils/gcc` staged tools (if changed) → dependents (`ncurses`, `readline`, `bash`, `coreutils`, `vim`, `nano`, `fastfetch`, ...). Script `discover_all_packages()` enforces mlibc-first via `xbstrap list-pkgs`.

### 6.3 Clean Strategy

* `clean <pkg>` removes `pkg-builds/<pkg>*`, `pkg-stamps/<pkg>*`, and `system-root` stamp — keeps `sources/<pkg>` (no re-download).
* `clean --all` removes entire `build-xbstrap` — next build re-fetches all sources and reapplies patches (slow; use for toolchain/sysroot corruption).
* To discard a bad patch and restore pristine source: `clean <pkg>` + delete/revert the `.patch` file + `fetch <pkg>`.

### 6.4 Screening Rebuild Success

```bash
bash tools/build_and_run_userspace.sh status <pkg>
ls build-xbstrap/system-root/usr/lib/*.so* 2>/dev/null | head
cat build/initramfs.cpio | cpio -t 2>/dev/null | grep <binary> | head
```

---

## 7. What Makes a Proper Petra Port — Checklist

A port is **proper** only if it cross-compiles reproducibly against `mlibc` + `sysdeps/petra` and runs on PetraOS without host leakage.

### 7.1 Mandatory Cross-Compile Hygiene

* **`--host=x86_64-petra`** (autotools) or correct cross-file (`--cross-file=@SOURCE_ROOT@/cross-files/petra-x86_64.ini` for Meson, `-DCMAKE_TOOLCHAIN_FILE=@SOURCE_ROOT@/cross-files/petra-x86_64.cmake` for CMake). Never `--host=x86_64-linux-gnu`.
* **`--prefix=/usr`** and **`DESTDIR=@SYSROOT_DIR@`** — no absolute host paths in install.
* **`CFLAGS`/`LDFLAGS` with sysroot:** e.g., `CFLAGS="--sysroot=@SYSROOT_DIR@ -I@SYSROOT_DIR@/usr/include"` or meson/cmake `CMAKE_SYSROOT=@SYSROOT_DIR@`. See [`packages/vim/vim.yml`](file:///home/ananta/PetraOS/packages/vim/vim.yml) (`CFLAGS: '-O2 -pipe --sysroot=@SYSROOT_DIR@ ...'`).
* **No host tool leakage:** `tools_required: [host-gcc]` etc.; do not call bare `gcc`/`pkg-config` without sysroot.
* **Parallelism:** use `-j@PARALLELISM@`.

### 7.2 Petra Host Detection

* Autotools: `config.sub` must recognize `petra`. Patch `support/config.sub` or top-level `config.sub` to add `| petra*` alongside `linux* | ...` (see [`packages/bash/0001-petra-port.patch`](file:///home/ananta/PetraOS/packages/bash/0001-petra-port.patch)).
* `config.guess` / `configure` may need `--with-shared` or `petra)` case (see [`packages/ncurses/0001-petra-port.patch`](file:///home/ananta/PetraOS/packages/ncurses/0001-petra-port.patch) shared-lib block).
* Meson: already handled via `petra` system name in cross file ([`cross-files/petra-x86_64.ini`](file:///home/ananta/PetraOS/cross-files/petra-x86_64.ini) `system = 'petra'`).
* CMake: `CMAKE_SYSTEM_NAME Petra` in [`cross-files/petra-x86_64.cmake`](file:///home/ananta/PetraOS/cross-files/petra-x86_64.cmake).

### 7.3 Feature Probing & Missing Syscalls

Petra's kernel implements a Linux-like subset. Many `configure` probes try to **run** target binaries (fails when cross-compiling). Pre-seed cache variables:

* **Bash style:** `bash_cv_job_control_missing=present`, `bash_cv_sys_named_pipes=present`, `bash_cv_func_sigsetjmp=present`, `bash_cv_getcwd_malloc=yes` ([`packages/bash/bash.yml`](file:///home/ananta/PetraOS/packages/bash/bash.yml)).
* **ncurses style:** `cf_cv_func_nanosleep=yes`, `cf_cv_sizechange=yes` ([`packages/ncurses/ncurses.yml`](file:///home/ananta/PetraOS/packages/ncurses/ncurses.yml)).
* **vim style:** `vim_cv_terminfo=yes`, `vim_cv_tgetent=zero` ([`packages/vim/vim.yml`](file:///home/ananta/PetraOS/packages/vim/vim.yml)).
* **pkg-config/glib style:** `glib_cv_stack_grows=no`, `ac_cv_func_posix_getpwuid_r=yes` ([`packages/pkg-config/pkg-config.yml`](file:///home/ananta/PetraOS/packages/pkg-config/pkg-config.yml)).

When configure still fails, search `config.log` in `build-xbstrap/pkg-builds/<pkg>/` for `cannot run test program` or `undefined reference to` and add the corresponding `*_cv_*` variable.

Disable unavailable subsystems explicitly:
`--disable-nls`, `--disable-gpm`, `--disable-acl`, `--disable-selinux`, `--disable-gui`, `--without-x`, `--disable-libmagic`, `--disable-multilib`, etc. (see `vim`, `nano`, `gcc`, `binutils` ymls).

### 7.4 Dependencies

* Always `pkgs_required: [mlibc]`. Add `ncurses`, `readline`, `pkg-config`, `tzdata`, etc. as needed. Host tools go in `tools_required`.
* For ports requiring `gnulib` (coreutils), use `sources_required: ['gnulib']` + `regenerate` bootstrap copy.
* For `tzdata`, the host `zic` tool pattern (`host-zic`) is required.

### 7.5 Install & FHS

* Use `make DESTDIR=@SYSROOT_DIR@ install-strip` (or `DESTDIR` env for `ninja`/`cmake --install`).
* Mirror binaries for FHS compat if needed: `ln -sf /usr/bin/foo @SYSROOT_DIR@/bin/foo` (see `bash`, `vim`, `fastfetch` ymls). `create_initramfs.sh` further mirrors `/usr/bin` → `/bin` etc.
* For libraries, handle `.so` placement and pkg-config links (see `ncurses.yml` post-install `INPUT(-l...)` trick).

### 7.6 mlibc / Sysdeps Boundary

If a port fails with `undefined reference` to a libc symbol or `ENOSYS` at runtime:

1. Check `mlibc/sysdeps/petra/generic/generic.cpp` — is the required `Sysdeps<...>::operator()` implemented? If not, add it using `__petra_syscallN` from `sysdeps/petra/include/sys/syscall.h`.
2. Add ABI headers as symlinks to `abis/linux/*.h` in `sysdeps/petra/include/abi-bits/` (see mlibc patch).
3. Rebuild `mlibc` first: `bash tools/build_and_run_userspace.sh clean mlibc && bash tools/build_and_run_userspace.sh build mlibc`, then rebuild the dependent package.
4. For kernel-side missing syscalls, implement handler in `kernel/src/syscalls/` and wire to `kernel/src/syscalls/mod.rs` dispatcher — then rebuild kernel (`make -C kernel`).

### 7.7 Quality Gates for a New Port

Before submitting, verify:

* [ ] `bash tools/build_and_run_userspace.sh clean <pkg> && bash tools/build_and_run_userspace.sh build <pkg>` succeeds from clean state
* [ ] `file build-xbstrap/system-root/usr/bin/<bin>` reports `ELF 64-bit LSB pie executable, x86-64` (or `shared object`) with interpreter `/lib/ld-linux-x86-64.so.2` or Petra `ld.so`
* [ ] Binary appears in `build/initramfs.cpio` after `make initramfs`
* [ ] Runs in Petra QEMU: `python3 tools/test_cli.py --cmd "<bin> --help" --expect "<hint>"` or manual `make run` interactive test
* [ ] Patch (if any) has correct human `From:` + `patch_path_strip: 1` and applies idempotently
* [ ] YML added to `bootstrap.yml` imports and package is discovered by `xbstrap list-pkgs`

---

## 8. Porting a New Package — End-to-End Recipe

### 8.1 Minimal Template (Autotools)

```yaml
sources:
  - name: htop
    subdir: sources
    url: https://github.com/htop-dev/htop/archive/refs/tags/3.3.0.tar.gz
    format: tar.gz
    extract_path: htop-3.3.0
    version: '3.3.0'
    patch_path_strip: 1

packages:
  - name: htop
    architecture: x86_64
    from_source: htop
    tools_required: [host-gcc, host-autoconf-v2.69, host-automake-v1.16]
    pkgs_required: [mlibc, ncurses, pkg-config]
    configure:
      - args: ['@THIS_SOURCE_DIR@/autogen.sh']  # if needed
      - args: ['@THIS_SOURCE_DIR@/configure', '--host=x86_64-petra', '--prefix=/usr', '--disable-unicode', '--disable-nls']
    build:
      - args: ['make', '-j@PARALLELISM@']
    install:
      - args: ['make', 'DESTDIR=@SYSROOT_DIR@', 'install-strip']
```

### 8.2 CMake Template

See [`packages/fastfetch/fastfetch.yml`](file:///home/ananta/PetraOS/packages/fastfetch/fastfetch.yml) — use `cmake -B @THIS_BUILD_DIR@ -S @THIS_SOURCE_DIR@ -DCMAKE_TOOLCHAIN_FILE=@SOURCE_ROOT@/cross-files/petra-x86_64.cmake -DCMAKE_SYSROOT=@SYSROOT_DIR@`.

### 8.3 Meson Template

See [`packages/mlibc/mlibc.yml`](file:///home/ananta/PetraOS/packages/mlibc/mlibc.yml) — use `meson setup @BUILD_ROOT@/pkg-builds/<pkg> @THIS_SOURCE_DIR@ --cross-file=@SOURCE_ROOT@/cross-files/petra-x86_64.ini --prefix=/usr`.

### 8.4 Steps

```bash
# 1. Create YML and stub patches dir
mkdir -p packages/<newpkg>
cat > packages/<newpkg>/<newpkg>.yml <<'YML'
# ... template above ...
YML

# 2. Register in bootstrap.yml
# Edit bootstrap.yml: add "- file: packages/<newpkg>/<newpkg>.yml"

# 3. Fetch and attempt build (expect failures first iteration)
bash tools/build_and_run_userspace.sh fetch <newpkg>
bash tools/build_and_run_userspace.sh build <newpkg>
# Read build log: build-xbstrap/pkg-builds/<newpkg>/meson-logs/meson-log.txt or config.log

# 4. Fix host detection / cache variables / missing deps — edit YML, add .patch if needed
# For config.sub issue: create packages/<newpkg>/0001-petra-port.patch (see §5)

# 5. Iterate: clean + build until success
bash tools/build_and_run_userspace.sh clean <newpkg>
bash tools/build_and_run_userspace.sh build <newpkg>

# 6. Sync and test
make initramfs
python3 tools/test_cli.py --cmd "<newpkg> --version" --expect "<ver>"
make run  # manual interactive fallback
```

> **Example pending port:** `packages/git/` exists but is empty — apply the recipe above with `git` sources (`https://github.com/git/git/archive/v2.43.0.tar.gz`) and deps `mlibc`, `zlib` (add `zlib` port first if missing).

---

## 9. Troubleshooting

| Symptom | Cause | Fix |
|---|---|---|
| `configure: error: cannot run test program while cross compiling` | Probe tries to execute target | Add `*_cv_*` / `bash_cv_*` cache var in `configure:` args |
| `config.sub: ... machine 'x86_64-petra' not recognized` | `config.sub` predates petra | Patch `config.sub` to add `| petra*` or `cp` from `host-automake-v1.16/share/automake-1.16/config.sub` via `regenerate:` |
| `undefined reference to 'getrandom'` | Missing mlibc sysdep or kernel syscall | Implement `Sysdeps<...>` in `mlibc/sysdeps/petra/generic/generic.cpp` + kernel handler |
| `cannot find -lncurses` / `pkg-config not found` | Missing `pkgs_required` / `PKG_CONFIG_SYSROOT_DIR` | Add `pkgs_required: [ncurses, pkg-config]` and ensure `PKG_CONFIG_PATH=@SYSROOT_DIR@/usr/lib/pkgconfig` |
| Patch fails `Hunk #1 FAILED` | Wrong `-p` / stale offsets | Regenerate patch from current `sources/<pkg>` with `patch_path_strip: 1` |
| `xbstrap: package not found` | YML not imported or missing `packages:` | Add import to `bootstrap.yml`, ensure `packages:` key exists |
| Binary not in initramfs | `install` didn't use `DESTDIR=@SYSROOT_DIR@` | Fix install step, then `make initramfs` |
| `file` shows `x86-64` but `Exec format error` in QEMU | Interpreter mismatch | Ensure `mlibc` built first; check `readelf -l <bin> | grep interpreter` matches Petra `ld.so` |
| `build-xbstrap` corrupted after toolchain change | Stale stamps | `bash tools/build_and_run_userspace.sh clean --all && bash tools/build_and_run_userspace.sh build-all` |

**Log locations:**
* `build-xbstrap/pkg-builds/<pkg>/config.log` (autotools)
* `build-xbstrap/pkg-builds/<pkg>/meson-logs/meson-log.txt`
* `build-xbstrap/pkg-builds/<pkg>/CMakeFiles/CMakeError.log`
* `build/initramfs.cpio` listing: `cpio -t < build/initramfs.cpio | sort`

---

## 10. Agent Workflow Checklist

When asked to patch / port / rebuild:

1. **Read** `AGENTS.md:0` (mandatory), this skill, and current `packages/<pkg>.yml` + `*.patch`.
2. **Fetch** source if needed: `bash tools/build_and_run_userspace.sh fetch <pkg>`.
3. **Edit** either `packages/<pkg>.yml` or `sources/<pkg>/` content.
4. **If sources edited:** create `packages/<pkg>/000N-*.patch` with correct `From:` and `patch_path_strip: 1` (§5.2).
5. **Clean & build** the target: `clean <pkg>` → `build <pkg>` → verify `build-xbstrap/system-root` artifact.
6. **Sync & package** initramfs: `make initramfs`.
7. **Verify** in QEMU: `tools/test_cli.py --cmd "<bin> --version"` or `make run` manual.
8. **If mlibc/kernel changed:** rebuild dependents (`build-all`) and re-test.

Never commit patches with AI author; always use `git config user.name/email` (AGENTS.md:1).

---

## 11. References

* Engine: [`tools/build_and_run_userspace.sh:1`](file:///home/ananta/PetraOS/tools/build_and_run_userspace.sh)
* Packaging: [`tools/create_initramfs.sh:1`](file:///home/ananta/PetraOS/tools/create_initramfs.sh)
* Manifest: [`bootstrap.yml:1`](file:///home/ananta/PetraOS/bootstrap.yml)
* Cross files: [`cross-files/petra-x86_64.ini:1`](file:///home/ananta/PetraOS/cross-files/petra-x86_64.ini), [`cross-files/petra-x86_64.cmake:1`](file:///home/ananta/PetraOS/cross-files/petra-x86_64.cmake)
* Example ports: [`packages/bash/bash.yml:1`](file:///home/ananta/PetraOS/packages/bash/bash.yml), [`packages/ncurses/ncurses.yml:1`](file:///home/ananta/PetraOS/packages/ncurses/ncurses.yml), [`packages/vim/vim.yml:1`](file:///home/ananta/PetraOS/packages/vim/vim.yml), [`packages/fastfetch/fastfetch.yml:1`](file:///home/ananta/PetraOS/packages/fastfetch/fastfetch.yml), [`packages/mlibc/mlibc.yml:1`](file:///home/ananta/PetraOS/packages/mlibc/mlibc.yml)
* Patches: [`packages/bash/0001-petra-port.patch:1`](file:///home/ananta/PetraOS/packages/bash/0001-petra-port.patch), [`packages/ncurses/0001-petra-port.patch:1`](file:///home/ananta/PetraOS/packages/ncurses/0001-petra-port.patch), [`packages/mlibc/0001-petra-port.patch:1`](file:///home/ananta/PetraOS/packages/mlibc/0001-petra-port.patch)
* Top-level build: [`GNUmakefile:54`](file:///home/ananta/PetraOS/GNUmakefile#L54) (`build-userspace`, `fetch-userspace`, `sync-initramfs`)
