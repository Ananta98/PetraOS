/// VirtIO GPU Driver (`virtio_gpu`)
///
/// Implements a VirtIO 2D display device driver for PetraOS over PCI.
/// Provides display info querying, 2D resource management, scanout setup,
/// and integration with PetraOS `GPU_MANAGER` and `/dev/fbN` framebuffer nodes.
///
/// Includes a simulated fallback mode when physical VirtIO GPU hardware is absent.
pub mod device;
pub mod regs;
pub mod virtqueue;

use self::device::{VirtioGpuDevice, VirtioGpuDeviceInner};
use self::regs::*;
use self::virtqueue::Virtqueue;

use crate::drivers::char::virtio_console::VirtioBar;
use crate::drivers::gpu::GPU_MANAGER;
use crate::drivers::gpu::framebuffer::{Framebuffer, PixelFormat, VideoMode};
use crate::drivers::pci::{self, PciBar};

use alloc::format;
use alloc::sync::Arc;
use ostd::io::IoMem;
use ostd::mm::dma::DmaCoherent;
use ostd::sync::SpinLock;

/// Helper to reset and configure a physical VirtIO GPU device over PCI.
fn init_physical_device(
    pci_dev: &pci::PciDevice,
    device_index: usize,
) -> Result<Arc<VirtioGpuDevice>, ostd::Error> {
    pci_dev.enable_bus_mastering();

    let bar = match pci_dev.bars[0] {
        PciBar::IoSpace { port, .. } => {
            pci_dev.enable_io_space();
            VirtioBar::Io {
                port_base: port as u16,
            }
        }
        PciBar::MemoryMapped {
            base_addr, size, ..
        } if base_addr != 0 => {
            pci_dev.enable_memory_space();
            let mem_size = if size > 0 { size as usize } else { 0x1000 };
            let mem = IoMem::acquire(base_addr as usize..base_addr as usize + mem_size)?;
            VirtioBar::Mmio { mem }
        }
        _ => return Err(ostd::Error::InvalidArgs),
    };

    // 1. Reset device and negotiate status
    bar.write_u8(LEGACY_STATUS as u16, STATUS_RESET);
    bar.write_u8(LEGACY_STATUS as u16, STATUS_ACKNOWLEDGE);
    bar.write_u8(LEGACY_STATUS as u16, STATUS_ACKNOWLEDGE | STATUS_DRIVER);

    // Feature negotiation (accept default feature set)
    let _host_features = bar.read_u32(LEGACY_DEVICE_FEATURES as u16);
    bar.write_u32(LEGACY_DRIVER_FEATURES as u16, 0);

    // 2. Configure Queue 0 (control queue: controlq)
    bar.write_u16(LEGACY_QUEUE_SELECT as u16, 0);
    let qsize = bar.read_u16(LEGACY_QUEUE_SIZE as u16);
    let control_vq = Virtqueue::new(qsize)?;
    bar.write_u32(LEGACY_QUEUE_PFN as u16, control_vq.pfn());

    // 3. Allocate DMA memory buffers
    let cmd_dma = DmaCoherent::alloc(2, true)?;
    // Allocate 1024 * 768 * 4 bytes (~3MB, 768 pages) for framebuffer DMA
    let fb_dma = DmaCoherent::alloc(768, true)?;

    // 4. Mark driver OK
    bar.write_u8(
        LEGACY_STATUS as u16,
        STATUS_ACKNOWLEDGE | STATUS_DRIVER | STATUS_DRIVER_OK,
    );

    let default_mode = VideoMode {
        width: 1024,
        height: 768,
        pitch: 1024 * 4,
        bpp: 32,
        format: PixelFormat::Rgba8888,
    };
    let fb = Arc::new(Framebuffer::new(default_mode));

    let name = format!("virtio-gpu{}", device_index);

    let dev = Arc::new(VirtioGpuDevice {
        name,
        inner: SpinLock::new(VirtioGpuDeviceInner::Physical {
            bar,
            control_vq,
            cmd_dma,
            fb_dma,
            fb,
            mode: default_mode,
            resource_id: 1,
        }),
    });

    Ok(dev)
}

/// VirtIO GPU Driver Registration.
pub struct VirtioGpuDriver;

impl crate::device::Driver for VirtioGpuDriver {
    fn name(&self) -> &str {
        "virtio-gpu"
    }

    fn bus_name(&self) -> &str {
        "pci"
    }

