CC ?= x86_64-linux-gnu-gcc
SYSROOT ?= $(abspath ../build-xbstrap/system-root)
CFLAGS ?= --sysroot=$(SYSROOT) -isystem $(SYSROOT)/usr/include -B$(SYSROOT)/usr/lib -O2 -Wall -Wextra -static
LDFLAGS ?= --sysroot=$(SYSROOT) -B$(SYSROOT)/usr/lib -L$(SYSROOT)/usr/lib -static

DESTDIR ?= $(abspath ../initramfs_root)

SRCS = $(wildcard */main.c)
PROGS = $(patsubst %/main.c,bin/%,$(SRCS))

.PHONY: all clean install

all: $(PROGS)

bin/%: %/main.c
	@mkdir -p bin
	$(CC) $(CFLAGS) $< -o $@ $(LDFLAGS)

install: all
	@mkdir -p $(DESTDIR)/bin $(DESTDIR)/sbin
	@if [ -f bin/init ]; then \
		cp -vf bin/init $(DESTDIR)/sbin/init; \
	fi
	@for prog in $(filter-out bin/init,$(PROGS)); do \
		cp -vf $$prog $(DESTDIR)/bin/; \
	done

clean:
	rm -rf bin
