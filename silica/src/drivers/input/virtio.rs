// SPDX-License-Identifier: GPL-2.0

//! VirtIO Input Device Driver (`virtio 1.1` §5.8).
//!
//! Provides support for QEMU / KVM VirtIO input devices (keyboards, mice,
//! tablets, multi-touch devices). Reads capabilities and absolute axis info
//! from device config space, registers an [`InputDevice`], manages `eventq`
//! (Queue 0) for the incoming event stream, and `statusq` (Queue 1) for
//! host feedback (keyboard LEDs, sound, repeat).

use alloc::{
    format,
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};
use core::mem::size_of;
use ostd::{
    mm::{HasPaddr, VmIo, dma::DmaCoherent},
    sync::{LocalIrqDisabled, SpinLock},
};
use spin::Mutex;

use crate::{
    drivers::{
        bus::virtio::{
            SubmitBuffer, VIRTIO_F_VERSION_1, VirtQueue, VirtioDevice, VirtioTransport,
            register_virtio_device,
        },
        input::{Capabilities, InputDevice, register_device_full},
    },
    modules::InitcallLevel,
    uapi::{
        errno::{Errno, Result},
        input::{
            ABS_MAX, INPUT_PROP_MAX, InputAbsInfo, InputId, KEY_MAX, LED_MAX, MSC_MAX, REL_MAX,
            SND_MAX, SW_MAX, bus, ev,
        },
    },
};

/// PCI Device ID for modern VirtIO Input devices (`0x1040 + 18`).
pub const PCI_DEVICE_ID_VIRTIO_INPUT: u16 = 0x1052;

/// VirtQueue indices for VirtIO Input.
pub const EVENT_QUEUE: usize = 0;
pub const STATUS_QUEUE: usize = 1;
pub const QUEUE_COUNT: usize = 2;

/// VirtIO Input configuration selection registers (`virtio 1.1` §5.8.4).
pub const VIRTIO_INPUT_CFG_UNSET: u8 = 0x00;
pub const VIRTIO_INPUT_CFG_ID_NAME: u8 = 0x01;
pub const VIRTIO_INPUT_CFG_ID_SERIAL: u8 = 0x02;
pub const VIRTIO_INPUT_CFG_ID_DEVIDS: u8 = 0x03;
pub const VIRTIO_INPUT_CFG_PROP_BITS: u8 = 0x10;
pub const VIRTIO_INPUT_CFG_EV_BITS: u8 = 0x11;
pub const VIRTIO_INPUT_CFG_ABS_INFO: u8 = 0x12;

/// Maximum number of event descriptors pre-populated in `eventq`.
pub const MAX_EVENT_BUFFERS: usize = 64;

/// A single event transmitted across `eventq` or `statusq` (`virtio 1.1` §5.8.6.1).
#[repr(C)]
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    zerocopy::FromBytes,
    zerocopy::IntoBytes,
    zerocopy::KnownLayout,
    zerocopy::Immutable,
)]
pub struct VirtioInputEvent {
    pub type_: u16,
    pub code: u16,
    pub value: u32,
}

/// Global registry of active `VirtioInput` devices.
static VIRTIO_INPUT_DEVICES: SpinLock<Vec<Arc<VirtioInput>>, LocalIrqDisabled> =
    SpinLock::new(Vec::new());

/// Polls all registered `VirtioInput` devices for pending events in `eventq`.
pub fn poll_all() {
    let devices = VIRTIO_INPUT_DEVICES.lock().clone();
    for dev in devices {
        dev.poll_events();
    }
}

/// Helper function to query device config space by `select` and `subsel`.
fn query_config(
    transport: &VirtioTransport,
    select: u8,
    subsel: u8,
    buf: &mut [u8; 128],
) -> Result<u8> {
    let cmd = (select as u16) | ((subsel as u16) << 8);
    transport.device_write16(0, cmd)?;
    let size = transport.device_read8(2)?;
    let copy_len = (size as usize).min(128);
    if copy_len > 0 {
        transport.device_read_bytes(8, &mut buf[..copy_len])?;
    }
    Ok(size)
}

