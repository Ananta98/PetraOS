#!/usr/bin/env python3
import os
import pty
import select
import subprocess
import sys
import time

def main():
    env = os.environ.copy()
    env["PATH"] = f"{os.path.expanduser('~/.cargo/bin')}:{env.get('PATH', '')}"

    # Kill any stale QEMU instances first and wait for disk image lock release
    subprocess.run(["pkill", "-9", "-f", "qemu-system-x86_64"], check=False, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    time.sleep(1.0)
    for _ in range(10):
        res = subprocess.run(["fuser", "build/ext4.img"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        if res.returncode != 0:
            break
        subprocess.run(["pkill", "-9", "-f", "qemu-system-x86_64"], check=False, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        time.sleep(0.5)

    # Master/slave PTY to simulate real terminal interaction with QEMU
    master_fd, slave_fd = pty.openpty()

    cmd = [
        "qemu-system-x86_64",
        "-kernel", "/home/ananta/PetraOS/target/osdk/petra-osdk-bin",
        "-initrd", "/home/ananta/PetraOS/build/initramfs.cpio",
        "-append", "SHELL=/bin/sh LOGNAME=root HOME=/ USER=root PATH=/bin:/sbin init=/init -- sh -l",
        "-machine", "q35,kernel-irqchip=split",
        "-cpu", "host",
        "--no-reboot",
        "-enable-kvm",
        "-m", "8G",
        "-smp", "4",
        "-vga", "std",
        "-serial", "stdio",
        "-display", "none",
        "-device", "isa-debug-exit,iobase=0xf4,iosize=0x04",
        "-device", "ich9-ahci,id=ahci",
        "-drive", "id=ext4,file=build/ext4.img,format=raw,if=none",
        "-device", "ide-hd,drive=ext4,bus=ahci.0",
        "-drive", "id=exfat,file=build/exfat.img,format=raw,if=none",
        "-device", "ide-hd,drive=exfat,bus=ahci.1",
        "-drive", "if=pflash,format=raw,unit=0,readonly=on,file=/usr/share/OVMF/OVMF_CODE_4M.fd",
        "-drive", "if=pflash,format=raw,unit=1,file=/usr/share/OVMF/OVMF_VARS_4M.fd",
    ]

    print(f"[TEST] Starting QEMU directly...")
    proc = subprocess.Popen(
        cmd,
        stdin=slave_fd,
        stdout=slave_fd,
        stderr=slave_fd,
        close_fds=True,
        start_new_session=True,
        env=env,
        cwd="/home/ananta/PetraOS"
    )
    os.close(slave_fd)

    output = ""
    start_time = time.time()
    test_finished = False

    while time.time() - start_time < 120:
        r, _, _ = select.select([master_fd], [], [], 0.5)
        if r:
            try:
                data = os.read(master_fd, 4096).decode("utf-8", errors="replace")
                output += data
                sys.stdout.write(data)
                sys.stdout.flush()

                if "ALL USERSPACE COMMANDS VERIFIED SUCCESSFULLY" in output or "SOME USERSPACE COMMANDS FAILED" in output:
                    test_finished = True
                    time.sleep(1.0)
                    break
            except OSError:
                break

        if proc.poll() is not None:
            print(f"\n[TEST] QEMU exited with code: {proc.returncode}")
            break

    try:
        os.killpg(proc.pid, 15)
        proc.wait(timeout=2)
    except Exception:
        try:
            os.killpg(proc.pid, 9)
        except Exception:
            pass
    subprocess.run(["killall", "-9", "qemu-system-x86_64"], check=False, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    time.sleep(1.0)
    for _ in range(10):
        res = subprocess.run(["lsof", "build/ext4.img"], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        if not res.stdout:
            break
        subprocess.run(["killall", "-9", "qemu-system-x86_64"], check=False, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        time.sleep(0.5)

    print("\n[TEST] Finished QEMU run.")

if __name__ == "__main__":
    main()
