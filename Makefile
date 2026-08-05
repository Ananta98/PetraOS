# ==============================================================================
# Petra OS - musl libc & GNU Bash Integration Makefile
# ==============================================================================
# Utilize musl libc (v1.2.5) for a full POSIX C library.
# musl provides standard C library functions targeting Linux syscall ABI.
# ==============================================================================

MUSL_VERSION   ?= 1.2.5
MUSL_TARBALL   := musl-$(MUSL_VERSION).tar.gz
MUSL_URL       := https://musl.libc.org/releases/$(MUSL_TARBALL)

BASH_VERSION   ?= 5.2.21
BASH_TARBALL   := bash-$(BASH_VERSION).tar.gz
BASH_URL       := https://mirrors.kernel.org/gnu/bash/$(BASH_TARBALL)

BUSYBOX_VERSION ?= 1.36.1
BUSYBOX_TARBALL := busybox-$(BUSYBOX_VERSION).tar.bz2
BUSYBOX_URL     := https://busybox.net/downloads/$(BUSYBOX_TARBALL)

# Cross-compiler: prefer x86_64-linux-gnu toolchain, fall back to native gcc
CC     := $(shell which x86_64-linux-gnu-gcc 2>/dev/null || echo gcc)
CXX    := $(shell which x86_64-linux-gnu-g++ 2>/dev/null || echo g++)
AR     := $(shell which x86_64-linux-gnu-ar  2>/dev/null || echo ar)
RANLIB := $(shell which x86_64-linux-gnu-ranlib 2>/dev/null || echo ranlib)
STRIP  := $(shell which x86_64-linux-gnu-strip   2>/dev/null || echo strip)

HOST_TRIPLE  := x86_64-petra-linux
BUILD_TRIPLE := $(shell gcc -dumpmachine)

ROOT_DIR       := $(shell pwd)
DOWNLOAD_DIR   := $(ROOT_DIR)/downloads
PATCHES_DIR    := $(ROOT_DIR)/patches
BUILD_DIR      := $(ROOT_DIR)/build
SRC_DIR        := $(BUILD_DIR)/src
SYSROOT        := $(BUILD_DIR)/sysroot
INITRAMFS_DIR  := $(ROOT_DIR)/initramfs
INITRAMFS_CPIO := $(BUILD_DIR)/initramfs.cpio

MUSL_SRC       := $(SRC_DIR)/musl-$(MUSL_VERSION)
MUSL_BUILD     := $(BUILD_DIR)/musl

BASH_SRC       := $(SRC_DIR)/bash-$(BASH_VERSION)
BASH_BUILD     := $(BUILD_DIR)/bash

BUSYBOX_SRC     := $(SRC_DIR)/busybox-$(BUSYBOX_VERSION)
BUSYBOX_BUILD   := $(BUILD_DIR)/busybox

# Flags used when cross-compiling bash against the musl sysroot.
SYSROOT_CFLAGS  := -O2 -g -isystem $(SYSROOT)/include -D_GNU_SOURCE -D_POSIX_C_SOURCE=200809L
SYSROOT_LDFLAGS := -static -L$(SYSROOT)/lib -lc -lgcc

# Autoconf cache overrides for bash cross-configure.
CROSS_ENV := \
	CC="$(CC)" \
	CXX="$(CXX)" \
	AR="$(AR)" \
	RANLIB="$(RANLIB)" \
	CFLAGS="$(SYSROOT_CFLAGS)" \
	LDFLAGS="$(SYSROOT_LDFLAGS)" \
	ac_cv_header_dirent_h=yes \
	ac_cv_func_opendir=yes \
	ac_cv_func_closedir=yes \
	ac_cv_func_readdir=yes \
	ac_cv_func_gethostname=yes \
	ac_cv_func_sigsetjmp=yes \
	ac_cv_type_sigjmp_buf=yes \
	ac_cv_func_sigaction=yes \
	ac_cv_func_sigprocmask=yes \
	ac_cv_func_mkfifo=yes \
	ac_cv_func_dup2=yes \
	ac_cv_func_fcntl=yes \
	ac_cv_func_isatty=yes \
	ac_cv_func_lseek=yes \
	ac_cv_func_open=yes \
	ac_cv_func_close=yes \
	ac_cv_func_read=yes \
	ac_cv_func_write=yes \
	ac_cv_func_getcwd=yes \
	ac_cv_func_getpwuid=yes \
	ac_cv_func_getgroups=yes \
	ac_cv_type_intmax_t=yes \
	ac_cv_type_uintmax_t=yes \
	ac_cv_sizeof_char=1 \
	ac_cv_sizeof_short=2 \
	ac_cv_sizeof_int=4 \
	ac_cv_sizeof_long=8 \
	ac_cv_sizeof_char_p=8 \
	ac_cv_sizeof_size_t=8 \
	ac_cv_sizeof_double=8 \
	ac_cv_sizeof_long_long=8 \
	ac_cv_sizeof_intmax_t=8 \
	ac_cv_sizeof_uintmax_t=8 \
	ac_cv_sizeof_off_t=8 \
	ac_cv_sizeof_pid_t=4 \
	ac_cv_sizeof_uid_t=4 \
	ac_cv_sizeof_gid_t=4 \
	ac_cv_sizeof_time_t=8 \
	ac_cv_func_strtoimax=yes \
	ac_cv_func_strtoumax=yes \
	bash_cv_posix_signals=yes \
	bash_cv_getcwd_malloc=yes \
	bash_cv_func_sigsetjmp=present \
	bash_cv_must_relink_at_exec=no \
	bash_cv_sys_named_pipes=present \
	bash_cv_dup2_clamper=yes \
	bash_cv_job_control_missing=present \
	bash_cv_sys_restartable_syscalls=yes \
	bash_cv_wcwidth_broken=no \
	bash_cv_func_strcoll_broken=no

