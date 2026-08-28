# Nuke built-in rules and variables.
MAKEFLAGS += -rR
.SUFFIXES:
unexport MAKEFLAGS

# Convenience macro to reliably declare user overridable variables.
override USER_VARIABLE = $(if $(filter $(origin $(1)),default undefined),$(eval override $(1) := $(2)))

# Target architecture to build for. Default to x86_64.
$(call USER_VARIABLE,KARCH,x86_64)

# Default user QEMU flags. These are appended to the QEMU command calls.
QEMU_KVM_FLAGS := $(shell [ -w /dev/kvm ] && echo "-enable-kvm -cpu host" || echo "")
$(call USER_VARIABLE,QEMUFLAGS,-m 2G -serial stdio $(QEMU_KVM_FLAGS))

# Output and directory layout configuration
BUILD_DIR ?= build
BUILD_DIR_XBSTRAP := build-xbstrap
SYSROOT := $(BUILD_DIR_XBSTRAP)/system-root
CONFIG_DIR := config
INITRAMFS_ROOT := $(BUILD_DIR)/initramfs_root
INITRAMFS_CPIO := $(BUILD_DIR)/initramfs.cpio
ISO_ROOT := $(BUILD_DIR)/iso_root
LIMINE_DIR := $(BUILD_DIR)/limine
OVMF_DIR := $(CONFIG_DIR)/edk2-ovmf

override IMAGE_NAME := $(BUILD_DIR)/PetraOS-$(KARCH)

.PHONY: all
all: $(IMAGE_NAME).iso

.PHONY: all-hdd
all-hdd: $(IMAGE_NAME).hdd

.PHONY: run
run: run-$(KARCH)

# ==============================================================================
# Userspace / Ports
#
# All xbstrap operations (workspace init, source fetching, patching, package
# compilation and cleaning) live in tools/build_and_run_userspace.sh:
#   ./tools/build_and_run_userspace.sh            # core userspace pipeline + QEMU
#   ./tools/build_and_run_userspace.sh --all      # recompile ALL packages + QEMU
#
# For convenience, long-running operations are also exposed as make targets:
#   make build-userspace      - fetch + compile + install ALL packages (incl. htop)
#   make fetch-userspace      - fetch all sources only (no compile)
#   make compile-userspace    - alias for build-userspace
#   make install-userspace    - alias for build-userspace
#   make clean-userspace      - clean xbstrap workspace
#   make build-userspace-htop - build only htop
# See also kernel/GNUmakefile for the same targets when running `make -C kernel`.
# ==============================================================================

.PHONY: build-userspace
build-userspace:
	@echo "==> Building all userspace packages (this may take a long time)..."
	@bash tools/build_and_run_userspace.sh build-all

.PHONY: build-userspace-htop
build-userspace-htop:
	@bash tools/build_and_run_userspace.sh build htop

.PHONY: fetch-userspace
fetch-userspace:
	@bash tools/build_and_run_userspace.sh fetch --all

.PHONY: compile-userspace
compile-userspace: build-userspace

.PHONY: install-userspace
install-userspace: build-userspace

.PHONY: clean-userspace
clean-userspace:
	@bash tools/build_and_run_userspace.sh clean --all

