// SPDX-License-Identifier: GPL-2.0

//! Virtio sound device wire protocol (`virtio 1.2` §5.14).
//!
//! Defines PCI device IDs, virtqueue indices, feature bits, `virtio_snd_config`
//! layout, control request/response structures, PCM format/rate bitmasks, and
//! stream I/O headers for `virtio-snd`.

use core::mem::size_of;
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

use crate::drivers::audio::PcmFormat;
use crate::drivers::bus::virtio::VirtioTransport;
use crate::uapi::errno::{Errno, Result};

/// `virtio-snd` modern PCI device ID (`0x1040 + VIRTIO_ID_SOUND`, where `VIRTIO_ID_SOUND = 25`).
pub const PCI_DEVICE_ID_VIRTIO_SND: u16 = 0x1059;

/// Control virtqueue index (`controlq`).
pub const CONTROL_QUEUE: usize = 0;
/// Event virtqueue index (`eventq`).
pub const EVENT_QUEUE: usize = 1;
/// PCM playback (guest → host) virtqueue index (`txq`).
pub const TX_QUEUE: usize = 2;
/// PCM capture (host → guest) virtqueue index (`rxq`).
pub const RX_QUEUE: usize = 3;
/// Total number of virtqueues in `virtio-snd`.
pub const QUEUE_COUNT: usize = 4;

// ----------------------------------------------------------------------------
// Feature bits (`virtio 1.2` §5.14.3)
// ----------------------------------------------------------------------------

/// `VIRTIO_SND_F_CTLS`: Device supports audio control elements.
pub const FEATURE_CTLS: u64 = 1 << 0;

// ----------------------------------------------------------------------------
// Device configuration layout (`struct virtio_snd_config`, `virtio 1.2` §5.14.4)
// ----------------------------------------------------------------------------

/// Offset of `jacks` (`u32`).
pub const CONFIG_JACKS_OFFSET: usize = 0;
/// Offset of `streams` (`u32`).
pub const CONFIG_STREAMS_OFFSET: usize = 4;
/// Offset of `chmaps` (`u32`).
pub const CONFIG_CHMAPS_OFFSET: usize = 8;

/// Snapshot of `struct virtio_snd_config`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SndConfig {
    /// Total number of available jacks.
    pub jacks: u32,
    /// Total number of available PCM streams.
    pub streams: u32,
    /// Total number of available channel maps.
    pub chmaps: u32,
}

impl SndConfig {
    /// Reads `virtio_snd_config` from the device-specific configuration space.
    pub fn read(transport: &VirtioTransport) -> Result<Self> {
        let jacks = transport.device_read32(CONFIG_JACKS_OFFSET).unwrap_or(0);
        let streams = transport.device_read32(CONFIG_STREAMS_OFFSET)?;
        let chmaps = transport.device_read32(CONFIG_CHMAPS_OFFSET).unwrap_or(0);
        Ok(Self {
            jacks,
            streams,
            chmaps,
        })
    }
}

// ----------------------------------------------------------------------------
// Control request opcodes and response status codes (`virtio 1.2` §5.14.6)
// ----------------------------------------------------------------------------

/// Query jack information (`VIRTIO_SND_R_JACK_INFO`).
pub const CMD_JACK_INFO: u32 = 1;
/// Remap jack (`VIRTIO_SND_R_JACK_REMAP`).
pub const CMD_JACK_REMAP: u32 = 2;

/// Query PCM stream capabilities (`VIRTIO_SND_R_PCM_INFO`).
pub const CMD_PCM_INFO: u32 = 0x0100;
/// Set PCM stream parameters (`VIRTIO_SND_R_PCM_SET_PARAMS`).
pub const CMD_PCM_SET_PARAMS: u32 = 0x0101;
/// Prepare PCM stream (`VIRTIO_SND_R_PCM_PREPARE`).
pub const CMD_PCM_PREPARE: u32 = 0x0102;
/// Release PCM stream (`VIRTIO_SND_R_PCM_RELEASE`).
pub const CMD_PCM_RELEASE: u32 = 0x0103;
/// Start PCM stream (`VIRTIO_SND_R_PCM_START`).
pub const CMD_PCM_START: u32 = 0x0104;
/// Stop PCM stream (`VIRTIO_SND_R_PCM_STOP`).
pub const CMD_PCM_STOP: u32 = 0x0105;

