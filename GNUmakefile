# Nuke built-in rules and variables.
MAKEFLAGS += -rR
.SUFFIXES:
unexport MAKEFLAGS

# Convenience macro to reliably declare user overridable variables.
override USER_VARIABLE = $(if $(filter $(origin $(1)),default undefined),$(eval override $(1) := $(2)))

# Target architecture to build for. Default to x86_64.
$(call USER_VARIABLE,KARCH,x86_64)

# Default user QEMU flags. These are appended to the QEMU command calls.
$(call USER_VARIABLE,QEMUFLAGS,-m 2G -serial stdio)

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

# Userspace / Ports build system (xbstrap)
$(call USER_VARIABLE,XBSTRAP,xbstrap)

.PHONY: xbstrap-init
xbstrap-init:
	@mkdir -p $(BUILD_DIR_XBSTRAP)
	@if [ ! -f $(BUILD_DIR_XBSTRAP)/bootstrap.link ]; then \
		(cd $(BUILD_DIR_XBSTRAP) && $(XBSTRAP) init ..); \
	fi

.PHONY: mlibc-headers
mlibc-headers: xbstrap-init
	(cd $(BUILD_DIR_XBSTRAP) && $(XBSTRAP) install mlibc-headers)

.PHONY: mlibc
mlibc: xbstrap-init
	(cd $(BUILD_DIR_XBSTRAP) && $(XBSTRAP) install mlibc)

.PHONY: bash
bash: xbstrap-init mlibc
	(cd $(BUILD_DIR_XBSTRAP) && $(XBSTRAP) install bash)

.PHONY: ncurses
ncurses: xbstrap-init mlibc
	(cd $(BUILD_DIR_XBSTRAP) && $(XBSTRAP) install ncurses)

.PHONY: readline
readline: xbstrap-init mlibc ncurses
	(cd $(BUILD_DIR_XBSTRAP) && $(XBSTRAP) install readline)

.PHONY: automake
automake: xbstrap-init mlibc
	(cd $(BUILD_DIR_XBSTRAP) && $(XBSTRAP) install automake)

.PHONY: libtool
libtool: xbstrap-init mlibc
	(cd $(BUILD_DIR_XBSTRAP) && $(XBSTRAP) install libtool)

.PHONY: binutils
binutils: xbstrap-init mlibc
	(cd $(BUILD_DIR_XBSTRAP) && $(XBSTRAP) install binutils)

.PHONY: gcc
gcc: xbstrap-init mlibc binutils
	(cd $(BUILD_DIR_XBSTRAP) && $(XBSTRAP) install gcc)

.PHONY: coreutils
coreutils: xbstrap-init mlibc tzdata
	(cd $(BUILD_DIR_XBSTRAP) && $(XBSTRAP) install coreutils)

.PHONY: tzdata
tzdata: xbstrap-init mlibc
	(cd $(BUILD_DIR_XBSTRAP) && $(XBSTRAP) install tzdata)

.PHONY: pkg-config
pkg-config: xbstrap-init mlibc
	(cd $(BUILD_DIR_XBSTRAP) && $(XBSTRAP) install pkg-config)

.PHONY: xbstrap-fetch
xbstrap-fetch: xbstrap-init
	(cd $(BUILD_DIR_XBSTRAP) && $(XBSTRAP) fetch --all)

.PHONY: userspace
userspace: xbstrap-init mlibc bash sync-initramfs

.PHONY: userspace-all
userspace-all: xbstrap-init xbstrap-fetch mlibc ncurses readline bash automake libtool binutils gcc coreutils tzdata pkg-config sync-initramfs

.PHONY: sync-initramfs
sync-initramfs:
	@mkdir -p $(INITRAMFS_ROOT)/bin $(INITRAMFS_ROOT)/sbin $(INITRAMFS_ROOT)/lib $(INITRAMFS_ROOT)/usr/bin $(INITRAMFS_ROOT)/usr/lib
	@if [ -d $(SYSROOT)/usr/lib ]; then \
		cp -rf $(SYSROOT)/usr/lib/*.so* $(INITRAMFS_ROOT)/lib/ 2>/dev/null || true; \
		cp -rf $(SYSROOT)/usr/lib/*.so* $(INITRAMFS_ROOT)/usr/lib/ 2>/dev/null || true; \
	fi
	@if [ -d $(SYSROOT)/usr/bin ]; then \
		cp -rf $(SYSROOT)/usr/bin/* $(INITRAMFS_ROOT)/usr/bin/ 2>/dev/null || true; \
		cp -rf $(SYSROOT)/usr/bin/* $(INITRAMFS_ROOT)/bin/ 2>/dev/null || true; \
	fi

.PHONY: clean-userspace
clean-userspace:
	rm -rf $(BUILD_DIR_XBSTRAP)

.PHONY: initramfs
initramfs: userspace $(INITRAMFS_CPIO)

.PHONY: run-userspace
run-userspace: userspace $(INITRAMFS_CPIO) run

.PHONY: run-userspace-all
run-userspace-all: userspace-all $(INITRAMFS_CPIO) run

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
ifeq ($(KARCH),riscv64)
	cp -v $(LIMINE_DIR)/limine-uefi-cd.bin $(ISO_ROOT)/boot/limine/
	cp -v $(LIMINE_DIR)/BOOTRISCV64.EFI $(ISO_ROOT)/EFI/BOOT/
	xorriso -as mkisofs \
		--efi-boot boot/limine/limine-uefi-cd.bin \
		-efi-boot-part --efi-boot-image --protective-msdos-label \
		$(ISO_ROOT) -o $(IMAGE_NAME).iso
endif
ifeq ($(KARCH),loongarch64)
	cp -v $(LIMINE_DIR)/limine-uefi-cd.bin $(ISO_ROOT)/boot/limine/
	cp -v $(LIMINE_DIR)/BOOTLOONGARCH64.EFI $(ISO_ROOT)/EFI/BOOT/
	xorriso -as mkisofs \
		--efi-boot boot/limine/limine-uefi-cd.bin \
		-efi-boot-part --efi-boot-image --protective-msdos-label \
		$(ISO_ROOT) -o $(IMAGE_NAME).iso
endif
	rm -rf $(ISO_ROOT)

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
ifeq ($(KARCH),riscv64)
	mcopy -i $(IMAGE_NAME).hdd@@1M $(LIMINE_DIR)/BOOTRISCV64.EFI ::/EFI/BOOT
endif
ifeq ($(KARCH),loongarch64)
	mcopy -i $(IMAGE_NAME).hdd@@1M $(LIMINE_DIR)/BOOTLOONGARCH64.EFI ::/EFI/BOOT
endif

.PHONY: clean
clean:
	$(MAKE) -C kernel clean
	rm -rf $(ISO_ROOT) $(IMAGE_NAME).iso $(IMAGE_NAME).hdd $(INITRAMFS_CPIO) $(INITRAMFS_ROOT)

.PHONY: distclean
distclean: clean
	$(MAKE) -C kernel distclean
	rm -rf $(BUILD_DIR) $(BUILD_DIR_XBSTRAP) .xbstrap $(OVMF_DIR)
