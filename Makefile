.PHONY: all run build check clean

all: build

build:
	export PATH="$$HOME/.cargo/bin:$$PATH" && cargo osdk build

check:
	export PATH="$$HOME/.cargo/bin:$$PATH" && cargo osdk check

run:
	bash scripts/run-qemu.sh -n --mem 4G -- -vga std -monitor unix:/tmp/petra_qemu_monitor.sock,server,nowait $(QEMUFLAGS)
