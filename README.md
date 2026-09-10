# PetraOS

PetraOS is a modular monolithic, UNIX-like operating system written in Rust (`no_std` kernel) featuring a standard POSIX userland powered by mlibc, GNU toolchains, and xbstrap package orchestration.

---

## 1. Directory Structure

```
PetraOS/
├── kernel/          # Rust kernel crate (no_std, architecture, drivers, VFS, mm)
├── base-files/      # Root filesystem skeleton conforming to UsrMerge (/bin, /etc, etc.)
├── packages/        # Userland package definitions and patches (xbstrap YAMLs)
├── cross-files/     # Meson and CMake cross-compilation definition files
├── tools/           # Build and testing scripts
│   ├── build_userland.sh   # Unified userspace build engine (xbstrap wrapper)
│   ├── build_initramfs.sh  # Initramfs CPIO archive generator
│   └── test_cli.py         # Automated QEMU CLI test runner
├── limine/          # Bootloader assets and configuration (limine.conf)
├── bootstrap.yml    # Main xbstrap orchestration manifest
└── GNUmakefile      # Top-level build orchestration (ISO, HDD, QEMU runner)
```

---

## 2. Requirements & Dependencies

### Host System Tools
Install the essential host build tools:
```bash
# Debian / Ubuntu
sudo apt update
sudo apt install -y build-essential git cpio xorriso gdisk mtools qemu-system-x86 python3 python3-pip curl

# Install xbstrap (userspace package orchestrator)
pip install xbstrap
```

### Rust Toolchain
Rust is required to build the kernel crate:
```bash
rustup target add x86_64-unknown-none
```

---

## 3. Quick Start

### Build Everything & Run in QEMU
To initialize the workspace, fetch sources, compile all userspace packages, build the kernel, package the initramfs, and launch in QEMU:
```bash
./tools/build_userland.sh
```
*(Alternatively, run `make run` to boot with current artifacts).*

### Build Kernel Only
```bash
make -C kernel
```

### Build Initramfs Only
```bash
./tools/build_initramfs.sh
# or: make initramfs
```

---

## 4. Working with Userland Packages (`xbstrap`)

PetraOS uses `xbstrap` to cross-compile software into `build-xbstrap/system-root`. All package definitions reside in `packages/<package-name>/<package-name>.yml` and are managed via `tools/build_userland.sh`.

### Common Commands

| Task | Command | Description |
| :--- | :--- | :--- |
| **Build a Package** | `./tools/build_userland.sh build <pkg>` | Fetches, patches, compiles, and installs package into sysroot |
| **Fetch Source** | `./tools/build_userland.sh fetch <pkg>` | Downloads package source to `sources/<pkg>` |
| **Inspect Patches** | `./tools/build_userland.sh patch <pkg>` | Lists active patch files for the package |
| **Clean Package** | `./tools/build_userland.sh clean <pkg>` | Cleans build cache and stamps for a specific package |
| **Clean All** | `./tools/build_userland.sh clean` | Wipes the `build-xbstrap` build directory |
| **Status** | `./tools/build_userland.sh status [pkg]` | Displays workspace and package build status |

### Adding or Patching a Package

1. **Adding a New Port**:
   - Create `packages/<name>/<name>.yml` with source URL, build steps, and `DESTDIR=@SYSROOT_DIR@`.
   - Add `- file: packages/<name>/<name>.yml` to `bootstrap.yml`.
   - Run `./tools/build_userland.sh build <name>`.

2. **Creating & Applying Patches**:
   - Place patch files in `packages/<pkg>/` named sequentially (e.g., `0001-petra-port.patch`).
   - Use `-p1` strip level (`patch_path_strip: 1` in YML).
   - Patches are applied automatically by `xbstrap` during the source prepare phase.
   - Clean and rebuild to verify:
     ```bash
     ./tools/build_userland.sh clean <pkg>
     ./tools/build_userland.sh build <pkg>
     ```

---

## 5. Automated Testing

PetraOS includes an automated testing harness to run commands inside QEMU and check results:

```bash
# Run a single command test in PetraOS
python3 tools/test_cli.py --cmd "gcc --version" --expect "gcc"

# Run default automated CLI test suite
python3 tools/test_cli.py --test-file .agents/skills/cli-testing/scripts/default_tests.json
```
Reports and failure screenshots are saved under `test_reports/`.