.PHONY: all check-tools download extract patch configure build install busybox-configure busybox-build bash-install run clean

all: install

# ------------------------------------------------------------------------------
# 0. Validate required build tools
# ------------------------------------------------------------------------------
check-tools:
	@command -v $(CC) >/dev/null 2>&1 || { echo "ERROR: $(CC) not found"; exit 1; }
	@command -v make >/dev/null 2>&1 || { echo "ERROR: make not found"; exit 1; }
	@echo "==> Tool check passed (cc=$(CC))"

# ------------------------------------------------------------------------------
# 1. Download musl libc, GNU Bash & Busybox Tarballs
# ------------------------------------------------------------------------------
download: check-tools
	@mkdir -p $(DOWNLOAD_DIR) $(SRC_DIR) $(PATCHES_DIR) $(BUILD_DIR) $(SYSROOT) $(INITRAMFS_DIR)
	@if [ ! -f $(DOWNLOAD_DIR)/$(MUSL_TARBALL) ] || [ ! -s $(DOWNLOAD_DIR)/$(MUSL_TARBALL) ]; then \
		echo "==> Downloading musl libc $(MUSL_VERSION)..."; \
		curl -fSL $(MUSL_URL) -o $(DOWNLOAD_DIR)/$(MUSL_TARBALL); \
	fi
	@if [ ! -f $(DOWNLOAD_DIR)/$(BASH_TARBALL) ] || [ ! -s $(DOWNLOAD_DIR)/$(BASH_TARBALL) ]; then \
		echo "==> Downloading GNU Bash $(BASH_VERSION)..."; \
		curl -fSL $(BASH_URL) -o $(DOWNLOAD_DIR)/$(BASH_TARBALL); \
	fi
	@if [ ! -f $(DOWNLOAD_DIR)/$(BUSYBOX_TARBALL) ] || [ ! -s $(DOWNLOAD_DIR)/$(BUSYBOX_TARBALL) ]; then \
		echo "==> Downloading Busybox $(BUSYBOX_VERSION)..."; \
		curl -fSL $(BUSYBOX_URL) -o $(DOWNLOAD_DIR)/$(BUSYBOX_TARBALL); \
	fi

# ------------------------------------------------------------------------------
# 2. Extract Sources
# ------------------------------------------------------------------------------
extract: download
	@if [ ! -f $(MUSL_SRC)/configure ]; then \
		echo "==> Extracting musl libc $(MUSL_VERSION)..."; \
		rm -rf $(MUSL_SRC); \
		tar -xzf $(DOWNLOAD_DIR)/$(MUSL_TARBALL) -C $(SRC_DIR); \
	fi
	@if [ ! -f $(BASH_SRC)/support/config.sub ]; then \
		echo "==> Extracting GNU Bash $(BASH_VERSION)..."; \
		rm -rf $(BASH_SRC); \
		tar -xzf $(DOWNLOAD_DIR)/$(BASH_TARBALL) -C $(SRC_DIR); \
		chmod -R +w $(BASH_SRC); \
	fi
	@if [ ! -f $(BUSYBOX_SRC)/Makefile ]; then \
		echo "==> Extracting Busybox $(BUSYBOX_VERSION)..."; \
		rm -rf $(BUSYBOX_SRC); \
		tar -xjf $(DOWNLOAD_DIR)/$(BUSYBOX_TARBALL) -C $(SRC_DIR); \
	fi

# ------------------------------------------------------------------------------
# 3. Patch GNU Bash (petra* OS recognition)
# ------------------------------------------------------------------------------
patch: extract
	@echo "==> Applying bash petra-OS config patch..."
	@grep -q "bash_cv_func_strtoimax = no" $(BASH_SRC)/configure || \
		patch -p1 -d $(BASH_SRC) < $(PATCHES_DIR)/0001-Petra-OS-bash-port.patch

