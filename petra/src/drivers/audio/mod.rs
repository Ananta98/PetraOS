// SPDX-License-Identifier: GPL-2.0

//! Audio Device Driver Abstractions.
//!
//! Provides the core [`AudioDevice`] trait, PCM stream format and parameter
//! definitions ([`PcmFormat`], [`PcmConfig`]), an OSS-compatible `/dev/dsp`
//! character device adapter ([`AudioCharDevice`]), and audio device drivers
//! such as `virtio-snd`.

pub mod virtio;

use alloc::{string::String, sync::Arc, vec::Vec};

use crate::drivers::{CharacterDevice, get_audiodevs, register_audiodev, register_chrdev, unregister_audiodev, unregister_chrdev};
use crate::proc::thread::Thread;
use crate::uapi::errno::{Errno, Result};
use crate::uapi::ioctl::{_io, _ior, _iowr};
use crate::utils::userspace::UserPtr;

// ----------------------------------------------------------------------------
// OSS `/dev/dsp` ioctl request numbers (`linux/soundcard.h`)
// ----------------------------------------------------------------------------

const OSS_IOC_MAGIC: u32 = b'P' as u32;

/// `SNDCTL_DSP_RESET`: Halt current operation and reset the device.
pub const SNDCTL_DSP_RESET: u32 = _io(OSS_IOC_MAGIC, 0);
/// `SNDCTL_DSP_SYNC`: Drain queued playback samples before returning.
pub const SNDCTL_DSP_SYNC: u32 = _io(OSS_IOC_MAGIC, 1);
/// `SNDCTL_DSP_SPEED`: Set or query the PCM sample rate in Hz.
pub const SNDCTL_DSP_SPEED: u32 = _iowr::<i32>(OSS_IOC_MAGIC, 2);
/// `SNDCTL_DSP_STEREO`: Set stereo (`1`) or mono (`0`).
pub const SNDCTL_DSP_STEREO: u32 = _iowr::<i32>(OSS_IOC_MAGIC, 3);
/// `SNDCTL_DSP_GETBLKSIZE`: Query the period / fragment size in bytes.
pub const SNDCTL_DSP_GETBLKSIZE: u32 = _iowr::<i32>(OSS_IOC_MAGIC, 4);
/// `SNDCTL_DSP_SETFMT`: Set or query the sample format (`AFMT_*`).
pub const SNDCTL_DSP_SETFMT: u32 = _iowr::<i32>(OSS_IOC_MAGIC, 5);
/// `SNDCTL_DSP_CHANNELS`: Set or query the number of audio channels.
pub const SNDCTL_DSP_CHANNELS: u32 = _iowr::<i32>(OSS_IOC_MAGIC, 6);
/// `SNDCTL_DSP_GETFMTS`: Return bitmask of supported `AFMT_*` sample formats.
pub const SNDCTL_DSP_GETFMTS: u32 = _ior::<i32>(OSS_IOC_MAGIC, 11);

/// `AFMT_QUERY`: Query current format without changing it.
pub const AFMT_QUERY: i32 = 0x0000_0000;
/// `AFMT_U8`: Unsigned 8-bit PCM.
pub const AFMT_U8: i32 = 0x0000_0008;
/// `AFMT_S16_LE`: Signed 16-bit little-endian PCM.
pub const AFMT_S16_LE: i32 = 0x0000_0010;
/// `AFMT_S32_LE`: Signed 32-bit little-endian PCM.
pub const AFMT_S32_LE: i32 = 0x0000_1000;
/// `AFMT_FLOAT`: 32-bit IEEE float PCM.
pub const AFMT_FLOAT: i32 = 0x0000_4000;
/// `AFMT_S24_LE`: Signed 24-bit (in 32-bit container) little-endian PCM.
pub const AFMT_S24_LE: i32 = 0x0000_8000;

/// Default PCM sample rate (48 kHz).
pub const DEFAULT_SAMPLE_RATE: u32 = 48_000;
/// Default PCM channel count (stereo).
pub const DEFAULT_CHANNELS: u8 = 2;
/// Default ring buffer size in bytes (64 KiB).
pub const DEFAULT_BUFFER_BYTES: u32 = 65_536;
/// Default period (fragment) size in bytes (16 KiB).
pub const DEFAULT_PERIOD_BYTES: u32 = 16_384;

/// Supported PCM sample formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PcmFormat {
    /// Unsigned 8-bit integer PCM (`U8`).
    U8,
    /// Signed 16-bit little-endian PCM (`S16_LE`).
    #[default]
    S16Le,
    /// Signed 24-bit little-endian PCM stored in 32-bit words (`S24_LE`).
    S24Le,
    /// Signed 32-bit little-endian PCM (`S32_LE`).
    S32Le,
    /// 32-bit IEEE-754 floating-point little-endian PCM (`FLOAT_LE`).
    FloatLe,
}