/// Query channel map information (`VIRTIO_SND_R_CHMAP_INFO`).
pub const CMD_CHMAP_INFO: u32 = 0x0200;

/// Request succeeded (`VIRTIO_SND_S_OK`).
pub const STATUS_OK: u32 = 0x8000;
/// Driver sent a malformed message (`VIRTIO_SND_S_BAD_MSG`).
pub const STATUS_BAD_MSG: u32 = 0x8001;
/// Requested operation or parameters not supported (`VIRTIO_SND_S_NOT_SUPP`).
pub const STATUS_NOT_SUPP: u32 = 0x8002;
/// Device I/O error (`VIRTIO_SND_S_IO_ERR`).
pub const STATUS_IO_ERR: u32 = 0x8003;

/// Maps a `virtio-snd` status code into `Ok(())` or a kernel [`Errno`].
pub fn check_status(code: u32) -> Result<()> {
    match code {
        STATUS_OK => Ok(()),
        STATUS_BAD_MSG => Err(Errno::EINVAL),
        STATUS_NOT_SUPP => Err(Errno::ENOTSUP),
        STATUS_IO_ERR => Err(Errno::EIO),
        _ => Err(Errno::EIO),
    }
}

// ----------------------------------------------------------------------------
// Data flow directions, PCM formats, and sample rates (`virtio 1.2` §5.14.6.6)
// ----------------------------------------------------------------------------

/// Output / playback direction (`VIRTIO_SND_D_OUTPUT`, guest → host).
pub const DIR_OUTPUT: u8 = 0;
/// Input / capture direction (`VIRTIO_SND_D_INPUT`, host → guest).
pub const DIR_INPUT: u8 = 1;

/// `VIRTIO_SND_PCM_FMT_U8`: Unsigned 8-bit integer PCM.
pub const PCM_FMT_U8: u8 = 4;
/// `VIRTIO_SND_PCM_FMT_S16`: Signed 16-bit integer PCM.
pub const PCM_FMT_S16: u8 = 5;
/// `VIRTIO_SND_PCM_FMT_S24`: Signed 24-bit integer PCM (in 4-byte container).
pub const PCM_FMT_S24: u8 = 15;
/// `VIRTIO_SND_PCM_FMT_S32`: Signed 32-bit integer PCM.
pub const PCM_FMT_S32: u8 = 17;
/// `VIRTIO_SND_PCM_FMT_FLOAT`: 32-bit IEEE float PCM.
pub const PCM_FMT_FLOAT: u8 = 19;

/// `VIRTIO_SND_PCM_RATE_5512` (5512 Hz).
pub const PCM_RATE_5512: u8 = 0;
/// `VIRTIO_SND_PCM_RATE_8000` (8000 Hz).
pub const PCM_RATE_8000: u8 = 1;
/// `VIRTIO_SND_PCM_RATE_11025` (11025 Hz).
pub const PCM_RATE_11025: u8 = 2;
/// `VIRTIO_SND_PCM_RATE_16000` (16000 Hz).
pub const PCM_RATE_16000: u8 = 3;
/// `VIRTIO_SND_PCM_RATE_22050` (22050 Hz).
pub const PCM_RATE_22050: u8 = 4;
/// `VIRTIO_SND_PCM_RATE_32000` (32000 Hz).
pub const PCM_RATE_32000: u8 = 5;
/// `VIRTIO_SND_PCM_RATE_44100` (44100 Hz).
pub const PCM_RATE_44100: u8 = 6;
/// `VIRTIO_SND_PCM_RATE_48000` (48000 Hz).
pub const PCM_RATE_48000: u8 = 7;
/// `VIRTIO_SND_PCM_RATE_64000` (64000 Hz).
pub const PCM_RATE_64000: u8 = 8;
/// `VIRTIO_SND_PCM_RATE_88200` (88200 Hz).
pub const PCM_RATE_88200: u8 = 9;
/// `VIRTIO_SND_PCM_RATE_96000` (96000 Hz).
pub const PCM_RATE_96000: u8 = 10;
/// `VIRTIO_SND_PCM_RATE_176400` (176400 Hz).
pub const PCM_RATE_176400: u8 = 11;
/// `VIRTIO_SND_PCM_RATE_192000` (192000 Hz).
pub const PCM_RATE_192000: u8 = 12;
/// `VIRTIO_SND_PCM_RATE_384000` (384000 Hz).
pub const PCM_RATE_384000: u8 = 13;

