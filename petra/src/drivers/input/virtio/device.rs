// SPDX-License-Identifier: GPL-2.0

//! VirtIO Input Device Implementation.

use alloc::{
    format,
    string::{String, ToString},
    sync::Arc,
};
use core::mem::size_of;
use ostd::mm::{HasPaddr, VmIo, dma::DmaCoherent};
use spin::Mutex;

use crate::{
    drivers::{
        bus::virtio::{SubmitBuffer, VirtQueue, VirtioTransport},
        input::{Capabilities, InputDevice, register_device_full},
    },
    uapi::{
        errno::{Errno, Result},
        input::{
            ABS_MAX, INPUT_PROP_MAX, InputAbsInfo, InputId, KEY_MAX, LED_MAX, MSC_MAX, REL_MAX,
            SND_MAX, SW_MAX, bus, ev,
        },
    },
};

use super::protocol::*;

/// Helper function to query device config space by `select` and `subsel`.
pub fn query_config(
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

fn query_virtio_abs_info(transport: &VirtioTransport, code: u8) -> Option<InputAbsInfo> {
    let mut abs_buf = [0u8; 128];
    let abs_sz = query_config(transport, VIRTIO_INPUT_CFG_ABS_INFO, code, &mut abs_buf).ok()?;
    if abs_sz < 20 {
        return None;
    }
    let minimum = i32::from_le_bytes(abs_buf[0..4].try_into().unwrap_or_default());
    let maximum = i32::from_le_bytes(abs_buf[4..8].try_into().unwrap_or_default());
    let fuzz = i32::from_le_bytes(abs_buf[8..12].try_into().unwrap_or_default());
    let flat = i32::from_le_bytes(abs_buf[12..16].try_into().unwrap_or_default());
    let resolution = i32::from_le_bytes(abs_buf[16..20].try_into().unwrap_or_default());

    Some(InputAbsInfo {
        value: 0,
        minimum,
        maximum,
        fuzz,
        flat,
        resolution,
    })
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
                            abs_info[code] = query_virtio_abs_info(transport, code as u8);
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

        super::register_active_device(Arc::clone(&virtio_input));

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
