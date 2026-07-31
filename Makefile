# ==============================================================================
# Petra OS - Newlib Porting & Integration Makefile
# ==============================================================================

NEWLIB_VERSION ?= 4.3.0.20230120
NEWLIB_TARBALL := newlib-$(NEWLIB_VERSION).tar.gz
NEWLIB_URL     := https://sourceware.org/pub/newlib/$(NEWLIB_TARBALL)

TARGET   ?= x86_64-elf
CC       := $(shell which $(TARGET)-gcc 2>/dev/null || which x86_64-linux-gnu-gcc 2>/dev/null || echo gcc)
AS       := $(shell which $(TARGET)-as 2>/dev/null || which x86_64-linux-gnu-as 2>/dev/null || echo as)
AR       := $(shell which $(TARGET)-ar 2>/dev/null || which x86_64-linux-gnu-ar 2>/dev/null || echo ar)
RANLIB   := $(shell which $(TARGET)-ranlib 2>/dev/null || which x86_64-linux-gnu-ranlib 2>/dev/null || echo ranlib)

ROOT_DIR        := $(shell pwd)
DOWNLOAD_DIR    := $(ROOT_DIR)/downloads
SRC_DIR         := $(ROOT_DIR)/src
NEWLIB_SRC      := $(SRC_DIR)/newlib-$(NEWLIB_VERSION)
PETRA_SYS_DIR   := $(NEWLIB_SRC)/newlib/libc/sys/petra
PATCHES_DIR     := $(ROOT_DIR)/patches
BUILD_DIR       := $(ROOT_DIR)/build
NEWLIB_BUILD    := $(BUILD_DIR)/newlib
SYSROOT         := $(ROOT_DIR)/sysroot
INITRAMFS_DIR   := $(ROOT_DIR)/initramfs
INITRAMFS_CPIO  := $(BUILD_DIR)/initramfs.cpio

.PHONY: all download extract patch configure build install test-app run clean

all: build install test-app

# 1. Create directory structure & download tarball
download:
	@mkdir -p $(DOWNLOAD_DIR) $(SRC_DIR) $(PATCHES_DIR) $(BUILD_DIR) $(SYSROOT) $(INITRAMFS_DIR)
	@if [ ! -f $(DOWNLOAD_DIR)/$(NEWLIB_TARBALL) ]; then \
		echo "==> Downloading newlib $(NEWLIB_VERSION)..."; \
		curl -sSL $(NEWLIB_URL) -o $(DOWNLOAD_DIR)/$(NEWLIB_TARBALL); \
	fi

# 2. Extract source code
extract: download
	@if [ ! -d $(NEWLIB_SRC) ]; then \
		echo "==> Extracting newlib tarball..."; \
		tar -xzf $(DOWNLOAD_DIR)/$(NEWLIB_TARBALL) -C $(SRC_DIR); \
		chmod -R +x $(NEWLIB_SRC); \
	fi