/// Table of `(VIRTIO_SND_PCM_RATE_*, Hz)` ordered from lowest to highest rate.
const RATE_TABLE: [(u8, u32); 14] = [
    (PCM_RATE_5512, 5_512),
    (PCM_RATE_8000, 8_000),
    (PCM_RATE_11025, 11_025),
    (PCM_RATE_16000, 16_000),
    (PCM_RATE_22050, 22_050),
    (PCM_RATE_32000, 32_000),
    (PCM_RATE_44100, 44_100),
    (PCM_RATE_48000, 48_000),
    (PCM_RATE_64000, 64_000),
    (PCM_RATE_88200, 88_200),
    (PCM_RATE_96000, 96_000),
    (PCM_RATE_176400, 176_400),
    (PCM_RATE_192000, 192_000),
    (PCM_RATE_384000, 384_000),
];

/// Converts a kernel [`PcmFormat`] into its `VIRTIO_SND_PCM_FMT_*` code.
pub const fn pcm_format_to_wire(fmt: PcmFormat) -> u8 {
    match fmt {
        PcmFormat::U8 => PCM_FMT_U8,
        PcmFormat::S16Le => PCM_FMT_S16,
        PcmFormat::S24Le => PCM_FMT_S24,
        PcmFormat::S32Le => PCM_FMT_S32,
        PcmFormat::FloatLe => PCM_FMT_FLOAT,
    }
}

/// Selects `requested` if supported by `supported_mask`, or falls back to the
/// best supported [`PcmFormat`] (`S16Le`, `S32Le`, `FloatLe`, `S24Le`, `U8`).
pub fn negotiate_format(requested: PcmFormat, supported_mask: u64) -> (PcmFormat, u8) {
    let candidates = [
        requested,
        PcmFormat::S16Le,
        PcmFormat::S32Le,
        PcmFormat::FloatLe,
        PcmFormat::S24Le,
        PcmFormat::U8,
    ];
    for candidate in candidates {
        let code = pcm_format_to_wire(candidate);
        if supported_mask == 0 || (supported_mask & (1u64 << code)) != 0 {
            return (candidate, code);
        }
    }
    (PcmFormat::S16Le, PCM_FMT_S16)
}

/// Selects the supported sample rate closest to `requested_hz` from `supported_mask`.
pub fn negotiate_rate(requested_hz: u32, supported_mask: u64) -> (u32, u8) {
    let mut best: Option<(u8, u32, u32)> = None;
    for (code, hz) in RATE_TABLE {
        if supported_mask != 0 && (supported_mask & (1u64 << code)) == 0 {
            continue;
        }
        let diff = requested_hz.abs_diff(hz);
        if best.is_none_or(|(_, _, best_diff)| diff < best_diff) {
            best = Some((code, hz, diff));
        }
    }
    match best {
        Some((code, hz, _)) => (hz, code),
        None => (48_000, PCM_RATE_48000),
    }
}

// ----------------------------------------------------------------------------
// Wire structures (`virtio 1.2` §5.14.6)
// ----------------------------------------------------------------------------

/// Generic 4-byte control header (`struct virtio_snd_hdr`).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, FromBytes, IntoBytes, KnownLayout, Immutable)]
pub struct SndHdr {
    /// Request opcode (`CMD_*`) or response status (`STATUS_*`).
    pub code: u32,
}

impl SndHdr {
    pub const fn new(code: u32) -> Self {
        Self { code }
    }
}

/// Item information query request (`struct virtio_snd_query_info`, 16 bytes).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, FromBytes, IntoBytes, KnownLayout, Immutable)]
pub struct SndQueryInfo {
    /// Control request header (`CMD_JACK_INFO`, `CMD_PCM_INFO`, `CMD_CHMAP_INFO`).
    pub hdr: SndHdr,
    /// Starting item identifier.
    pub start_id: u32,
    /// Number of items to query.
    pub count: u32,
    /// Size of a single information structure in bytes.
    pub size: u32,
}

