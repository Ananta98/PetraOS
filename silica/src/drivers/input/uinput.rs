// SPDX-License-Identifier: GPL-2.0

//! uinput: userspace-created input devices (`/dev/input/uinput`).
//!
//! The protocol is the Linux one: the caller declares capabilities with
//! `UI_SET_*BIT`, writes one `uinput_user_dev` for the name/id, then
//! `UI_DEV_CREATE`. Every later write is an array of `struct input_event` that
//! the kernel injects through the normal input core path, so the virtual device
//! behaves exactly like a hardware one — `eventN` appears, evdev clients read
//! it, and the TTY keyboard handler sees its keys.

use alloc::{string::String, sync::Arc};
use spin::Mutex;

use super::{
    Capabilities, InputDevice, register_device_full, unregister_device,
};
use crate::uapi::{
    errno::{Errno, Result},
    input::{
        ABS_MAX, INPUT_PROP_MAX, InputAbsInfo, InputEvent, InputId, KEY_MAX, LED_MAX, MSC_MAX,
        REL_MAX, SND_MAX, SW_MAX, UINPUT_MAX_NAME_SIZE, UinputAbsSetup, UinputSetup, bus, ev,
        ioctl,
    },
};

/// Mutable uinput state, one per open file description.
struct UinputState {
    name: String,
    id: InputId,
    caps: Capabilities,
    abs_info: [Option<InputAbsInfo>; ABS_MAX + 1],
    /// Set once the description header has been parsed (via write or `UI_DEV_SETUP`).
    described: bool,
    /// The live device after `UI_DEV_CREATE`.
    device: Option<Arc<InputDevice>>,
}

/// An open `/dev/input/uinput` file.
pub struct UinputFile {
    state: Mutex<UinputState>,
}