/// One live `virtio-input` device instance bound to a PCI transport.
pub struct VirtioInput {
    _transport: Arc<VirtioTransport>,
    event_queue: Arc<VirtQueue>,
    status_queue: Arc<VirtQueue>,
    event_dma: DmaCoherent,
    event_count: usize,
    status_staging: Mutex<DmaCoherent>,
    input_device: Arc<InputDevice>,
}

impl VirtioInput {
    /// Creates and configures a new `VirtioInput` instance.
    pub fn new(transport: &Arc<VirtioTransport>) -> Result<Arc<Self>> {
        let Some(event_queue) = transport.queue(EVENT_QUEUE) else {
            crate::return_errno!(ENODEV, "virtio-input: missing event queue");
        };
        let Some(status_queue) = transport.queue(STATUS_QUEUE) else {
            crate::return_errno!(ENODEV, "virtio-input: missing status queue");
        };

        let mut cfg_buf = [0u8; 128];

        // 1. Query device name
        let name_len =
            query_config(transport, VIRTIO_INPUT_CFG_ID_NAME, 0, &mut cfg_buf).unwrap_or(0);
        let name = if name_len > 0 {
            let len = (name_len as usize).min(128);
            core::str::from_utf8(&cfg_buf[..len])
                .unwrap_or("VirtIO Input Device")
                .trim_matches('\0')
                .to_string()
        } else {
            String::from("VirtIO Input Device")
        };

        // 2. Query serial / uniq
        let uniq_len =
            query_config(transport, VIRTIO_INPUT_CFG_ID_SERIAL, 0, &mut cfg_buf).unwrap_or(0);
        let uniq = if uniq_len > 0 {
            let len = (uniq_len as usize).min(128);
            core::str::from_utf8(&cfg_buf[..len])
                .unwrap_or("")
                .trim_matches('\0')
                .to_string()
        } else {
            String::new()
        };

        // 3. Query device IDs
        let ids_len =
            query_config(transport, VIRTIO_INPUT_CFG_ID_DEVIDS, 0, &mut cfg_buf).unwrap_or(0);
        let id = if ids_len >= 8 {
            InputId {
                bustype: u16::from_le_bytes([cfg_buf[0], cfg_buf[1]]),
                vendor: u16::from_le_bytes([cfg_buf[2], cfg_buf[3]]),
                product: u16::from_le_bytes([cfg_buf[4], cfg_buf[5]]),
                version: u16::from_le_bytes([cfg_buf[6], cfg_buf[7]]),
            }
        } else {
            InputId {
                bustype: bus::VIRTUAL,
                vendor: 0x1af4,
                product: 0x1052,
                version: 1,
            }
        };

        let phys = format!("virtio-pci/{}/input0", transport.pci_device_id());

        // 4. Query capabilities and absolute axis metadata
        let mut caps = Capabilities::new();

        // 4a. Properties
        let prop_len =
            query_config(transport, VIRTIO_INPUT_CFG_PROP_BITS, 0, &mut cfg_buf).unwrap_or(0);
        if prop_len > 0 {
            let max_bits = (prop_len as usize * 8).min(INPUT_PROP_MAX + 1);
            for bit in 0..max_bits {
                if (cfg_buf[bit / 8] & (1 << (bit % 8))) != 0 {
                    caps.set_prop(bit as u16);
                }
            }
        }

        // 4b. Event types supported
        let mut supported_ev = [0u8; 128];
        let ev_len =
            query_config(transport, VIRTIO_INPUT_CFG_EV_BITS, 0, &mut cfg_buf).unwrap_or(0);
        if ev_len > 0 {
            let len = (ev_len as usize).min(128);
            supported_ev[..len].copy_from_slice(&cfg_buf[..len]);
        }

        let mut abs_info = [None; ABS_MAX + 1];

        for ev_type in 0..128 {
            if (supported_ev[ev_type / 8] & (1 << (ev_type % 8))) == 0 {
                continue;
            }
            caps.set_ev(ev_type as u16);

            let mut type_buf = [0u8; 128];
            let t_size = query_config(
                transport,
                VIRTIO_INPUT_CFG_EV_BITS,
                ev_type as u8,
                &mut type_buf,
            )
            .unwrap_or(0);
            if t_size == 0 {
                continue;
            }
            let bit_count = t_size as usize * 8;

            match ev_type as u16 {
                ev::KEY => {
                    let max = bit_count.min(KEY_MAX + 1);
                    for code in 0..max {
                        if (type_buf[code / 8] & (1 << (code % 8))) != 0 {
                            caps.set_key(code as u16);
                        }
                    }
                }
                ev::REL => {
                    let max = bit_count.min(REL_MAX + 1);
                    for code in 0..max {
                        if (type_buf[code / 8] & (1 << (code % 8))) != 0 {
                            caps.set_rel(code as u16);
                        }
                    }
                }
                ev::ABS => {
                    let max = bit_count.min(ABS_MAX + 1);
                    for code in 0..max {
                        if (type_buf[code / 8] & (1 << (code % 8))) != 0 {
                            caps.set_abs(code as u16);
                            let mut abs_buf = [0u8; 128];
                            if let Ok(abs_sz) = query_config(
                                transport,
                                VIRTIO_INPUT_CFG_ABS_INFO,
                                code as u8,
                                &mut abs_buf,
                            ) {
                                if abs_sz >= 20 {
                                    let min = i32::from_le_bytes(
                                        abs_buf[0..4].try_into().unwrap_or_default(),
                                    );
                                    let max = i32::from_le_bytes(
                                        abs_buf[4..8].try_into().unwrap_or_default(),
                                    );
                                    let fuzz = i32::from_le_bytes(
                                        abs_buf[8..12].try_into().unwrap_or_default(),
                                    );
                                    let flat = i32::from_le_bytes(
                                        abs_buf[12..16].try_into().unwrap_or_default(),
                                    );
                                    let res = i32::from_le_bytes(
                                        abs_buf[16..20].try_into().unwrap_or_default(),
                                    );
                                    abs_info[code] = Some(InputAbsInfo {
                                        value: 0,
                                        minimum: min,
                                        maximum: max,
                                        fuzz,
                                        flat,
                                        resolution: res,
                                    });
                                }
                            }
                        }
                    }
                }
                ev::MSC => {
                    let max = bit_count.min(MSC_MAX + 1);
                    for code in 0..max {
                        if (type_buf[code / 8] & (1 << (code % 8))) != 0 {
                            caps.set_msc(code as u16);
                        }
                    }
                }
                ev::LED => {
                    let max = bit_count.min(LED_MAX + 1);
                    for code in 0..max {
                        if (type_buf[code / 8] & (1 << (code % 8))) != 0 {
                            caps.set_led(code as u16);
                        }
                    }
                }
                ev::SND => {
                    let max = bit_count.min(SND_MAX + 1);
                    for code in 0..max {
                        if (type_buf[code / 8] & (1 << (code % 8))) != 0 {
                            caps.set_snd(code as u16);
                        }
                    }
                }
                ev::SW => {
                    let max = bit_count.min(SW_MAX + 1);
                    for code in 0..max {
                        if (type_buf[code / 8] & (1 << (code % 8))) != 0 {
                            caps.set_sw(code as u16);
                        }
                    }
                }
                ev::REP => {}
                _ => {}
            }
        }

        // 5. Allocate DMA buffers for eventq receive descriptors
        let event_count = (event_queue.size() as usize).min(MAX_EVENT_BUFFERS);
        let event_dma = DmaCoherent::alloc(1, true).map_err(|_| Errno::ENOMEM)?;
        let base_paddr = event_dma.paddr() as u64;

        for slot in 0..event_count {
            let slot_paddr = base_paddr + (slot * size_of::<VirtioInputEvent>()) as u64;
            event_queue.push_avail_desc(
                slot as u16,
                SubmitBuffer {
                    addr: slot_paddr,
                    len: size_of::<VirtioInputEvent>() as u32,
                    writable: true,
                },
            )?;
        }
        event_queue.notify()?;

        let status_staging = DmaCoherent::alloc(1, true).map_err(|_| Errno::ENOMEM)?;

        // 6. Register input device into core input registry
        let input_device = register_device_full(&name, &phys, &uniq, id, caps, abs_info);

        let virtio_input = Arc::new(Self {
            _transport: Arc::clone(transport),
            event_queue,
            status_queue,
            event_dma,
            event_count,
            status_staging: Mutex::new(status_staging),
            input_device: Arc::clone(&input_device),
        });

        // 7. Connect poller and feedback handler
        let weak_poller = Arc::downgrade(&virtio_input);
        input_device.set_poller(move || {
            if let Some(dev) = weak_poller.upgrade() {
                dev.poll_events();
            }
        });

        let weak_feedback = Arc::downgrade(&virtio_input);
        input_device.set_feedback_handler(move |ty, code, val| {
            if let Some(dev) = weak_feedback.upgrade() {
                let _ = dev.send_status(ty, code, val as u32);
            }
        });

        VIRTIO_INPUT_DEVICES.lock().push(Arc::clone(&virtio_input));

        ostd::info!(
            "virtio-input: initialized \"{}\" (uniq: \"{}\", phys: \"{}\")",
            name,
            uniq,
            phys
        );

        Ok(virtio_input)
    }

