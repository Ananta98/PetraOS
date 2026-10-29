// SPDX-License-Identifier: GPL-2.0

//! `/proc/bus/input/devices` and `/proc/bus/input/handlers`.
//!
//! Provides Linux-compatible device enumeration views for `libinput`,
//! `udev`, Xorg, and desktop utilities.

use alloc::{format, string::String, sync::Arc, vec::Vec};

use crate::{
    drivers::input,
    fs::procfs::template::{ProcDirOps, ProcFileOps, dir_node, file_node},
    fs::vfs::INode,
    uapi::{errno::Result, input::ev},
};

pub(crate) const PROC_BUS_INPUT_INO: usize = 3001;
pub(crate) const PROC_BUS_INPUT_DEVICES_INO: usize = 3002;
pub(crate) const PROC_BUS_INPUT_HANDLERS_INO: usize = 3003;

/// Formats a bitmap as space-separated hexadecimal words (Linux format).
fn format_bitmap(bytes: &[u8]) -> String {
    let last = bytes.iter().rposition(|&b| b != 0).unwrap_or(0);
    let slice = &bytes[..=last];
    let mut words = Vec::new();
    for chunk in slice.chunks(8) {
        let mut val = 0u64;
        for (i, &b) in chunk.iter().enumerate() {
            val |= (b as u64) << (i * 8);
        }
        words.push(val);
    }
    if words.is_empty() {
        return format!("0");
    }
    words.reverse();
    let mut parts = Vec::new();
    for (i, w) in words.iter().enumerate() {
        if i == 0 {
            parts.push(format!("{:x}", w));
        } else {
            parts.push(format!("{:016x}", w));
        }
    }
    parts.join(" ")
}

/// Content generator for `/proc/bus/input/devices`.
pub struct DevicesFileOps;

impl ProcFileOps for DevicesFileOps {
    fn generate_content(&self) -> Vec<u8> {
        let mut out = String::new();
        for device in input::devices() {
            let id = device.id();
            let caps = device.capabilities();

            out.push_str(&format!(
                "I: Bus={:04x} Vendor={:04x} Product={:04x} Version={:04x}\n",
                id.bustype, id.vendor, id.product, id.version
            ));
            out.push_str(&format!("N: Name=\"{}\"\n", device.name()));
            out.push_str(&format!("P: Phys={}\n", device.phys()));
            out.push_str(&format!(
                "S: Sysfs=/devices/virtual/input/{}\n",
                device.node_name()
            ));
            out.push_str(&format!("U: Uniq={}\n", device.uniq()));

            // Determine handlers
            let mut handlers = Vec::new();
            let mut ev_bits = [0u8; 4];
            caps.bitmap(ev::SYN, &mut ev_bits);

            let mut key_bits = [0u8; 128];
            caps.bitmap(ev::KEY, &mut key_bits);
            let has_keys = key_bits.iter().any(|&b| b != 0);

            let mut rel_bits = [0u8; 4];
            caps.bitmap(ev::REL, &mut rel_bits);
            let has_rel = rel_bits.iter().any(|&b| b != 0);

            if has_keys {
                handlers.push("kbd");
            }
            if has_rel {
                handlers.push("mouse0");
            }
            let node_name = device.node_name();
            handlers.push(&node_name);

            out.push_str(&format!("H: Handlers={}\n", handlers.join(" ")));

            let mut prop_bits = [0u8; 4];
            device.properties(&mut prop_bits);
            out.push_str(&format!("B: PROP={}\n", format_bitmap(&prop_bits)));
            out.push_str(&format!("B: EV={}\n", format_bitmap(&ev_bits)));

            if has_keys {
                out.push_str(&format!("B: KEY={}\n", format_bitmap(&key_bits)));
            }
            if has_rel {
                out.push_str(&format!("B: REL={}\n", format_bitmap(&rel_bits)));
            }

            let mut abs_bits = [0u8; 8];
            caps.bitmap(ev::ABS, &mut abs_bits);
            if abs_bits.iter().any(|&b| b != 0) {
                out.push_str(&format!("B: ABS={}\n", format_bitmap(&abs_bits)));
            }

            let mut msc_bits = [0u8; 4];
            caps.bitmap(ev::MSC, &mut msc_bits);
            if msc_bits.iter().any(|&b| b != 0) {
                out.push_str(&format!("B: MSC={}\n", format_bitmap(&msc_bits)));
            }

            let mut led_bits = [0u8; 4];
            caps.bitmap(ev::LED, &mut led_bits);
            if led_bits.iter().any(|&b| b != 0) {
                out.push_str(&format!("B: LED={}\n", format_bitmap(&led_bits)));
            }

            let mut snd_bits = [0u8; 4];
            caps.bitmap(ev::SND, &mut snd_bits);
            if snd_bits.iter().any(|&b| b != 0) {
                out.push_str(&format!("B: SND={}\n", format_bitmap(&snd_bits)));
            }

            let mut sw_bits = [0u8; 4];
            caps.bitmap(ev::SW, &mut sw_bits);
            if sw_bits.iter().any(|&b| b != 0) {
                out.push_str(&format!("B: SW={}\n", format_bitmap(&sw_bits)));
            }

            out.push('\n');
        }

        out.into_bytes()
    }
}

/// Content generator for `/proc/bus/input/handlers`.
pub struct HandlersFileOps;

impl ProcFileOps for HandlersFileOps {
    fn generate_content(&self) -> Vec<u8> {
        b"N: Number=0 Name=kbd\nN: Number=1 Name=mousedev Minor=32\nN: Number=2 Name=evdev Minor=64\n"
            .to_vec()
    }
}

/// Directory operations for `/proc/bus/input`.
pub struct ProcBusInputDirOps;

impl ProcDirOps for ProcBusInputDirOps {
    fn lookup_child(&self, name: &str) -> Result<Arc<INode>> {
        match name {
            "devices" => Ok(file_node(DevicesFileOps, PROC_BUS_INPUT_DEVICES_INO)),
            "handlers" => Ok(file_node(HandlersFileOps, PROC_BUS_INPUT_HANDLERS_INO)),
            _ => crate::return_errno!(ENOENT, "unknown entry in /proc/bus/input"),
        }
    }

    fn list_entries(&self) -> Vec<String> {
        alloc::vec![String::from("devices"), String::from("handlers")]
    }
}

/// Creates the `/proc/bus/input` VFS inode.
pub fn new_input_inode() -> Arc<INode> {
    dir_node(ProcBusInputDirOps, PROC_BUS_INPUT_INO)
}