impl PcmFormat {
    /// Returns the number of bytes occupied by a single channel sample.
    pub const fn bytes_per_sample(self) -> usize {
        match self {
            Self::U8 => 1,
            Self::S16Le => 2,
            Self::S24Le | Self::S32Le | Self::FloatLe => 4,
        }
    }

    /// Converts an OSS `AFMT_*` constant into a [`PcmFormat`].
    pub const fn from_oss_afmt(afmt: i32) -> Option<Self> {
        match afmt {
            AFMT_U8 => Some(Self::U8),
            AFMT_S16_LE => Some(Self::S16Le),
            AFMT_S24_LE => Some(Self::S24Le),
            AFMT_S32_LE => Some(Self::S32Le),
            AFMT_FLOAT => Some(Self::FloatLe),
            _ => None,
        }
    }

    /// Converts this [`PcmFormat`] into its corresponding OSS `AFMT_*` bit.
    pub const fn to_oss_afmt(self) -> i32 {
        match self {
            Self::U8 => AFMT_U8,
            Self::S16Le => AFMT_S16_LE,
            Self::S24Le => AFMT_S24_LE,
            Self::S32Le => AFMT_S32_LE,
            Self::FloatLe => AFMT_FLOAT,
        }
    }
}

/// Configuration parameters for a PCM playback or capture stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PcmConfig {
    /// Number of interleaved channels (e.g. 1 = mono, 2 = stereo).
    pub channels: u8,
    /// Sample encoding format.
    pub format: PcmFormat,
    /// Sample rate in Hz (e.g. 44100, 48000).
    pub sample_rate: u32,
    /// Total stream buffer size in bytes.
    pub buffer_bytes: u32,
    /// Notification period size in bytes.
    pub period_bytes: u32,
}

impl Default for PcmConfig {
    fn default() -> Self {
        Self {
            channels: DEFAULT_CHANNELS,
            format: PcmFormat::S16Le,
            sample_rate: DEFAULT_SAMPLE_RATE,
            buffer_bytes: DEFAULT_BUFFER_BYTES,
            period_bytes: DEFAULT_PERIOD_BYTES,
        }
    }
}

impl PcmConfig {
    /// Returns the size in bytes of one interleaved multi-channel audio frame.
    pub const fn frame_bytes(&self) -> usize {
        let ch = if self.channels == 0 {
            1
        } else {
            self.channels as usize
        };
        ch * self.format.bytes_per_sample()
    }
}

/// A kernel audio PCM device (e.g. `virtio-snd` controller).
pub trait AudioDevice: Send + Sync {
    /// Unique audio device name (e.g. `"dsp0"`).
    fn name(&self) -> String;

    /// Returns the active playback stream configuration.
    fn playback_config(&self) -> PcmConfig;

    /// Reconfigures the playback stream with `config`, returning the negotiated [`PcmConfig`].
    fn set_playback_config(&self, config: PcmConfig) -> Result<PcmConfig>;

    /// Returns the active capture stream configuration.
    fn capture_config(&self) -> PcmConfig;

    /// Reconfigures the capture stream with `config`, returning the negotiated [`PcmConfig`].
    fn set_capture_config(&self, config: PcmConfig) -> Result<PcmConfig>;

    /// Writes interleaved PCM frames to the playback stream.
    fn write_pcm(&self, samples: &[u8]) -> Result<usize>;

    /// Reads interleaved PCM frames from the capture stream.
    fn read_pcm(&self, buf: &mut [u8]) -> Result<usize>;

    /// Waits for pending playback buffers to finish and stops active streams.
    fn drain(&self) -> Result<()> {
        Ok(())
    }

    /// Stops and releases active streams, restoring default state.
    fn reset(&self) -> Result<()> {
        Ok(())
    }
}

/// Character device wrapper exposing an [`AudioDevice`] in `/dev` (e.g. `/dev/dsp0`, `/dev/dsp`).
pub struct AudioCharDevice {
    dev_name: String,
    audio: Arc<dyn AudioDevice>,
}

impl AudioCharDevice {
    /// Creates a character device node named `dev_name` backed by `audio`.
    pub fn new(dev_name: String, audio: Arc<dyn AudioDevice>) -> Arc<Self> {
        Arc::new(Self { dev_name, audio })
    }
}

impl CharacterDevice for AudioCharDevice {
    fn name(&self) -> String {
        self.dev_name.clone()
    }

    fn read(&self, buf: &mut [u8]) -> Result<usize> {
        self.audio.read_pcm(buf)
    }

    fn write(&self, buf: &[u8]) -> Result<usize> {
        self.audio.write_pcm(buf)
    }