# 3. Apply custom Petra OS patch & generate petra sys_dir files
patch: extract
	@echo "==> Patching config.sub and configure.host for petra OS target..."; \
	grep -q "petra\*" $(NEWLIB_SRC)/config.sub || sed -i 's/| rtems\*/| rtems* | petra*/g' $(NEWLIB_SRC)/config.sub; \
	grep -q "petra\*" $(NEWLIB_SRC)/newlib/configure.host || sed -i '/\*\-\*\-rtems\*/a \  *-*-petra*)\n\tsys_dir=petra\n\t;;' $(NEWLIB_SRC)/newlib/configure.host; \
	echo "==> Creating $(PETRA_SYS_DIR)..."; \
	mkdir -p $(PETRA_SYS_DIR); \
	printf '/*\n * Petra OS C Runtime Startup (crt0) for x86_64\n */\n.global _start\n.type _start, @function\n\n_start:\n    xorq %%rbp, %%rbp\n    movq $$__bss_start, %%rdi\n    movq $$_end, %%rcx\n    subq %%rdi, %%rcx\n    xorl %%eax, %%eax\n    cld\n    rep stosb\n    andq $$-16, %%rsp\n    xorl %%edi, %%edi\n    xorl %%esi, %%esi\n    xorl %%edx, %%edx\n    call main\n    movq %%rax, %%rdi\n    call exit\n1:  hlt\n    jmp 1b\n.size _start, . - _start\n' > $(PETRA_SYS_DIR)/crt0.S; \
	printf '#include <sys/stat.h>\n#include <sys/types.h>\n#include <errno.h>\n#include <unistd.h>\n#include <stddef.h>\n\n#undef errno\nextern int errno;\n\n#define SYS_read     0\n#define SYS_write    1\n#define SYS_open     2\n#define SYS_close    3\n#define SYS_fstat    5\n#define SYS_lseek    8\n#define SYS_brk      12\n#define SYS_ioctl    16\n#define SYS_getpid   39\n#define SYS_kill     62\n#define SYS_exit     60\n\nstatic inline long __syscall6(long n, long a1, long a2, long a3, long a4, long a5, long a6) {\n    long ret;\n    register long r10 __asm__("r10") = a4;\n    register long r8  __asm__("r8")  = a5;\n    register long r9  __asm__("r9")  = a6;\n    __asm__ volatile (\n        "syscall"\n        : "=a"(ret)\n        : "a"(n), "D"(a1), "S"(a2), "d"(a3), "r"(r10), "r"(r8), "r"(r9)\n        : "rcx", "r11", "memory"\n    );\n    return ret;\n}\n\nstatic inline long __check_sys_err(long ret) {\n    if ((unsigned long)ret >= (unsigned long)-4095) {\n        errno = -ret;\n        return -1;\n    }\n    return ret;\n}\n\nint _read(int file, char *ptr, int len) { return (int)__check_sys_err(__syscall6(SYS_read, file, (long)ptr, len, 0, 0, 0)); }\nint _write(int file, char *ptr, int len) { return (int)__check_sys_err(__syscall6(SYS_write, file, (long)ptr, len, 0, 0, 0)); }\nint _open(const char *name, int flags, int mode) { return (int)__check_sys_err(__syscall6(SYS_open, (long)name, flags, mode, 0, 0, 0)); }\nint _close(int file) { return (int)__check_sys_err(__syscall6(SYS_close, file, 0, 0, 0, 0, 0)); }\nint _lseek(int file, int ptr, int dir) { return (int)__check_sys_err(__syscall6(SYS_lseek, file, ptr, dir, 0, 0, 0)); }\nint _getpid(void) { return (int)__check_sys_err(__syscall6(SYS_getpid, 0, 0, 0, 0, 0, 0)); }\nint _kill(int pid, int sig) { return (int)__check_sys_err(__syscall6(SYS_kill, pid, sig, 0, 0, 0, 0)); }\nvoid _exit(int status) { __syscall6(SYS_exit, status, 0, 0, 0, 0, 0); while (1) {} }\nint _isatty(int file) { return (file >= 0 && file <= 2) ? 1 : 0; }\nint _fstat(int file, struct stat *st) { if (!st) { errno = EINVAL; return -1; } st->st_mode = S_IFCHR; st->st_blksize = 2048; return 0; }\nvoid *_sbrk(ptrdiff_t incr) {\n    static void *current_brk = NULL;\n    if (current_brk == NULL) {\n        long res = __syscall6(SYS_brk, 0, 0, 0, 0, 0, 0);\n        if ((unsigned long)res >= (unsigned long)-4095) { errno = ENOMEM; return (void *)-1; }\n        current_brk = (void *)res;\n    }\n    if (incr == 0) return current_brk;\n    void *new_brk = (char *)current_brk + incr;\n    long res = __syscall6(SYS_brk, (long)new_brk, 0, 0, 0, 0, 0);\n    if ((void *)res < new_brk) { errno = ENOMEM; return (void *)-1; }\n    void *old_brk = current_brk;\n    current_brk = (void *)res;\n    return old_brk;\n}\n\nint read(int file, void *ptr, size_t len) { return _read(file, (char *)ptr, (int)len); }\nint write(int file, const void *ptr, size_t len) { return _write(file, (char *)ptr, (int)len); }\nint open(const char *name, int flags, int mode) { return _open(name, flags, mode); }\nint close(int file) { return _close(file); }\noff_t lseek(int file, off_t ptr, int dir) { return _lseek(file, (int)ptr, dir); }\nint fstat(int file, struct stat *st) { return _fstat(file, st); }\nint isatty(int file) { return _isatty(file); }\nvoid *sbrk(ptrdiff_t incr) { return _sbrk(incr); }\nint getpid(void) { return _getpid(); }\nint kill(int pid, int sig) { return _kill(pid, sig); }\n' > $(PETRA_SYS_DIR)/syscalls.c;

