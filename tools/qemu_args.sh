#!/bin/bash

# SPDX-License-Identifier: GPL-2.0
# PetraOS QEMU argument generator for OSDK.
# Usage: `qemu_args.sh [scheme]`
#   - scheme: "normal" (default), "test", "headless", "direct"

SCHEME=${1:-"normal"}

# Configurable options via environment variables
ARCH=${ARCH:-"x86_64"}
SMP=${SMP:-4}
MEM=${MEM:-"8G"}
CPU=${CPU:-"host"}
ENABLE_KVM=${ENABLE_KVM:-"on"}
DISPLAY=${DISPLAY_MODE:-"std"}       # "std", "none", "vnc", or custom
SERIAL=${SERIAL:-"stdio"}           # "stdio", "file", "none"
LOG_FILE=${LOG_FILE:-"qemu-serial.log"}
MONITOR_SOCK=${MONITOR_SOCK:-"/tmp/petra_qemu_monitor.sock"}
OVMF=${OVMF:-"on"}
OVMF_CODE=${OVMF_CODE:-"/usr/share/OVMF/OVMF_CODE_4M.fd"}
OVMF_VARS=${OVMF_VARS:-"/usr/share/OVMF/OVMF_VARS_4M.fd"}
AHCI_DRIVES=${AHCI_DRIVES:-"on"}
EXT4_IMG=${EXT4_IMG:-"build/ext4.img"}
EXFAT_IMG=${EXFAT_IMG:-"build/exfat.img"}

QEMU_ARGS=""

# Machine & Accel
MACHINE_ARGS="-machine q35,kernel-irqchip=split"
if [ "$ENABLE_KVM" = "on" ] && [ -e /dev/kvm ]; then
    ACCEL_ARGS="-enable-kvm -cpu ${CPU}"
else
    ACCEL_ARGS="-cpu qemu64"
fi

QEMU_ARGS="$QEMU_ARGS $MACHINE_ARGS $ACCEL_ARGS --no-reboot -m ${MEM} -smp ${SMP}"

# Display & Graphic
case "$SCHEME" in
    "test"|"headless")
        QEMU_ARGS="$QEMU_ARGS -nographic -display none"
        ;;
    *)
        if [ "$DISPLAY" = "none" ]; then
            QEMU_ARGS="$QEMU_ARGS -nographic -display none"
        elif [ "$DISPLAY" = "std" ]; then
            QEMU_ARGS="$QEMU_ARGS -vga std"
        else
            QEMU_ARGS="$QEMU_ARGS -display $DISPLAY"
        fi
        ;;
esac

# Serial & Logging
if [ "$SCHEME" = "test" ]; then
    QEMU_ARGS="$QEMU_ARGS -serial file:${LOG_FILE}"
else
    if [ "$SERIAL" = "stdio" ]; then
        QEMU_ARGS="$QEMU_ARGS -serial stdio"
    elif [ "$SERIAL" = "file" ]; then
        QEMU_ARGS="$QEMU_ARGS -serial file:${LOG_FILE}"
    fi
fi

# QEMU Monitor
if [ -n "$MONITOR_SOCK" ] && [ "$SCHEME" != "test" ]; then
    QEMU_ARGS="$QEMU_ARGS -chardev socket,id=mon0,path=${MONITOR_SOCK},server=on,wait=off -mon chardev=mon0,mode=readline"
fi

# Debug exit device for test runners
QEMU_ARGS="$QEMU_ARGS -device isa-debug-exit,iobase=0xf4,iosize=0x04"

# Storage / Drives (AHCI SATA Controller with ext4 and exfat disk images)
if [ "$AHCI_DRIVES" = "on" ]; then
    DRIVE_ARGS="-device ich9-ahci,id=ahci"
    if [ -f "$EXT4_IMG" ]; then
        DRIVE_ARGS="$DRIVE_ARGS -drive id=ext4,file=${EXT4_IMG},format=raw,if=none -device ide-hd,drive=ext4,bus=ahci.0"
    fi
    if [ -f "$EXFAT_IMG" ]; then
        DRIVE_ARGS="$DRIVE_ARGS -drive id=exfat,file=${EXFAT_IMG},format=raw,if=none -device ide-hd,drive=exfat,bus=ahci.1"
    fi
    QEMU_ARGS="$QEMU_ARGS $DRIVE_ARGS"
fi

# UEFI / OVMF Firmware
if [ "$OVMF" = "on" ]; then
    if [ -f "$OVMF_CODE" ] && [ -f "$OVMF_VARS" ]; then
        QEMU_ARGS="$QEMU_ARGS -drive if=pflash,format=raw,unit=0,readonly=on,file=${OVMF_CODE} -drive if=pflash,format=raw,unit=1,file=${OVMF_VARS}"
    fi
fi

echo "$QEMU_ARGS"