# ------------------------------------------------------------------------------
# 4. Configure musl libc
# ------------------------------------------------------------------------------
configure: patch
	@if [ ! -f $(MUSL_BUILD)/Makefile ]; then \
		echo "==> Configuring musl libc $(MUSL_VERSION)..."; \
		mkdir -p $(MUSL_BUILD); \
		cd $(MUSL_BUILD) && CC="$(CC)" $(MUSL_SRC)/configure \
			--prefix=$(SYSROOT) \
			--exec-prefix=$(SYSROOT) \
			--syslibdir=$(SYSROOT)/lib \
			--includedir=$(SYSROOT)/include \
			--disable-shared \
			--enable-static; \
	fi

# ------------------------------------------------------------------------------
# 5. Build musl static library
# ------------------------------------------------------------------------------
build: configure
	@echo "==> Compiling musl libc..."
	@$(MAKE) -C $(MUSL_BUILD)

# ------------------------------------------------------------------------------
# 6. Install musl libc headers & static library into sysroot
# ------------------------------------------------------------------------------
install: build
	@echo "==> Installing musl libc into sysroot ($(SYSROOT))..."
	@$(MAKE) -C $(MUSL_BUILD) install
	@ls -la $(SYSROOT)/lib/libc.a 2>/dev/null || { echo "ERROR: libc.a not found in $(SYSROOT)/lib after install"; exit 1; }
	@echo "==> musl libc sysroot ready."

# ------------------------------------------------------------------------------
# 7. Configure, Build & Install Busybox into Initramfs
# ------------------------------------------------------------------------------
busybox-configure: install
	@if [ ! -f $(BUSYBOX_SRC)/.config ]; then \
		echo "==> Configuring Busybox $(BUSYBOX_VERSION)..."; \
		$(MAKE) -C $(BUSYBOX_SRC) defconfig; \
		sed -i 's/# CONFIG_STATIC is not set/CONFIG_STATIC=y/' $(BUSYBOX_SRC)/.config; \
		sed -i 's/CONFIG_TC=y/# CONFIG_TC is not set/' $(BUSYBOX_SRC)/.config; \
		sed -i 's/# CONFIG_BASH_IS_ASH is not set/CONFIG_BASH_IS_ASH=y/' $(BUSYBOX_SRC)/.config; \
		sed -i 's/# CONFIG_ASH_BASH_COMPAT is not set/CONFIG_ASH_BASH_COMPAT=y/' $(BUSYBOX_SRC)/.config; \
	fi

busybox-build: busybox-configure
	@if [ ! -f $(INITRAMFS_DIR)/bin/busybox ]; then \
		echo "==> Compiling Busybox $(BUSYBOX_VERSION)..."; \
		$(MAKE) -C $(BUSYBOX_SRC) CC="$(CC)" CFLAGS="$(SYSROOT_CFLAGS)" LDFLAGS="$(SYSROOT_LDFLAGS)"; \
		echo "==> Installing Busybox into initramfs..."; \
		mkdir -p $(INITRAMFS_DIR)/bin $(INITRAMFS_DIR)/sbin $(INITRAMFS_DIR)/usr/bin $(INITRAMFS_DIR)/usr/sbin; \
		$(MAKE) -C $(BUSYBOX_SRC) CC="$(CC)" CFLAGS="$(SYSROOT_CFLAGS)" LDFLAGS="$(SYSROOT_LDFLAGS)" CONFIG_PREFIX=$(INITRAMFS_DIR) install; \
	fi

# ------------------------------------------------------------------------------
# 8. Configure, Build & Install GNU Bash into Initramfs
# ------------------------------------------------------------------------------
bash-configure: install
	@if [ ! -f $(BASH_BUILD)/Makefile ]; then \
		echo "==> Configuring GNU Bash $(BASH_VERSION) for $(HOST_TRIPLE)..."; \
		mkdir -p $(BASH_BUILD); \
		chmod +x $(BASH_SRC)/support/config.sub $(BASH_SRC)/support/config.guess $(BASH_SRC)/configure; \
		cd $(BASH_BUILD) && $(CROSS_ENV) $(BASH_SRC)/configure \
			--host=$(HOST_TRIPLE) \
			--build=$(BUILD_TRIPLE) \
			--prefix=/ \
			--enable-static-link \
			--without-bash-malloc \
			--disable-nls \
			--disable-job-control \
			--disable-net-redirections \
			--disable-rpath \
			--disable-readline \
			--with-installed-readline=no; \
	fi