    /// Reaps completed event descriptors from `eventq` and passes them to [`InputDevice`].
    pub fn poll_events(&self) {
        let base_paddr = self.event_dma.paddr() as u64;
        let event_sz = size_of::<VirtioInputEvent>();

        while let Ok(Some((desc_id, written_len))) = self.event_queue.pop_used_desc() {
            let slot = desc_id as usize;
            if slot < self.event_count && written_len >= event_sz as u32 {
                let offset = slot * event_sz;
                if let Ok(event) = self.event_dma.read_val::<VirtioInputEvent>(offset) {
                    self.input_device
                        .report(event.type_, event.code, event.value as i32);
                }
            }

            // Recycle descriptor back into eventq
            let slot_paddr = base_paddr + (slot * event_sz) as u64;
            let _ = self.event_queue.push_avail_desc(
                desc_id,
                SubmitBuffer {
                    addr: slot_paddr,
                    len: event_sz as u32,
                    writable: true,
                },
            );
            let _ = self.event_queue.notify();
        }
    }

    /// Transmits a status event across `statusq` (e.g. keyboard LEDs, sound, repeat).
    pub fn send_status(&self, ty: u16, code: u16, value: u32) -> Result<()> {
        let staging = self.status_staging.lock();
        let event = VirtioInputEvent {
            type_: ty,
            code,
            value,
        };
        staging.write_val(0, &event).map_err(|_| Errno::EIO)?;
        let paddr = staging.paddr() as u64;
        self.status_queue.submit_wait(&[SubmitBuffer {
            addr: paddr,
            len: size_of::<VirtioInputEvent>() as u32,
            writable: false,
        }])?;
        Ok(())
    }
}