    fn description(&self) -> &str {
        "VirtIO GPU 2D Display Device Driver"
    }

    fn probe(&self) -> Result<(), ostd::Error> {
        let mut physical_found = false;
        let mut device_index = 0usize;

        let pci_devices = pci::enumerate();
        for pci_dev in &pci_devices {
            if pci_dev.vendor_id == VIRTIO_VENDOR_ID
                && (pci_dev.device_id == VIRTIO_GPU_DEVICE_ID_TRANSITIONAL
                    || pci_dev.device_id == VIRTIO_GPU_DEVICE_ID_MODERN)
            {
                if let Ok(dev) = init_physical_device(pci_dev, device_index) {
                    if GPU_MANAGER.register_driver(dev).is_ok() {
                        physical_found = true;
                        device_index += 1;
                    }
                }
            }
        }

        // Register simulated fallback driver if no physical VirtIO GPU was detected
        if !physical_found {
            let default_mode = VideoMode {
                width: 1024,
                height: 768,
                pitch: 1024 * 4,
                bpp: 32,
                format: PixelFormat::Rgba8888,
            };
            let fb = Arc::new(Framebuffer::new(default_mode));
            let sim_dev = Arc::new(VirtioGpuDevice {
                name: alloc::string::String::from("virtio-gpu-simulated"),
                inner: SpinLock::new(VirtioGpuDeviceInner::Simulated {
                    fb,
                    mode: default_mode,
                }),
            });
            let _ = GPU_MANAGER.register_driver(sim_dev);
        }

        Ok(())
    }
}

crate::module_driver!(
    VIRTIO_GPU_INITCALL,
    virtio_gpu_driver_init,
    "virtio_gpu",
    VirtioGpuDriver
);

// ──────────────────────────────────────────────────────────────
// Kernel unit tests
// ──────────────────────────────────────────────────────────────

#[cfg(ktest)]
mod tests {
    use super::*;
    use crate::device::driver::Driver;
    use crate::drivers::gpu::framebuffer::Color;
    use crate::fs::ramfs::RamFs;
    use crate::fs::vfs::{init_root_fs, mount, register_filesystem, resolve_path};
    use ostd::prelude::ktest;

    #[ktest]
    fn test_virtio_gpu_driver() {
        // Probe virtio-gpu driver
        VirtioGpuDriver.probe().unwrap();

        // Retrieve registered driver from GPU manager
        let driver = GPU_MANAGER
            .get_driver("virtio-gpu-simulated")
            .or_else(|| GPU_MANAGER.get_driver("virtio-gpu0"))
            .expect("VirtIO GPU driver registration failed");

        let mode = driver.current_mode();
        assert_eq!(mode.width, 1024);
        assert_eq!(mode.height, 768);

        // Test drawing pixels via driver framebuffer
        let fb = driver.framebuffer();
        let red = Color::RED;
        fb.draw_pixel(100, 100, red);

        {
            let pixels = fb.pixels.lock();
            let offset = (100 * mode.pitch as usize) + (100 * 4);
            assert_eq!(pixels[offset], red.r);
            assert_eq!(pixels[offset + 1], red.g);
            assert_eq!(pixels[offset + 2], red.b);
            assert_eq!(pixels[offset + 3], red.a);
        }

        // Mount devfs and verify /dev/fb0 node
        let ramfs = Arc::new(RamFs);
        let _ = register_filesystem(ramfs);
        let _ = init_root_fs("ramfs", &[]);

        let devfs = Arc::new(crate::fs::devfs::DevFs);
        let _ = register_filesystem(devfs);

        let root = crate::fs::vfs::ROOT_DENTRY
            .lock()
            .as_ref()
            .cloned()
            .unwrap();
        root.inode.mkdir("dev", 0o755).unwrap();

        mount("devfs", "/dev", 0, &[]).unwrap();

        let fb_node = resolve_path("/dev/fb0").expect("/dev/fb0 node should exist");
        assert_eq!(
            fb_node.inode.metadata().unwrap().file_type,
            crate::fs::vfs::FileType::CharDevice
        );

        // Clean up
        crate::fs::vfs::unregister_filesystem("devfs").unwrap();
        crate::fs::vfs::unregister_filesystem("ramfs").unwrap();
        *crate::fs::vfs::ROOT_DENTRY.lock() = None;
        *crate::fs::vfs::CWD_DENTRY.lock() = None;
        crate::fs::vfs::DENTRY_CACHE.clear();
    }
}