impl UinputFile {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(UinputState {
                name: String::from("uinput device"),
                id: InputId {
                    bustype: bus::VIRTUAL,
                    ..InputId::default()
                },
                caps: Capabilities::new(),
                abs_info: [None; ABS_MAX + 1],
                described: false,
                device: None,
            }),
        }
    }

    /// Handles `UI_*` requests.
    pub fn ioctl(&self, cmd: u32, arg: usize) -> Result<usize> {
        let proc = crate::proc::thread::Thread::proc_or_esrch()?;
        let vmar = proc.vmspace();
        let ptr = crate::utils::userspace::user_ptr::UserPtr::new(arg);
        let mut state = self.state.lock();

        match cmd {
            ioctl::UI_GET_VERSION => {
                let version = ioctl::UINPUT_VERSION;
                ptr.write_bytes(&vmar, &version.to_ne_bytes())?;
                Ok(0)
            }
            ioctl::UI_DEV_CREATE => {
                if state.device.is_some() {
                    return Err(Errno::EINVAL);
                }
                let device = register_device_full(
                    &state.name,
                    "uinput",
                    "uinput",
                    state.id,
                    state.caps,
                    state.abs_info,
                );
                state.device = Some(device);
                Ok(0)
            }
            ioctl::UI_DEV_DESTROY => {
                if let Some(device) = state.device.take() {
                    unregister_device(&device);
                }
                Ok(0)
            }
            ioctl::UI_DEV_SETUP => {
                if state.device.is_some() {
                    return Err(Errno::EINVAL);
                }
                let mut buf = [0u8; UinputSetup::SIZE];
                ptr.read_bytes(&vmar, &mut buf)?;
                let Some(setup) = UinputSetup::from_bytes(&buf) else {
                    return Err(Errno::EINVAL);
                };
                let end = setup
                    .name
                    .iter()
                    .position(|&b| b == 0)
                    .unwrap_or(UINPUT_MAX_NAME_SIZE);
                if let Ok(name) = core::str::from_utf8(&setup.name[..end]) {
                    if !name.is_empty() {
                        state.name = String::from(name);
                    }
                }
                state.id = setup.id;
                state.described = true;
                Ok(0)
            }
            ioctl::UI_ABS_SETUP => {
                if state.device.is_some() {
                    return Err(Errno::EINVAL);
                }
                let mut buf = [0u8; UinputAbsSetup::SIZE];
                ptr.read_bytes(&vmar, &mut buf)?;
                let Some(setup) = UinputAbsSetup::from_bytes(&buf) else {
                    return Err(Errno::EINVAL);
                };
                if (setup.code as usize) <= ABS_MAX {
                    state.abs_info[setup.code as usize] = Some(setup.absinfo);
                    state.caps.set_abs(setup.code);
                }
                Ok(0)
            }
            _ if crate::uapi::ioctl::ioc_type(cmd) == ioctl::UI_TYPE
                && crate::uapi::ioctl::ioc_nr(cmd) == 44 =>
            {
                // UI_GET_SYSNAME(len)
                let Some(device) = state.device.as_ref() else {
                    return Err(Errno::EINVAL);
                };
                let len = crate::uapi::ioctl::ioc_size(cmd);
                let name = device.node_name();
                let bytes = name.as_bytes();
                let n = len.min(bytes.len() + 1);
                let mut buf = alloc::vec![0u8; n];
                let payload = n.min(bytes.len());
                buf[..payload].copy_from_slice(&bytes[..payload]);
                ptr.write_bytes(&vmar, &buf)?;
                Ok(n)
            }
            ioctl::UI_SET_PHYS => {
                // The physical path is informational; the node name suffices.
                Ok(0)
            }
            ioctl::UI_SET_PROPBIT => {
                let index: i32 = ptr.read_val(&vmar)?;
                if index < 0 || index as usize > INPUT_PROP_MAX {
                    return Err(Errno::EINVAL);
                }
                state.caps.set_prop(index as u16);
                Ok(0)
            }
            ioctl::UI_SET_EVBIT
            | ioctl::UI_SET_KEYBIT
            | ioctl::UI_SET_RELBIT
            | ioctl::UI_SET_ABSBIT
            | ioctl::UI_SET_MSCBIT
            | ioctl::UI_SET_LEDBIT
            | ioctl::UI_SET_SNDBIT
            | ioctl::UI_SET_SWBIT => {
                let index: i32 = ptr.read_val(&vmar)?;
                if index < 0 {
                    return Err(Errno::EINVAL);
                }
                let code = index as u16;
                match cmd {
                    ioctl::UI_SET_EVBIT => state.caps.set_ev(code),
                    ioctl::UI_SET_KEYBIT => state.caps.set_key(code),
                    ioctl::UI_SET_RELBIT => state.caps.set_rel(code),
                    ioctl::UI_SET_ABSBIT => state.caps.set_abs(code),
                    ioctl::UI_SET_MSCBIT => state.caps.set_msc(code),
                    ioctl::UI_SET_LEDBIT => state.caps.set_led(code),
                    ioctl::UI_SET_SNDBIT => state.caps.set_snd(code),
                    _ => state.caps.set_sw(code),
                }
                Ok(0)
            }
            ioctl::UI_SET_FFBIT => {
                // Force feedback is not modelled; accepting keeps standard initialisation working.
                Ok(0)
            }
            _ => Err(Errno::ENOTTY),
        }
    }


    /// Consumes a `write(2)` on the uinput node.
    ///
    /// The first write carries the `uinput_user_dev` description; later writes
    /// are event arrays. A write after `UI_DEV_CREATE` is always an event array,
    /// matching Linux' `uinput_write`.
    pub fn write(&self, buf: &[u8]) -> Result<usize> {
        let mut state = self.state.lock();

        if !state.described {
            if buf.len() < UINPUT_MAX_NAME_SIZE + InputId::SIZE {
                return Err(Errno::EINVAL);
            }
            let name_bytes = &buf[..UINPUT_MAX_NAME_SIZE];
            let end = name_bytes
                .iter()
                .position(|&b| b == 0)
                .unwrap_or(UINPUT_MAX_NAME_SIZE);
            if let Ok(name) = core::str::from_utf8(&name_bytes[..end]) {
                state.name = String::from(name);
            }
            if let Some(id) = InputId::from_bytes(&buf[UINPUT_MAX_NAME_SIZE..]) {
                state.id = id;
            }
            state.described = true;
            // The rest of the legacy setup struct (absolute ranges, ff effects)
            // is not modelled, so only the header is consumed.
            return Ok(buf.len());
        }

        let Some(device) = state.device.clone() else {
            return Err(Errno::EINVAL);
        };
        drop(state);

        if buf.len() % InputEvent::SIZE != 0 {
            return Err(Errno::EINVAL);
        }
        for chunk in buf.chunks_exact(InputEvent::SIZE) {
            let event = InputEvent {
                time: crate::time::Timeval::from_bytes(&chunk[0..16]).unwrap_or_default(),
                type_: u16::from_ne_bytes([chunk[16], chunk[17]]),
                code: u16::from_ne_bytes([chunk[18], chunk[19]]),
                value: i32::from_ne_bytes([chunk[20], chunk[21], chunk[22], chunk[23]]),
            };
            if !is_valid(&event) {
                return Err(Errno::EINVAL);
            }
            device.inject(event);
        }
        Ok(buf.len())
    }
}

impl Drop for UinputFile {
    fn drop(&mut self) {
        if let Some(device) = self.state.lock().device.take() {
            unregister_device(&device);
        }
    }
}

/// Rejects event type/code pairs that no real device could report.
fn is_valid(event: &InputEvent) -> bool {
    let code = event.code as usize;
    match event.type_ {
        ev::SYN => event.code <= ev::SYN_DROPPED,
        ev::KEY => code <= KEY_MAX,
        ev::REL => code <= REL_MAX,
        ev::ABS => code <= ABS_MAX,
        ev::MSC => code <= MSC_MAX,
        ev::LED => code <= LED_MAX,
        ev::SND => code <= SND_MAX,
        ev::SW => code <= SW_MAX,
        ev::REP | ev::FF | ev::PWR | ev::FF_STATUS => true,
        _ => false,
    }
}
