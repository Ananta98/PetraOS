# PetraOS Hypervisor, Rocky Linux (Ginux Patterns), and Safe Driver Research

This document captures verified technical research on three areas for PetraOS:

1. **Hypervisor capabilities in OSTD `0.19.0`** (Intel VT-x / VMX + EPT) to run unmodified Linux kernels, Linux applications, and Linux driver subsystems alongside PetraOS's native Void Linux (`xbps`) + Space Desktop environment.
2. **Lessons from `gmkurtzer/ginux`** ([`scripts/` at `8c7e6ba`](https://github.com/gmkurtzer/ginux/tree/8c7e6ba5a5e0922be35ad2757b4c7fa743d7a6cc/scripts)) for provisioning, verifying, and booting **Rocky Linux 10** guest images without host root privileges or a Docker daemon.
3. **Asterinas safe device model & component initialization architecture** ([`aster-device` at `e36b5b3`](https://github.com/asterinas/asterinas/tree/e36b5b3b759e616febf2c8f0e88d8cb2c103f266/kernel/core/comps/device/src) and `kernel/libs/comp-sys`), eliminating the unsafe `kpi::shim` bridge and `.init_array` raw function pointer casts from PetraOS.

---

## 1. Hypervisor Technology in OSTD `0.19.0`

PetraOS already builds against `ostd = "0.19.0"` (see [`petra/Cargo.toml`](../petra/Cargo.toml)). On `x86_64`, OSTD `0.19.0` ships a complete, safe-Rust hardware virtualization layer modeled on Asterinas [RFC-0003: Hypervisor Support](https://github.com/asterinas/asterinas/blob/e36b5b3b759e616febf2c8f0e88d8cb2c103f266/book/src/rfcs/0003-hypervisor-support.md) and demonstrated in [Writing a Hypervisor in About 100 Lines of Safe Rust](https://github.com/asterinas/asterinas/blob/e36b5b3b759e616febf2c8f0e88d8cb2c103f266/book/src/ostd/a-100-line-hypervisor.md).

### 1.1 Safe Public Abstractions Available in `ostd 0.19.0`

All privileged VMX instructions (`VMXON`, `VMXOFF`, `VMLAUNCH`, `VMRESUME`, `VMPTRLD`, `VMCLEAR`, `VMREAD`, `VMWRITE`, `INVEPT`), host/guest register save/restore, `IA32_FEATURE_CONTROL` configuration, and 4-level EPT construction are encapsulated inside OSTD. The kernel (`petra`) stays `#![deny(unsafe_code)]` / `#![forbid(unsafe_code)]`:

| Abstraction | Module | Responsibility |
| :--- | :--- | :--- |
| `GuestPhysMemSpace` | `ostd::vm::gpm_space` | Owns a 4-level 48-bit Intel EPT (`EptPtConfig`), mapping `Gpaddr` ranges (4-KiB granularity) to OSTD-tracked `UFrame`s via `cursor()` / `cursor_mut()`. Handles cross-CPU `INVEPT` invalidation automatically. |
| `GuestContext` | `ostd::arch::vm::context` | Owns a VMCS + `VcpuArchState` (`VcpuRegs`, `VcpuSregs`, `VcpuMsrs`, `pending_interrupt`, `timer_deadline`). Provides `VcpuSregs::new_real_mode()` for 16-bit boot and full 64-bit `IA32E_MODE_GUEST` control/segment/MSR fields (`cr0`, `cr2`, `cr3`, `cr4`, `efer`, `apic_base`, `star`, `lstar`, `syscall_mask`, `kernel_gs_base`, `pat`, `tsc_aux`, `sysenter_*`). |
| `GuestMode` | `ostd::vm::guest_mode` | Acquires a ref-counted `VmxGuard` (enabling VMX across CPUs) and runs `execute(&mut GuestContext, &GuestPhysMemSpace, &impl GuestModeHooks)` until a `GuestReturnReason::VmExit(GuestExitInfo)` or `GuestReturnReason::KernelEvent`. |
| `GuestModeHooks` | `ostd::vm::guest_mode` | Pre-entry and post-exit callbacks (`pre_guest_run`, `post_guest_run`, `has_kernel_event`) with IRQ-disabled access to `VcpuArchState` for injecting virtual interrupts (`GuestInterrupt { vector: 32..=255 }`, using `INTERRUPT_WINDOW_EXITING` if `RFLAGS.IF == 0`) and setting VMX preemption timer deadlines (`GuestTimerInstant`). |
| `GuestExitInfo` & `VmxExitReason` | `ostd::arch::vm::exit` | Exposes `exit_reason`, `instruction_len`, `exit_qualification`, `guest_phys_addr` (for EPT violations/misconfigs), `interruption_info`, and `idt_vectoring_info`. |

In addition, OSTD's QEMU runner configuration in [`scripts/qemu_args.sh`](../scripts/qemu_args.sh) already launches PetraOS with `-enable-kvm -cpu host` when `/dev/kvm` is available, which supports nested VMX testing whenever the host Linux kernel has `kvm_intel.nested=1` (or on bare-metal Intel hardware).

### 1.2 Verified Limitations in `ostd 0.19.0`

Inspecting the `ostd-0.19.0` source tree reveals three boundaries that directly affect PetraOS design decisions:

1. **Intel VT-x (VMX) only**: `ostd::vm` is currently gated by `#[cfg(target_arch = "x86_64")]` and backed exclusively by `ostd::arch::x86::vm::vmx` and `ept`. AMD-V (SVM / Nested Paging) is not yet implemented in `ostd 0.19.0` (though the `GuestMode` / `GuestPhysMemSpace` traits were designed in RFC-0003 to support SVM, ARM stage-2, and RISC-V G-stage later).
2. **Untyped RAM (`UFrame`) only in `GuestPhysMemSpace`**: `CursorMut::map` accepts `UFrame` + `PageProperty::new_guest(...)`. `ostd 0.19.0` does **not** yet have a public API to map `ostd::io::IoMem` (physical device MMIO BARs) directly into `GuestPhysMemSpace` for zero-trap device passthrough.
3. **No public per-VM IOMMU / DMA remapping API yet**: While `ostd::arch::x86::iommu` (`dma_remapping`, `interrupt_remapping`) exists internally in OSTD for host driver isolation, it is `pub(crate)` and not yet wired to `GuestPhysMemSpace` or VFIO-style device assignment.

---

## 2. Feasibility Analysis: Keeping Void/XBPS + Space Desktop Alive While Adding Linux Apps & Drivers via Virtualization

PetraOS already runs a Void Linux (`glibc`, `xbps`) userland and the native Space Desktop GUI (`userspace/space-*`) natively on top of PetraOS syscalls and DRM/KMS. Adding virtualization should **complement** Void/XBPS and Space Desktop, not replace them.

### 2.1 Running Linux Applications (Rocky Linux / Arbitrary Distros) — **Feasible Today with Zero OSTD Changes**

Using `ostd::vm` directly, PetraOS can implement a KVM-style kernel hypervisor subsystem (`petra/src/virt/`) plus a userspace or kernel-hosted VMM (similar to `crosvm` / `cloud-hypervisor` / Asterinas KVM):

- **How it works**:
  - A guest Rocky Linux kernel (`vmlinuz` / `bzImage`) boots inside a `GuestPhysMemSpace` + `GuestContext` per vCPU.
  - Because `UNRESTRICTED_GUEST` and `ENABLE_EPT` are configured by OSTD (`ostd/src/arch/x86/vm/vmcs.rs`), the guest can boot unmodified 64-bit Linux kernels (via direct Linux `bzImage` boot protocol in 64-bit long mode or starting in 16-bit real mode).
  - Virtual hardware is emulated cleanly in safe Rust by handling `IO_INSTRUCTION`, `EPT_VIOLATION` (MMIO), `CPUID`, `MSR_READ`/`MSR_WRITE`, and `HLT` VM exits:
    - In-kernel fast paths: virtual Local APIC (xAPIC/x2APIC), IOAPIC, PIT/HPET/KVM-clock, serial 16550 (`0x3f8`).
    - Paravirtual VirtIO devices (virtio-mmio or virtio-pci): `virtio-blk` (Rocky rootfs disk), `virtio-net` (bridged/NAT to PetraOS `smoltcp`), `virtio-console` (headless/PTY integration), `virtio-vsock` (host-guest RPC), and `virtio-fs` (sharing `/home` and `/tmp` with Void/Space Desktop).
- **Space Desktop + XBPS coexistence**:
  - Native PetraOS apps continue to install via `xbps-install` and run natively at bare-metal syscall speed.
  - Rocky Linux CLI tools, RPM/DNF developer toolchains, systemd services, and Linux GUI apps run in an on-demand Rocky microVM and integrate into Space Desktop:
    - Terminal integration via `virtio-console` / `virtio-vsock` (`petra-vm enter rocky`).
    - Graphical apps via a Wayland/X11 proxy over `virtio-vsock` (Crostini `sommelier` / WSLg model) rendering windows directly inside `space-compositor`, plus `.desktop` launchers in `space-launcher`.

### 2.2 Running Linux Drivers via Virtualization — **Two Distinct Tiers**

| Linux Driver Category | Today (`ostd 0.19.0`) | Required Support | Notes |
| :--- | :--- | :--- | :--- |
| **Virtual / software kernel drivers & filesystems** (`xfs`, `btrfs`, `zfs`, `nfs`, `overlayfs`, `wireguard`, `nftables`, `iptables`, `fuse`, `loop`, `dm-crypt`, USB-over-IP `usbip`) | **Supported today** | `petra/src/virt/` + `virtio-blk` / `virtio-net` / `virtio-fs` / `virtio-vsock` | The Rocky Linux guest runs its real upstream Linux kernel modules and exposes storage, network tunnels, or block/filesystem services back to PetraOS over `virtio-vsock` / `virtio-fs` / `NBD`. |
| **Physical PCI hardware drivers** (Wi-Fi cards, discrete GPUs, vendor NVMe/NIC/USB controllers in a Linux driver VM) | **Blocked on OSTD additions** | Upstream OSTD extensions: (1) `IoMem` MMIO mapping in `GuestPhysMemSpace`, (2) VT-d IOMMU domain attachment per VM, (3) physical IRQ / MSI-X forwarding to `GuestInterruptPort` | Without IOMMU DMA isolation and `IoMem` EPT mapping, giving a guest access to physical PCI DMA is unsafe and bypasses OSTD's memory-safety guarantees. |

**Recommendation**: Build the Rocky Linux microVM first for **Linux application compatibility, systemd workloads, and software/filesystem/network Linux kernel subsystems** (Tier 1), keeping Void Linux `xbps` + Space Desktop as the native OS. Treat PCI passthrough driver VMs (Tier 2) as a later phase after extending OSTD with IOMMU + `IoMem` EPT support.

---

## 3. What PetraOS Can Learn from `gmkurtzer/ginux`

### 3.1 Important Architectural Clarification

At commit [`8c7e6ba`](https://github.com/gmkurtzer/ginux/tree/8c7e6ba5a5e0922be35ad2757b4c7fa743d7a6cc), Ginux is **no longer** a standalone bare-metal kernel (its earlier standalone UEFI Rust kernel research was frozen on the `bootable-kernel` branch / `bootable-kernel-final` tag). As documented in Ginux's `README.md`, `hdae/ARCHITECTURE.md`, and `docs/how-ginux-differs.md`:

- Current Ginux runs as a **Linux userspace supervisor** on top of a host Rocky Linux 10 kernel.
- It uses **seccomp-bpf + `ptrace` (`PTRACE_SYSEMU` / syscall-entry-exit stops)** to intercept managed x86-64 ELF syscalls and emulate, mediate, or deny them per a TOML `[contract]` — architecturally similar to gVisor, **not** a hardware hypervisor and **not** capable of loading Linux kernel `.ko` drivers.
- Because PetraOS *is* an independent OSTD-based OS kernel (`#![deny(unsafe_code)]`) with hardware VT-x (`ostd::vm`) available, PetraOS **should not** copy Ginux's `ptrace`/seccomp Linux-on-Linux backend. Instead, PetraOS should adopt Ginux's **Rocky Linux image pipeline, cloud-init/QEMU automation, minimal systemd tailoring, and declarative instance/VM configuration model**.

### 3.2 Concrete Patterns to Reuse from `ginux/scripts` and `ginux/config`

1. **Rootless, Digest-Pinned Rocky Linux 10 OCI Rootfs Pipeline**
   - Reference scripts: `scripts/oci-common.sh`, `scripts/fetch-rocky-oci.sh`, `scripts/verify-rocky-oci.sh`, `scripts/prepare-rocky-oci-rootfs.sh`, and manifests `config/rocky-instance-oci.env` / `config/rocky-systemd-oci.env`.
   - **How Ginux does it cleanly (zero Docker daemon, zero root)**:
     - Authenticates anonymously to `auth.docker.io` and downloads the exact OCI index, manifest, config JSON, and `.tar.gz` layers for `rockylinux/rockylinux:10-ubi-init` (systemd-ready) or `10.2` using only `curl`, `jq`, and `sha256sum`.
     - Verifies **both** compressed blob digests (`sha256:...`) and uncompressed OCI `diff_ids` after `gzip -dc`.
     - Rejects path-traversal entries (`..`) during tar inspection, applies OCI whiteout files (`.wh.*`, `.wh..wh..opq`), and merges layers into a deterministic rootfs tarball.
   - **How PetraOS can apply this**: Add a `scripts/rocky/` pipeline (alongside `scripts/build-userland/build-userland-amd64.sh` which builds PetraOS's native Void/XBPS `ext4.img`) to fetch Rocky Linux `10-ubi-init` rootlessly and pack a dedicated `build/rocky-rootfs.ext4` guest disk image using `mke2fs -d` (no root required).

2. **Checksum-Verified Rocky 10 Cloud Image + Kernel Extraction + Cloud-Init Seed**
   - Reference scripts: `scripts/fetch-rocky-host.sh`, `scripts/prepare-rocky-host.sh`, `scripts/start-rocky-host.sh`, `scripts/extract-rocky-instance.sh`, `config/rocky-host-image.env`, `config/rocky-host-image.CHECKSUM`.
   - **What to adopt**:
     - Download `Rocky-10-GenericCloud-Base-10.2-*.x86_64.qcow2` into `build/cache/rocky/` using atomic `.download` -> `sha256sum` verification -> `mv` (preserving cached images on failed refreshes).
     - Generate a NoCloud `cidata` seed ISO (`meta-data` + `user-data` with SSH key / console autologin) and back test runs with copy-on-write QCOW2 overlays (`qemu-img create -f qcow2 -F qcow2 -b "$base" "$overlay"`).
     - Notably, `Rocky-10-GenericCloud-Base` also contains the upstream Rocky Linux `vmlinuz` kernel + modules, which can either boot directly under PetraOS's hypervisor or be inspected inside QEMU TCG/KVM test harnesses. (Note: `prepare-rocky-host.sh` used macOS `hdiutil makehybrid`; on PetraOS's Linux build host, use `xorriso -as mkisofs` / `genisoimage` / `cloud-localds`.)

3. **Minimal `systemd` Tuning for Fast MicroVM Boot (`config/examples/rocky10-systemd.toml`)**
   - Ginux's `[hooks].pre_boot` section documents the exact tweaks required to make Rocky Linux 10 `ubi-init` boot cleanly to a login prompt inside a minimal virtual environment:
     - Mask/gate services that hang or fail without full host hardware/D-Bus/udev (`dbus-broker.service`, `dbus.service`, `dbus.socket`, `modprobe@.service`, `systemd-userdbd.service`, `systemd-update-utmp.service`, `systemd-initctl.*`) using `ConditionPathExists=` drop-ins or unit removals.
     - Work around `systemd-journald` sandbox failures (`IPAddressDeny=` drop-in when host/guest lacks full cgroup/BPF filtering).
     - Wire `console-getty.service` under `getty.target.wants` and set deterministic `/etc/pam.d/login` + `/etc/localtime` so serial/virtio-console logins work reliably.

4. **Declarative TOML VM Configuration & Contract Philosophy**
   - Reference: `config/examples/rocky10-systemd.toml` (`[instance]`, `[resources]` `vcpus` / `memory_mib`, `[storage]`, `[devices]`, `[network]`, `[hooks]`, `[failure]`).
   - PetraOS can adopt a clean versioned TOML descriptor (e.g., `/etc/petra/vms/rocky10.toml`) for configuring each Linux microVM's vCPU count, RAM limit, block overlays, shared `virtio-fs` host directories (such as `/home` or `/tmp`), and network bridge policy.

---

## 4. Safe Asterinas Device Model & Component Initialization

To bring PetraOS's kernel infrastructure up to Asterinas's safety standards:

1. **Remove `petra/src/kpi/` and `unsafe` C-ABI shims**:
   - The only parts of `kpi/` used by PetraOS were the safe soft-timer wheel (`KTimer` -> `petra/src/time/timer.rs`), completion primitive (`KCompletion` -> `petra/src/sync/completion.rs`), and kernel workqueue (`schedule_work*` -> `petra/src/proc/workqueue.rs`).
   - All `unsafe` code in PetraOS lived in `kpi/shim/` (`linux_register_chrdev`, `linux_request_irq`, `copy_from_user`/`copy_to_user`, raw C pointer callbacks). Removing `kpi/` eliminates `kpi/shim/` completely.
2. **Port `aster-device` (`kernel/core/comps/device/src`) into `petra/src/drivers/core/`**:
   - Replaces the simplified prototype in `petra/src/drivers/core/` with Asterinas's full safe-Rust device model (`AnyDevice`, `BareDevice`, `Bus`, `BusHandle`, `BusDevice`, `Driver`, `DriverHandle`, `Class`, `ClassHandle`, `ClassDevice`, `ClassObserver`, `Attr<D>`, `DeviceType<D>`, `DevNum`/`DeviceId`, `KernelHooks`/`DevNodeRequest`, transactional `add_device`/`remove_device` with automatic rollback and parent-child reference checks, and glue directories / sysfs symlinks).
3. **Eliminate `.init_array` `unsafe` in `petra/src/modules/`**:
   - Following Asterinas's `component` crate (`kernel/libs/comp-sys/component/src/lib.rs`), replace the custom `#[unsafe(link_section = ".init_array")] static __INIT_CALL` macro in `petra/src/modules/mod.rs` with `inventory::collect!(ModuleDescriptor)` and `inventory::submit!`.
   - `inventory::submit!` registers static `ModuleDescriptor` records at link time without any `unsafe` in `petra`, without heap allocation or mutex locking during `.init_array`, and enables deterministic sorting by `(InitcallLevel, name)` inside `modules::init_calls()`.

---

## 5. Phased Implementation Roadmap for Virtualization on PetraOS

1. **Phase 0 — OSTD VMX Capability Probe & Smoke Test**:
   - Add a `#[cfg(ktest)]` test in a new `petra/src/virt/` module verifying `GuestMode::new()` + `GuestPhysMemSpace::new()` + `GuestContext::new(...)` cleanly detect VMX support (or skip gracefully with `Error::NotEnoughResources | Error::AccessDenied` under TCG without VT-x) and execute a minimal guest payload like Asterinas's 100-line hypervisor.
2. **Phase 1 — Kernel `/dev/kvm` / Safe Hypervisor Subsystem (`petra/src/virt/`)**:
   - Implement `Vm`, `Vcpu`, guest physical memory slot management over `GuestPhysMemSpace`, minimal x86 interrupt controller (in-kernel LAPIC/IOAPIC/PIT/serial) implementing `GuestModeHooks`, and either a `/dev/kvm` `CharacterDevice` + `ioctl` ABI (compatible with `cloud-hypervisor` / `qemu` / custom VMM) or an integrated `petra-vmm` syscall/device interface.
3. **Phase 2 — Rocky Linux 10 Guest Pipeline (Ginux-Inspired)**:
   - Add `scripts/rocky/` scripts modeled on `ginux/scripts/{oci-common.sh,fetch-rocky-oci.sh,verify-rocky-oci.sh,prepare-rocky-oci-rootfs.sh}` to pull `rockylinux/rockylinux:10-ubi-init` rootlessly, apply minimal systemd unit drop-ins from `rocky10-systemd.toml`, build `build/rocky-rootfs.ext4`, and pair it with a standard Linux `bzImage` kernel.
   - Add virtio paravirtual device backends (`virtio-blk`, `virtio-console`, `virtio-net`, `virtio-vsock`, `virtio-fs`).
4. **Phase 3 — Space Desktop & Void/XBPS Integration**:
   - Expose a `petra-vm` CLI (packaged or installed in the Void rootfs) to start/stop/attach the Rocky Linux VM (`virtio-console`), forward Wayland/GUI windows over `virtio-vsock` into `space-compositor`, and share user directories via `virtio-fs`.
5. **Phase 4 — Linux Driver Domains (Upstream OSTD Extensions)**:
   - Extend `ostd` (upstream PR or workspace fork) to support:
     1. Safe `IoMem` MMIO region mapping in `GuestPhysMemSpace`.
     2. Public VT-d IOMMU domain creation and attachment to a `GuestPhysMemSpace` + MSI/MSI-X IRQ forwarding into `GuestInterruptPort`.
   - Use this to run unmodified Linux PCI drivers inside isolated driver microVMs when native PetraOS drivers do not yet exist for specific hardware.