# 4. Configure out-of-tree newlib build
configure: patch
	@mkdir -p $(NEWLIB_BUILD)
	@if [ ! -f $(NEWLIB_BUILD)/Makefile ]; then \
		echo "==> Configuring newlib for target $(TARGET)-petra using CC=$(CC)..."; \
		chmod +x $(NEWLIB_SRC)/config.sub $(NEWLIB_SRC)/configure $(NEWLIB_SRC)/newlib/configure; \
		cd $(NEWLIB_BUILD) && $(NEWLIB_SRC)/configure \
			--target=$(TARGET)-petra \
			--prefix=$(SYSROOT) \
			--disable-newlib-supplied-syscalls \
			--enable-newlib-io-long-long \
			--disable-multilib \
			--disable-dependency-tracking \
			--disable-libgloss \
			MAKEINFO=true \
			CC_FOR_TARGET="$(CC)" \
			AR_FOR_TARGET="$(AR)" \
			AS_FOR_TARGET="$(AS)" \
			RANLIB_FOR_TARGET="$(RANLIB)" \
			LD_FOR_TARGET="$(CC)"; \
	fi

# 5. Compile newlib
build: configure
	@echo "==> Compiling newlib..."
	@$(MAKE) -C $(NEWLIB_BUILD) \
		MAKEINFO=true \
		CC_FOR_TARGET="$(CC)" \
		AR_FOR_TARGET="$(AR)" \
		AS_FOR_TARGET="$(AS)" \
		RANLIB_FOR_TARGET="$(RANLIB)" \
		LD_FOR_TARGET="$(CC)"

# 6. Install headers and libraries into sysroot
install: build
	@echo "==> Installing libc to sysroot..."
	@$(MAKE) -C $(NEWLIB_BUILD) install \
		MAKEINFO=true \
		CC_FOR_TARGET="$(CC)" \
		AR_FOR_TARGET="$(AR)" \
		AS_FOR_TARGET="$(AS)" \
		RANLIB_FOR_TARGET="$(RANLIB)" \
		LD_FOR_TARGET="$(CC)"
	@if [ -d $(SYSROOT)/$(TARGET)-petra ]; then \
		cp -r $(SYSROOT)/$(TARGET)-petra/* $(SYSROOT)/; \
	fi
	@echo "==> Compiling Petra OS crt0.o and system call stubs..."
	@$(CC) -I$(SYSROOT)/include -c $(PETRA_SYS_DIR)/crt0.S -o $(SYSROOT)/lib/crt0.o
	@$(CC) -I$(SYSROOT)/include -c $(PETRA_SYS_DIR)/syscalls.c -o $(BUILD_DIR)/syscalls.o
	@$(AR) r $(SYSROOT)/lib/libc.a $(BUILD_DIR)/syscalls.o
	@$(RANLIB) $(SYSROOT)/lib/libc.a

# 7. Generate test C application and pack initramfs
test-app: install
	@echo "==> Creating test_app source..."
	@mkdir -p $(INITRAMFS_DIR) $(INITRAMFS_DIR)/sbin $(INITRAMFS_DIR)/etc $(INITRAMFS_DIR)/bin
	@printf '#include <stdio.h>\n#include <stdlib.h>\n#include <string.h>\n\nint main(int argc, char **argv) {\n    printf("[Petra OS Userland] Hello from newlib C Standard Library!\\n");\n    void *ptr = malloc(1024);\n    if (!ptr) {\n        printf("[Petra OS Userland] Error: malloc failed!\\n");\n        return 1;\n    }\n    strcpy((char *)ptr, "[Petra OS Userland] Dynamic memory allocation (sbrk/malloc) verified successfully.");\n    printf("%%s\\n", (char *)ptr);\n    free(ptr);\n    return 0;\n}\n' > $(BUILD_DIR)/test_app.c
	@echo "==> Compiling test_app with $(CC)..."
	@$(CC) -I$(SYSROOT)/include -L$(SYSROOT)/lib -nostdlib -no-pie $(SYSROOT)/lib/crt0.o $(BUILD_DIR)/test_app.c -lc -lgcc -o $(INITRAMFS_DIR)/test_app
	@cp $(INITRAMFS_DIR)/test_app $(INITRAMFS_DIR)/sbin/init
	@cp $(INITRAMFS_DIR)/test_app $(INITRAMFS_DIR)/etc/init
	@cp $(INITRAMFS_DIR)/test_app $(INITRAMFS_DIR)/bin/init
	@cp $(INITRAMFS_DIR)/test_app $(INITRAMFS_DIR)/bin/sh
	@echo "==> Packing initramfs.cpio archive..."
	@cd $(INITRAMFS_DIR) && find . -print0 | cpio --null -ov --format=newc > $(INITRAMFS_CPIO)

# 8. Run in QEMU via cargo-osdk
run: test-app
	@echo "==> Launching Petra OS in QEMU via cargo osdk..."
	cargo osdk run

# 9. Clean artifacts
clean:
	@echo "==> Cleaning build environment..."
	rm -rf $(BUILD_DIR) $(SRC_DIR) $(SYSROOT) $(INITRAMFS_DIR)