    fn ioctl(&self, cmd: u32, arg: usize) -> Result<usize> {
        match cmd {
            SNDCTL_DSP_RESET => {
                self.audio.reset()?;
                Ok(0)
            }
            SNDCTL_DSP_SYNC => {
                self.audio.drain()?;
                Ok(0)
            }
            SNDCTL_DSP_GETFMTS => {
                let proc = Thread::proc_or_esrch()?;
                let mask = AFMT_U8 | AFMT_S16_LE | AFMT_S24_LE | AFMT_S32_LE | AFMT_FLOAT;
                UserPtr::new(arg).write_val(&proc.vmspace(), mask)?;
                Ok(0)
            }
            SNDCTL_DSP_SETFMT => {
                let proc = Thread::proc_or_esrch()?;
                let vmar = proc.vmspace();
                let requested: i32 = UserPtr::new(arg).read_val(&vmar)?;
                let mut cfg = self.audio.playback_config();
                if requested != AFMT_QUERY {
                    let fmt = PcmFormat::from_oss_afmt(requested).ok_or(Errno::EINVAL)?;
                    cfg.format = fmt;
                    cfg = self.audio.set_playback_config(cfg)?;
                }
                UserPtr::new(arg).write_val(&vmar, cfg.format.to_oss_afmt())?;
                Ok(0)
            }
            SNDCTL_DSP_CHANNELS => {
                let proc = Thread::proc_or_esrch()?;
                let vmar = proc.vmspace();
                let requested: i32 = UserPtr::new(arg).read_val(&vmar)?;
                let mut cfg = self.audio.playback_config();
                if requested > 0 {
                    cfg.channels = requested.clamp(1, 8) as u8;
                    cfg = self.audio.set_playback_config(cfg)?;
                }
                UserPtr::new(arg).write_val(&vmar, cfg.channels as i32)?;
                Ok(0)
            }
            SNDCTL_DSP_STEREO => {
                let proc = Thread::proc_or_esrch()?;
                let vmar = proc.vmspace();
                let stereo: i32 = UserPtr::new(arg).read_val(&vmar)?;
                let mut cfg = self.audio.playback_config();
                cfg.channels = if stereo != 0 { 2 } else { 1 };
                cfg = self.audio.set_playback_config(cfg)?;
                let actual_stereo = if cfg.channels >= 2 { 1i32 } else { 0i32 };
                UserPtr::new(arg).write_val(&vmar, actual_stereo)?;
                Ok(0)
            }
            SNDCTL_DSP_SPEED => {
                let proc = Thread::proc_or_esrch()?;
                let vmar = proc.vmspace();
                let requested: i32 = UserPtr::new(arg).read_val(&vmar)?;
                let mut cfg = self.audio.playback_config();
                if requested > 0 {
                    cfg.sample_rate = requested as u32;
                    cfg = self.audio.set_playback_config(cfg)?;
                }
                UserPtr::new(arg).write_val(&vmar, cfg.sample_rate as i32)?;
                Ok(0)
            }
            SNDCTL_DSP_GETBLKSIZE => {
                let proc = Thread::proc_or_esrch()?;
                let cfg = self.audio.playback_config();
                UserPtr::new(arg).write_val(&proc.vmspace(), cfg.period_bytes as i32)?;
                Ok(0)
            }
            _ => crate::return_errno!(ENOTTY, "unsupported audio ioctl"),
        }
    }

    fn poll(&self) -> Result<u32> {
        // POLLIN (0x001) | POLLOUT (0x004) | POLLRDNORM (0x040) | POLLWRNORM (0x100)
        Ok(0x001 | 0x004 | 0x040 | 0x100)
    }
}

/// Registers an audio device in the audio device class and exposes `/dev/dspN`
/// (plus `/dev/dsp` for the primary audio device).
pub fn register_audio_device(device: Arc<dyn AudioDevice>) -> Result<()> {
    let name = device.name();
    let cfg = device.playback_config();
    ostd::info!(
        "Audio: Registered audio device \"/dev/{}\" ({} Hz, {} ch, {:?})",
        name,
        cfg.sample_rate,
        cfg.channels,
        cfg.format,
    );
    register_audiodev(Arc::clone(&device))?;
    let chr = AudioCharDevice::new(name.clone(), Arc::clone(&device));
    register_chrdev(chr)?;

    if name == "dsp0" {
        let alias = AudioCharDevice::new(String::from("dsp"), device);
        let _ = register_chrdev(alias);
    }
    Ok(())
}

/// Unregisters an audio device by name.
pub fn unregister_audio_device(name: &str) -> Result<()> {
    let _ = unregister_chrdev(name);
    if name == "dsp0" {
        let _ = unregister_chrdev("dsp");
    }
    unregister_audiodev(name)
}

/// Retrieves a registered audio device by name.
pub fn get_audio_device(name: &str) -> Option<Arc<dyn AudioDevice>> {
    get_audiodevs().into_iter().find(|d| d.name() == name)
}

/// Lists all registered audio device names.
pub fn list_audio_devices() -> Vec<String> {
    get_audiodevs().into_iter().map(|d| d.name()).collect()
}