/// VirtIO bus registration driver for `virtio-input`.
pub struct VirtioInputDriver;

impl VirtioDevice for VirtioInputDriver {
    fn device_id(&self) -> u16 {
        PCI_DEVICE_ID_VIRTIO_INPUT
    }

    fn name(&self) -> &'static str {
        "virtio-input"
    }

    fn queue_count(&self) -> usize {
        QUEUE_COUNT
    }

    fn accept_features(&self, device_features: u64) -> u64 {
        device_features & VIRTIO_F_VERSION_1
    }

    fn on_init(&self, transport: &Arc<VirtioTransport>) -> Result<()> {
        let _dev = VirtioInput::new(transport)?;
        Ok(())
    }
}

/// Background worker thread that periodically polls VirtIO input queues (100 Hz).
fn virtio_input_poller_entry() {
    loop {
        poll_all();
        crate::kpi::time::arm(core::time::Duration::from_millis(10), None).wait();
    }
}

/// Initializes the `virtio-input` driver, registering with the VirtIO bus
/// and starting the background event polling worker.
pub fn init() -> Result<()> {
    register_virtio_device(Arc::new(VirtioInputDriver))?;
    let _ = crate::proc::kthread::kproc_create("virtio_input", virtio_input_poller_entry, 0);
    Ok(())
}

crate::module!(
    "Virtio Input Driver",
    "Ananta",
    InitcallLevel::Device,
    init
);
