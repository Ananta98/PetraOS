# This is the name that our final executable will have.
# Change as needed.
override OUTPUT := kernel

# Convenience macro to reliably declare user overridable variables.
override USER_VARIABLE = $(if $(filter $(origin $(1)),default undefined),$(eval override $(1) := $(2)))

# Target architecture to build for. Default to x86_64.
$(call USER_VARIABLE,KARCH,x86_64)

ifeq ($(RUST_TARGET),)
    override RUST_TARGET := $(KARCH)-unknown-none
	ifeq ($(KARCH),riscv64)
    	override RUST_TARGET := riscv64gc-unknown-none-elf
	endif
endif

ifeq ($(RUST_PROFILE),)
    override RUST_PROFILE := dev
endif

override RUST_PROFILE_SUBDIR := $(RUST_PROFILE)
ifeq ($(RUST_PROFILE),dev)
    override RUST_PROFILE_SUBDIR := debug
endif

# Default target.
.PHONY: all
all:
	RUSTFLAGS="-C relocation-model=static" cargo build --target $(RUST_TARGET) --profile $(RUST_PROFILE) --features binary-only
	cp target/$(RUST_TARGET)/$(RUST_PROFILE_SUBDIR)/petraos-kernel kernel

# Remove object files and the final executable.
.PHONY: clean
clean:
	cargo clean
	rm -rf kernel

.PHONY: distclean
distclean: clean

# ------------------------------------------------------------------------------
# Userspace / xbstrap helpers (delegates to ../tools/build_and_run_userspace.sh)
# ------------------------------------------------------------------------------
# Full userspace builds (fetch + patch + compile + install) can take a very
# long time (gcc, mlibc, ncurses, htop, etc.). These make targets wrap the
# central userspace script so users can run a single make command:
#
#   make build-userspace        - fetch & build ALL packages (including htop)
#   make build-userspace-htop   - build only htop (and its deps)
#   make fetch-userspace        - fetch all package sources (no compile)
#   make clean-userspace        - clean xbstrap workspace (full rebuild)
#   make userspace-status       - show workspace & htop package status
#
# All targets delegate to tools/build_and_run_userspace.sh, which handles
# xbstrap workspace init, fetching, patching, building and sysroot
# installation. Using make here avoids remembering the long script path.
# ------------------------------------------------------------------------------
USERSPACE_SCRIPT := $(abspath $(dir $(lastword $(MAKEFILE_LIST)))../tools/build_and_run_userspace.sh)

.PHONY: build-userspace
build-userspace:
	@echo "==> Building all userspace packages (this may take a long time)..."
	@bash $(USERSPACE_SCRIPT) build-all

.PHONY: build-userspace-core
build-userspace-core:
	@echo "==> Building core userspace (mlibc, bash, coreutils)..."
	@bash $(USERSPACE_SCRIPT) build mlibc
	@bash $(USERSPACE_SCRIPT) build bash
	@bash $(USERSPACE_SCRIPT) build coreutils

.PHONY: fetch-userspace
fetch-userspace:
	@bash $(USERSPACE_SCRIPT) fetch --all

.PHONY: compile-userspace
compile-userspace: build-userspace

.PHONY: install-userspace
install-userspace: build-userspace

.PHONY: clean-userspace
clean-userspace:
	@bash $(USERSPACE_SCRIPT) clean --all

.PHONY: userspace-status
userspace-status:
	@bash $(USERSPACE_SCRIPT) status htop

# ------------------------------------------------------------------------------
# Initramfs helpers (delegates to ../tools/create_initramfs.sh)
# ------------------------------------------------------------------------------
INITRAMFS_SCRIPT := $(abspath $(dir $(lastword $(MAKEFILE_LIST)))../tools/create_initramfs.sh)

.PHONY: sync-initramfs sync_ramfs
sync-initramfs sync_ramfs:
	@bash $(INITRAMFS_SCRIPT) --sync-only

.PHONY: initramfs
initramfs:
	@bash $(INITRAMFS_SCRIPT)

