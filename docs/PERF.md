# PetraOS Graphics, Console & Memory Performance Log

All measurements are collected directly from serial console output (`PERF_*` log lines) using hardware TSC (`ostd::arch::read_tsc()`) and monotonic clock (`crate::time::get_monotonic_ns()`) in QEMU (`x86_64`, KVM enabled, `-vga std -device virtio-gpu-pci`, `1280x800 @ 32 bpp` GOP framebuffer).

## Running Performance Table

| Milestone / Change | RAM Config | Metric | Cycles | Frame Time | Effective FPS | RAM / Footprint | Notes |
| :--- | :--- | :--- | ---: | ---: | ---: | ---: | :--- |
| **Baseline (dev ELF)** | 480 MiB (`MEM=480M`) | Boot | — | — | — | 102 MiB ELF | **PANIC** in UEFI `linux-bzimage-setup`: `Error { status: NOT_FOUND }` (ELF too large for < 500 MiB RAM) |
| **Baseline (release + strip)** | 480 MiB (`498,970,624` B / 475 MiB) | `PERF_CONSOLE_INIT` | 34,994,888 | 15,848 us (15.8 ms) | 63 | `4,096,000` B shadow + `4,096,000` B MMIO | `3.0` MiB stripped ELF; `fb.read_bytes` (4 MiB MMIO read) + `fb.clear()` 4 MiB heap `Vec` |
| **Baseline (release + strip)** | 480 MiB | `PERF_CONSOLE_BENCH[30lines+clear]` | 54,270,050 | 24,577 us (24.6 ms) | 40 | `4,096,000` B | Per-pixel `Mutex<ShadowFb>` lock + full-row flush |
| **Baseline (release + strip)** | 480 MiB | `PERF_CONSOLE_CHAR` | 104,180 | 47 us | 21,276 | — | Single-char update with full-row dirty flush |
| **Opt 1: Lock-free `ShadowFb` + dirty X-span + no MMIO read + `clear()` 4 KiB chunk** | 480 MiB (`498,970,624` B / 475 MiB) | `PERF_CONSOLE_BENCH[30lines+clear]` | 18,322,058 | 8,315 us (8.3 ms) | 120 | `4,096,000` B (0 B peak clear heap) | **3.0x faster** (120x faster than 1s target); tear-free dirty X-span flush |
| **Opt 1** | 480 MiB | `PERF_CONSOLE_CHAR` | 91,230 | 41 us | 24,390 | — | Incremental X-span flush only |
| **Opt 2: Zero-copy `GemObject` (`USegment` + `DmaStream`) + Batched `VirtioGpu` (`submit_batch_wait`) + `smoltcp` right-sizing** | 384 MiB (`398,307,328` B / 379 MiB) | `PERF_DRM_GEM` | 784 (`flush_pages`) | 0 us | — | `4,096,000` B (1x, **50% RAM saved**) | Eliminated duplicate `BTreeMap<UFrame>` and CPU `flush_pages` memcpy |
| **Opt 2** | 384 MiB | `PERF_VIRTIO_GPU_DMA_FULL[1280x800]` | 1,199,280 | 543 us (0.54 ms) | 1,841 | — | `TRANSFER_TO_HOST_2D` + `RESOURCE_FLUSH` in 1 VirtIO doorbell kick |
| **Opt 2** | 384 MiB | `PERF_VIRTIO_GPU_DMA_RECT[64x64]` | 129,632 | 58 us | 17,241 | — | Incremental 64x64 damage rect over batched VirtIO DMA |
| **Opt 2** | 384 MiB (`398,307,328` B / 379 MiB) | `PERF_CONSOLE_BENCH[30lines+clear]` | 56,004,562 | 25,387 us (25.4 ms) | 39 | `4,096,000` B | Boots and runs all subsystems under 384 MiB RAM (< 500 MiB target) |

## Summary of Optimizations

1. **Console Rendering (< 1s target, No full-screen redraws, Tear-free)**:
   - `petra/src/drivers/char/tty/vt/terminal.rs`: Replaced per-pixel `Mutex<ShadowFb>` locking with lock-free `AtomicU32` pixel storage in internal RAM, added per-row `(dirty_min_x, dirty_max_x)` X-span tracking so only changed horizontal spans are copied to VRAM, eliminated the 4–8 MiB `fb.read_bytes()` MMIO read on init, and increased `os-terminal` color cache size to 512.
   - `petra/src/drivers/drm/framebuffer/framebuffer.rs`: Replaced `FrameBuffer::clear()`'s full-screen `vec![0u8; size]` heap allocation with a 4 KiB stack zero buffer.

2. **DMA Copies & Zero-Copy GEM (`GemObject` + `VirtioGpu`)**:
   - `petra/src/drivers/drm/gem/object.rs`: Replaced `DmaCoherent` + duplicate `BTreeMap<usize, UFrame>` with a single `USegment` mapped via `DmaStream::map(..., true)`. User-space `mmap(2)` and GPU DMA share the exact same physical frames (`flush_pages` is `DmaStream::sync_to_device` with zero CPU memcpy, saving 50% of GEM RAM).
   - `petra/src/drivers/bus/virtio/queue.rs` & `petra/src/drivers/drm/virtio/device.rs`: Added `VirtQueue::submit_batch_wait` and `CommandStaging` dual slots so `VirtioGpu::transfer_and_flush` submits `TRANSFER_TO_HOST_2D` + `RESOURCE_FLUSH` in a single VirtIO doorbell kick.

3. **Fast Internal RAM Primitives & Low-RAM (< 500 MiB) Compatibility**:
   - `petra/src/drivers/drm/intel/display.rs`: Replaced per-damage heap `vec![0u8; copy_row_bytes]` in `sync_damage_to_scanout` with an 8 KiB stack buffer fast path.
   - `petra/src/net/interface.rs`: Right-sized `smoltcp` per-socket buffers (TCP RX 1 MiB -> 128 KiB, TX 256 KiB -> 64 KiB; UDP 64 KiB -> 16 KiB), saving > 1 MiB per TCP socket.
   - Verified full boot + console + VirtIO-GPU DMA at `MEM=384M` (`379 MiB` usable physical RAM) using `--release` (`3.0–4.0 MiB` kernel ELF vs `102 MiB` unstripped dev ELF).