.PHONY: sync-initramfs
sync-initramfs:
	@mkdir -p $(INITRAMFS_ROOT)/bin $(INITRAMFS_ROOT)/sbin $(INITRAMFS_ROOT)/lib $(INITRAMFS_ROOT)/usr/bin $(INITRAMFS_ROOT)/usr/lib $(INITRAMFS_ROOT)/usr/sbin $(INITRAMFS_ROOT)/usr/libexec $(INITRAMFS_ROOT)/usr/include $(INITRAMFS_ROOT)/usr/share $(INITRAMFS_ROOT)/etc
	@if [ -d $(SYSROOT)/lib ]; then cp -rf $(SYSROOT)/lib/* $(INITRAMFS_ROOT)/lib/ 2>/dev/null || true; fi
	@if [ -d $(SYSROOT)/usr/lib ]; then \
		cp -rf $(SYSROOT)/usr/lib/* $(INITRAMFS_ROOT)/usr/lib/ 2>/dev/null || true; \
		cp -rf $(SYSROOT)/usr/lib/*.so* $(INITRAMFS_ROOT)/lib/ 2>/dev/null || true; \
	fi
	@if [ -d $(SYSROOT)/usr/libexec ]; then \
		cp -rf $(SYSROOT)/usr/libexec/* $(INITRAMFS_ROOT)/usr/libexec/ 2>/dev/null || true; \
	fi
	@if [ -d $(SYSROOT)/usr/include ]; then \
		cp -rf $(SYSROOT)/usr/include/* $(INITRAMFS_ROOT)/usr/include/ 2>/dev/null || true; \
	fi
	@if [ -d $(SYSROOT)/usr/share ]; then \
		cp -rf $(SYSROOT)/usr/share/* $(INITRAMFS_ROOT)/usr/share/ 2>/dev/null || true; \
	fi
	@if [ -d /usr/share/terminfo ]; then \
		mkdir -p $(INITRAMFS_ROOT)/usr/share/terminfo $(INITRAMFS_ROOT)/etc/terminfo; \
		cp -rf /usr/share/terminfo/* $(INITRAMFS_ROOT)/usr/share/terminfo/ 2>/dev/null || true; \
		cp -rf /usr/share/terminfo/* $(INITRAMFS_ROOT)/etc/terminfo/ 2>/dev/null || true; \
	fi
	@if [ -d $(SYSROOT)/bin ]; then cp -rf $(SYSROOT)/bin/* $(INITRAMFS_ROOT)/bin/ 2>/dev/null || true; fi
	@if [ -d $(SYSROOT)/sbin ]; then cp -rf $(SYSROOT)/sbin/* $(INITRAMFS_ROOT)/sbin/ 2>/dev/null || true; fi
	@if [ -d $(SYSROOT)/usr/bin ]; then \
		cp -rf $(SYSROOT)/usr/bin/* $(INITRAMFS_ROOT)/usr/bin/ 2>/dev/null || true; \
		cp -rf $(SYSROOT)/usr/bin/* $(INITRAMFS_ROOT)/bin/ 2>/dev/null || true; \
	fi
	@if [ -d $(SYSROOT)/usr/sbin ]; then \
		cp -rf $(SYSROOT)/usr/sbin/* $(INITRAMFS_ROOT)/usr/sbin/ 2>/dev/null || true; \
		cp -rf $(SYSROOT)/usr/sbin/* $(INITRAMFS_ROOT)/sbin/ 2>/dev/null || true; \
	fi
	@if [ -d $(SYSROOT)/etc ]; then cp -rf $(SYSROOT)/etc/* $(INITRAMFS_ROOT)/etc/ 2>/dev/null || true; fi
	@if [ -d $(INITRAMFS_ROOT)/bin ]; then \
		find $(INITRAMFS_ROOT)/bin $(INITRAMFS_ROOT)/usr/bin $(INITRAMFS_ROOT)/usr/libexec -type f -exec x86_64-linux-gnu-strip -s {} + 2>/dev/null || true; \
	fi
	@if [ -d $(INITRAMFS_ROOT)/lib ]; then \
		find $(INITRAMFS_ROOT)/lib $(INITRAMFS_ROOT)/usr/lib -name "*.so*" -type f -exec x86_64-linux-gnu-strip -s {} + 2>/dev/null || true; \
	fi

.PHONY: initramfs
initramfs: $(INITRAMFS_CPIO)

$(INITRAMFS_CPIO): sync-initramfs tools/create_initramfs.sh $(shell find $(INITRAMFS_ROOT) -type f 2>/dev/null)
	@mkdir -p $(BUILD_DIR)
	@if [ -f tools/create_initramfs.sh ]; then \
		chmod +x tools/create_initramfs.sh && ./tools/create_initramfs.sh $(INITRAMFS_ROOT) $(INITRAMFS_CPIO); \
	fi

.PHONY: run-hdd
run-hdd: run-hdd-$(KARCH)

.PHONY: run-x86_64
run-x86_64: $(OVMF_DIR) $(IMAGE_NAME).iso
	qemu-system-$(KARCH) \
		-M q35 \
		-smp 4 \
		-drive if=pflash,unit=0,format=raw,file=$(OVMF_DIR)/ovmf-code-$(KARCH).fd,readonly=on \
		-cdrom $(IMAGE_NAME).iso \
		$(QEMUFLAGS)

.PHONY: run-hdd-x86_64
run-hdd-x86_64: $(OVMF_DIR) $(IMAGE_NAME).hdd
	qemu-system-$(KARCH) \
		-M q35 \
		-smp 4 \
		-drive if=pflash,unit=0,format=raw,file=$(OVMF_DIR)/ovmf-code-$(KARCH).fd,readonly=on \
		-hda $(IMAGE_NAME).hdd \
		$(QEMUFLAGS)

.PHONY: run-aarch64
run-aarch64: $(OVMF_DIR) $(IMAGE_NAME).iso
	qemu-system-$(KARCH) \
		-M virt \
		-cpu cortex-a72 \
		-device ramfb \
		-device qemu-xhci \
		-device usb-kbd \
		-device usb-mouse \
		-drive if=pflash,unit=0,format=raw,file=$(OVMF_DIR)/ovmf-code-$(KARCH).fd,readonly=on \
		-cdrom $(IMAGE_NAME).iso \
		$(QEMUFLAGS)

.PHONY: run-hdd-aarch64
run-hdd-aarch64: $(OVMF_DIR) $(IMAGE_NAME).hdd
	qemu-system-$(KARCH) \
		-M virt \
		-cpu cortex-a72 \
		-device ramfb \
		-device qemu-xhci \
		-device usb-kbd \
		-device usb-mouse \
		-drive if=pflash,unit=0,format=raw,file=$(OVMF_DIR)/ovmf-code-$(KARCH).fd,readonly=on \
		-hda $(IMAGE_NAME).hdd \
		$(QEMUFLAGS)

.PHONY: run-bios
run-bios: $(IMAGE_NAME).iso
	qemu-system-$(KARCH) \
		-M q35 \
		-cdrom $(IMAGE_NAME).iso \
		-boot d \
		$(QEMUFLAGS)

.PHONY: run-hdd-bios
run-hdd-bios: $(IMAGE_NAME).hdd
	qemu-system-$(KARCH) \
		-M q35 \
		-hda $(IMAGE_NAME).hdd \
		$(QEMUFLAGS)

$(OVMF_DIR):
	@mkdir -p $(CONFIG_DIR)
	curl -L https://github.com/osdev0/edk2-ovmf-nightly/releases/latest/download/edk2-ovmf.tar.gz | gunzip | tar -xf - -C $(CONFIG_DIR)

$(LIMINE_DIR)/limine:
	@mkdir -p $(BUILD_DIR)
	rm -rf $(LIMINE_DIR)
	git clone https://github.com/limine-bootloader/limine.git --branch=v10.x-binary --depth=1 $(LIMINE_DIR)
	$(MAKE) -C $(LIMINE_DIR)

.PHONY: kernel
kernel:
	$(MAKE) -C kernel

$(IMAGE_NAME).iso: $(LIMINE_DIR)/limine kernel $(INITRAMFS_CPIO)
	@mkdir -p $(BUILD_DIR)
	rm -rf $(ISO_ROOT)
	mkdir -p $(ISO_ROOT)/boot
	cp -v kernel/kernel $(ISO_ROOT)/boot/
	if [ -f $(INITRAMFS_CPIO) ]; then cp -v $(INITRAMFS_CPIO) $(ISO_ROOT)/boot/; fi
	mkdir -p $(ISO_ROOT)/boot/limine
	cp -v $(CONFIG_DIR)/limine.conf $(ISO_ROOT)/boot/limine/
	mkdir -p $(ISO_ROOT)/EFI/BOOT
ifeq ($(KARCH),x86_64)
	cp -v $(LIMINE_DIR)/limine-bios.sys $(LIMINE_DIR)/limine-bios-cd.bin $(LIMINE_DIR)/limine-uefi-cd.bin $(ISO_ROOT)/boot/limine/
	cp -v $(LIMINE_DIR)/BOOTX64.EFI $(ISO_ROOT)/EFI/BOOT/
	cp -v $(LIMINE_DIR)/BOOTIA32.EFI $(ISO_ROOT)/EFI/BOOT/
	xorriso -as mkisofs -b boot/limine/limine-bios-cd.bin \
		-no-emul-boot -boot-load-size 4 -boot-info-table \
		--efi-boot boot/limine/limine-uefi-cd.bin \
		-efi-boot-part --efi-boot-image --protective-msdos-label \
		$(ISO_ROOT) -o $(IMAGE_NAME).iso
	./$(LIMINE_DIR)/limine bios-install $(IMAGE_NAME).iso
endif
ifeq ($(KARCH),aarch64)
	cp -v $(LIMINE_DIR)/limine-uefi-cd.bin $(ISO_ROOT)/boot/limine/
	cp -v $(LIMINE_DIR)/BOOTAA64.EFI $(ISO_ROOT)/EFI/BOOT/
	xorriso -as mkisofs \
		--efi-boot boot/limine/limine-uefi-cd.bin \
		-efi-boot-part --efi-boot-image --protective-msdos-label \
		$(ISO_ROOT) -o $(IMAGE_NAME).iso
endif

$(IMAGE_NAME).hdd: $(LIMINE_DIR)/limine kernel $(INITRAMFS_CPIO)
	@mkdir -p $(BUILD_DIR)
	rm -f $(IMAGE_NAME).hdd
	dd if=/dev/zero bs=1M count=0 seek=64 of=$(IMAGE_NAME).hdd
	sgdisk $(IMAGE_NAME).hdd -n 1:2048 -t 1:ef00
ifeq ($(KARCH),x86_64)
	./$(LIMINE_DIR)/limine bios-install $(IMAGE_NAME).hdd
endif
	mformat -i $(IMAGE_NAME).hdd@@1M
	mmd -i $(IMAGE_NAME).hdd@@1M ::/EFI ::/EFI/BOOT ::/boot ::/boot/limine
	mcopy -i $(IMAGE_NAME).hdd@@1M kernel/kernel ::/boot
	if [ -f $(INITRAMFS_CPIO) ]; then mcopy -i $(IMAGE_NAME).hdd@@1M $(INITRAMFS_CPIO) ::/boot; fi
	mcopy -i $(IMAGE_NAME).hdd@@1M $(CONFIG_DIR)/limine.conf ::/boot/limine
ifeq ($(KARCH),x86_64)
	mcopy -i $(IMAGE_NAME).hdd@@1M $(LIMINE_DIR)/limine-bios.sys ::/boot/limine
	mcopy -i $(IMAGE_NAME).hdd@@1M $(LIMINE_DIR)/BOOTX64.EFI ::/EFI/BOOT
	mcopy -i $(IMAGE_NAME).hdd@@1M $(LIMINE_DIR)/BOOTIA32.EFI ::/EFI/BOOT
endif
ifeq ($(KARCH),aarch64)
	mcopy -i $(IMAGE_NAME).hdd@@1M $(LIMINE_DIR)/BOOTAA64.EFI ::/EFI/BOOT
	mcopy -i $(IMAGE_NAME).hdd@@1M $(LIMINE_DIR)/BOOTAA64.EFI ::/EFI/BOOT
endif

.PHONY: clean
clean:
	$(MAKE) -C kernel clean
	rm -rf $(ISO_ROOT) $(IMAGE_NAME).iso $(IMAGE_NAME).hdd $(INITRAMFS_CPIO) $(INITRAMFS_ROOT)

.PHONY: distclean
distclean: clean
	$(MAKE) -C kernel distclean
	rm -rf $(BUILD_DIR) $(BUILD_DIR_XBSTRAP) .xbstrap $(OVMF_DIR)