/// Common item info header (`struct virtio_snd_info`, 4 bytes).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, FromBytes, IntoBytes, KnownLayout, Immutable)]
pub struct SndInfo {
    /// High Definition Audio function group node ID.
    pub hda_fn_nid: u32,
}

/// PCM stream capability descriptor (`struct virtio_snd_pcm_info`, 32 bytes).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, FromBytes, IntoBytes, KnownLayout, Immutable)]
pub struct SndPcmInfo {
    /// Common info header (`hda_fn_nid`).
    pub hdr: SndInfo,
    /// Supported feature bitmask (`1 << VIRTIO_SND_PCM_F_*`).
    pub features: u32,
    /// Supported sample format bitmask (`1 << VIRTIO_SND_PCM_FMT_*`).
    pub formats: u64,
    /// Supported frame rate bitmask (`1 << VIRTIO_SND_PCM_RATE_*`).
    pub rates: u64,
    /// Stream direction (`DIR_OUTPUT` or `DIR_INPUT`).
    pub direction: u8,
    /// Minimum number of supported channels.
    pub channels_min: u8,
    /// Maximum number of supported channels.
    pub channels_max: u8,
    /// Explicit zero padding.
    pub padding: [u8; 5],
}

/// Header for stream-specific PCM control requests (`struct virtio_snd_pcm_hdr`, 8 bytes).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, FromBytes, IntoBytes, KnownLayout, Immutable)]
pub struct SndPcmHdr {
    /// Control request code (`CMD_PCM_*`).
    pub hdr: SndHdr,
    /// Target PCM stream index (`0..streams`).
    pub stream_id: u32,
}

impl SndPcmHdr {
    pub const fn new(code: u32, stream_id: u32) -> Self {
        Self {
            hdr: SndHdr::new(code),
            stream_id,
        }
    }
}

/// `VIRTIO_SND_R_PCM_SET_PARAMS` payload (`struct virtio_snd_pcm_set_params`, 24 bytes).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, FromBytes, IntoBytes, KnownLayout, Immutable)]
pub struct SndPcmSetParams {
    /// Stream command header (`CMD_PCM_SET_PARAMS`).
    pub hdr: SndPcmHdr,
    /// Size of the hardware ring buffer in bytes.
    pub buffer_bytes: u32,
    /// Size of a single playback/capture period in bytes.
    pub period_bytes: u32,
    /// Selected stream feature bitmask (`0` for standard transfers).
    pub features: u32,
    /// Number of audio channels.
    pub channels: u8,
    /// Wire sample format code (`PCM_FMT_*`).
    pub format: u8,
    /// Wire frame rate code (`PCM_RATE_*`).
    pub rate: u8,
    /// Explicit zero padding.
    pub padding: u8,
}

/// Prefix of every PCM data transfer buffer on `txq` and `rxq`
/// (`struct virtio_snd_pcm_xfer`, 4 bytes).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, FromBytes, IntoBytes, KnownLayout, Immutable)]
pub struct SndPcmXfer {
    /// Stream index (`0..streams`).
    pub stream_id: u32,
}

/// Trailer written by the device after completing a PCM transfer on `txq` or `rxq`
/// (`struct virtio_snd_pcm_status`, 8 bytes).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, FromBytes, IntoBytes, KnownLayout, Immutable)]
pub struct SndPcmStatus {
    /// Completion status (`STATUS_OK`, `STATUS_IO_ERR`, ...).
    pub status: u32,
    /// Current device latency in bytes.
    pub latency_bytes: u32,
}

const _: () = {
    assert!(size_of::<SndHdr>() == 4);
    assert!(size_of::<SndQueryInfo>() == 16);
    assert!(size_of::<SndInfo>() == 4);
    assert!(size_of::<SndPcmInfo>() == 32);
    assert!(size_of::<SndPcmHdr>() == 8);
    assert!(size_of::<SndPcmSetParams>() == 24);
    assert!(size_of::<SndPcmXfer>() == 4);
    assert!(size_of::<SndPcmStatus>() == 8);
};