bash-build: bash-configure
	@if [ ! -f $(BASH_BUILD)/bash ]; then \
		echo "==> Compiling GNU Bash..."; \
		$(MAKE) -C $(BASH_BUILD) || (ar rcs $(BASH_BUILD)/lib/sh/libsh.a $(BASH_BUILD)/lib/sh/*.o 2>/dev/null && $(MAKE) -C $(BASH_BUILD)); \
	fi
	@echo "==> Installing GNU Bash binary into initramfs..."; \
	mkdir -p $(INITRAMFS_DIR)/bin $(INITRAMFS_DIR)/sbin $(INITRAMFS_DIR)/etc $(INITRAMFS_DIR)/tmp $(INITRAMFS_DIR)/dev $(INITRAMFS_DIR)/proc $(INITRAMFS_DIR)/sys $(INITRAMFS_DIR)/usr/bin $(INITRAMFS_DIR)/usr/sbin $(INITRAMFS_DIR)/usr/lib $(INITRAMFS_DIR)/root $(INITRAMFS_DIR)/var/tmp $(INITRAMFS_DIR)/var/log $(INITRAMFS_DIR)/var/run; \
	echo "root:x:0:0:root:/root:/bin/bash" > $(INITRAMFS_DIR)/etc/passwd; \
	echo "root:x:0:" > $(INITRAMFS_DIR)/etc/group; \
	echo "export PATH=/bin:/sbin:/usr/bin:/usr/sbin" > $(INITRAMFS_DIR)/etc/profile; \
	rm -rf $(INITRAMFS_DIR)/etc/termcap; \
	printf "linux|linux console:\\\n\t:am:eo:mi:ms:co#80:li#24:\\\n\t:cl=\\E[H\\E[J:cm=\\E[%%i%%d;%%dH:nd=\\E[C:up=\\E[A:\\\n\t:ce=\\E[K:cd=\\E[J:so=\\E[7m:se=\\E[27m:\\\n\t:us=\\E[4m:ue=\\E[24m:md=\\E[1m:me=\\E[0m:\\\n\t:kb=^H:kd=\\E[B:kl=\\E[D:kr=\\E[C:ku=\\E[A:\n" > $(INITRAMFS_DIR)/etc/termcap; \
	rm -f $(INITRAMFS_DIR)/bin/bash $(INITRAMFS_DIR)/bin/sh $(INITRAMFS_DIR)/sbin/init $(INITRAMFS_DIR)/etc/init; \
	if [ -f $(BASH_BUILD)/bash ]; then \
		cp -f $(BASH_BUILD)/bash $(INITRAMFS_DIR)/bin/bash; \
		$(STRIP) --strip-all $(INITRAMFS_DIR)/bin/bash 2>/dev/null || true; \
	else \
		echo "==> Using Busybox for bash fallback..."; \
		ln -sf busybox $(INITRAMFS_DIR)/bin/bash; \
	fi; \
	cp -f $(INITRAMFS_DIR)/bin/bash $(INITRAMFS_DIR)/bin/sh; \
	cp -f $(INITRAMFS_DIR)/bin/bash $(INITRAMFS_DIR)/sbin/init; \
	cp -f $(INITRAMFS_DIR)/bin/bash $(INITRAMFS_DIR)/etc/init;

initramfs: busybox-build bash-build
	@echo "==> Generating /etc/termcap in initramfs..."
	@mkdir -p $(INITRAMFS_DIR)/etc
	@rm -rf $(INITRAMFS_DIR)/etc/termcap
	@printf "linux|linux console:\\\n\t:am:eo:mi:ms:co#80:li#24:\\\n\t:cl=\\E[H\\E[J:cm=\\E[%%i%%d;%%dH:nd=\\E[C:up=\\E[A:\\\n\t:ce=\\E[K:cd=\\E[J:so=\\E[7m:se=\\E[27m:\\\n\t:us=\\E[4m:ue=\\E[24m:md=\\E[1m:me=\\E[0m:\\\n\t:kb=^H:kd=\\E[B:kl=\\E[D:kr=\\E[C:ku=\\E[A:\n" > $(INITRAMFS_DIR)/etc/termcap
	@echo "==> Packing initramfs.cpio archive..."
	@cd $(INITRAMFS_DIR) && find . -print0 | cpio --null -ov --format=newc > $(INITRAMFS_CPIO)
	@echo "==> Build complete!"

bash-install: initramfs

# ------------------------------------------------------------------------------
# 10. Run in QEMU via cargo-osdk
# ------------------------------------------------------------------------------
run: bash-install
	@echo "==> Launching Petra OS in QEMU via cargo osdk..."
	cargo osdk run

# ------------------------------------------------------------------------------
# 11. Clean Artifacts
# ------------------------------------------------------------------------------
clean:
	@echo "==> Cleaning build environment..."
	rm -rf $(BUILD_DIR) $(SRC_DIR) $(SYSROOT) $(INITRAMFS_DIR)
