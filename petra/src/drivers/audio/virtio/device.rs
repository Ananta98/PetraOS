// SPDX-License-Identifier: GPL-2.0

//! `virtio-snd` PCI device glue and [`AudioDevice`] implementation.
//!
//! [`VirtioSndDriver`] registers with the VirtIO bus and constructs one
//! [`VirtioSnd`] instance per attached PCI transport (`dsp0`, `dsp1`, ...).

use alloc::format;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::mem::size_of;
use core::sync::atomic::{AtomicUsize, Ordering};
use ostd::mm::{
    HasPaddr, PAGE_SIZE, VmIo, VmReader, VmWriter, dma::DmaCoherent, io::util::HasVmReaderWriter,
};
use spin::Mutex;
use zerocopy::{Immutable, IntoBytes};

use super::protocol::{
    CMD_PCM_INFO, CMD_PCM_PREPARE, CMD_PCM_RELEASE, CMD_PCM_SET_PARAMS, CMD_PCM_START,
    CMD_PCM_STOP, CONTROL_QUEUE, DIR_INPUT, DIR_OUTPUT, PCI_DEVICE_ID_VIRTIO_SND, QUEUE_COUNT,
    RX_QUEUE, SndConfig, SndHdr, SndPcmHdr, SndPcmInfo, SndPcmSetParams, SndPcmStatus, SndPcmXfer,
    SndQueryInfo, TX_QUEUE, check_status, negotiate_format, negotiate_rate,
};
use crate::drivers::audio::{
    AudioDevice, PcmConfig, get_audio_device, register_audio_device,
};
use crate::drivers::bus::virtio::{
    SubmitBuffer, VIRTIO_F_VERSION_1, VirtQueue, VirtioDevice, VirtioTransport,
};
use crate::uapi::errno::{Errno, Result};

/// Maximum PCM payload bytes transferred in a single `txq` / `rxq` submission (16 KiB).
const MAX_PCM_XFER_BYTES: usize = 4 * PAGE_SIZE;

/// Maximum number of PCM streams queried during initialization.
const MAX_STREAMS: u32 = 32;

/// Monotonic device index counter for `dspN` naming.
static NEXT_AUDIO_INDEX: AtomicUsize = AtomicUsize::new(0);

/// Lifecycle state of a single VirtIO-SND PCM stream (`virtio 1.2` §5.14.6.6.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StreamPhase {
    Idle,
    Prepared,
    Running,
}

/// Per-direction PCM stream state tracked by [`VirtioSnd`].
#[derive(Debug, Clone)]
struct PcmStreamState {
    stream_id: u32,
    info: SndPcmInfo,
    config: PcmConfig,
    phase: StreamPhase,
}

impl PcmStreamState {
    fn new(stream_id: u32, info: SndPcmInfo) -> Self {
        let mut state = Self {
            stream_id,
            info,
            config: PcmConfig::default(),
            phase: StreamPhase::Idle,
        };
        let (negotiated, _, _) = state.negotiate_params(PcmConfig::default());
        state.config = negotiated;
        state
    }

    /// Clamps and negotiates `requested` against the stream's advertised hardware masks.
    fn negotiate_params(&self, requested: PcmConfig) -> (PcmConfig, u8, u8) {
        let min_ch = self.info.channels_min.max(1);
        let max_ch = self.info.channels_max.max(min_ch);
        let channels = requested.channels.clamp(min_ch, max_ch);

        let (format, fmt_code) = negotiate_format(requested.format, self.info.formats);
        let (sample_rate, rate_code) = negotiate_rate(requested.sample_rate, self.info.rates);

        let frame_bytes = ((channels as usize) * format.bytes_per_sample()).max(1) as u32;
        let raw_period = requested
            .period_bytes
            .clamp(frame_bytes, MAX_PCM_XFER_BYTES as u32);
        let period_bytes = ((raw_period / frame_bytes).max(1)) * frame_bytes;
        let raw_buffer = requested.buffer_bytes.max(period_bytes * 2);
        let buffer_bytes = ((raw_buffer / period_bytes).max(2)) * period_bytes;

        (
            PcmConfig {
                channels,
                format,
                sample_rate,
                buffer_bytes,
                period_bytes,
            },
            fmt_code,
            rate_code,
        )
    }
}

/// Coherent DMA staging buffers for control messages and PCM stream I/O.
struct SndStaging {
    /// Control request and response DMA page.
    ctrl_req_dma: DmaCoherent,
    ctrl_resp_dma: DmaCoherent,
    /// `txq` / `rxq` header (`SndPcmXfer`) and status (`SndPcmStatus`) page.
    xfer_meta_dma: DmaCoherent,
    /// Contiguous PCM sample buffer (`MAX_PCM_XFER_BYTES`).
    pcm_data_dma: DmaCoherent,
}

impl SndStaging {
    fn new() -> Result<Self> {
        let pcm_frames = MAX_PCM_XFER_BYTES / PAGE_SIZE;
        Ok(Self {
            ctrl_req_dma: DmaCoherent::alloc(1, false).map_err(|_| Errno::ENOMEM)?,
            ctrl_resp_dma: DmaCoherent::alloc(1, false).map_err(|_| Errno::ENOMEM)?,
            xfer_meta_dma: DmaCoherent::alloc(1, false).map_err(|_| Errno::ENOMEM)?,
            pcm_data_dma: DmaCoherent::alloc(pcm_frames, false).map_err(|_| Errno::ENOMEM)?,
        })
    }
}

/// Offset of `SndPcmStatus` inside `xfer_meta_dma`.
const XFER_STATUS_OFFSET: usize = 64;

/// One active `virtio-snd` device bound to a PCI transport.
pub struct VirtioSnd {
    _transport: Arc<VirtioTransport>,
    control_queue: Arc<VirtQueue>,
    tx_queue: Option<Arc<VirtQueue>>,
    rx_queue: Option<Arc<VirtQueue>>,
    staging: Mutex<SndStaging>,
    playback: Mutex<Option<PcmStreamState>>,
    capture: Mutex<Option<PcmStreamState>>,
    name: String,
}

impl VirtioSnd {
    /// Initializes a `VirtioSnd` device over an activated [`VirtioTransport`].
    pub fn new(transport: &Arc<VirtioTransport>) -> Result<Arc<Self>> {
        let Some(control_queue) = transport.queue(CONTROL_QUEUE) else {
            ostd::error!(
                "virtio-snd: missing control queue (activated queues = {})",
                transport.queues().len()
            );
            crate::return_errno!(ENODEV, "virtio-snd does not expose a control queue");
        };
        let tx_queue = transport.queue(TX_QUEUE);
        let rx_queue = transport.queue(RX_QUEUE);

        let config = SndConfig::read(transport)?;
        if config.streams == 0 {
            crate::return_errno!(ENODEV, "virtio-snd device reports zero PCM streams");
        }

        let staging = SndStaging::new()?;
        let stream_infos = Self::query_pcm_streams(&control_queue, &staging, config.streams)?;

        let mut playback_stream = None;
        let mut capture_stream = None;
        for (idx, info) in stream_infos.into_iter().enumerate() {
            match info.direction {
                DIR_OUTPUT if playback_stream.is_none() => {
                    playback_stream = Some(PcmStreamState::new(idx as u32, info));
                }
                DIR_INPUT if capture_stream.is_none() => {
                    capture_stream = Some(PcmStreamState::new(idx as u32, info));
                }
                _ => {}
            }
        }

        if playback_stream.is_none() && capture_stream.is_none() {
            crate::return_errno!(ENODEV, "virtio-snd exposes no usable PCM streams");
        }

        let name = allocate_audio_name();
        ostd::info!(
            "virtio-snd: initialized \"{}\" (streams: {}, jacks: {}, chmaps: {}, playback: {}, capture: {})",
            name,
            config.streams,
            config.jacks,
            config.chmaps,
            playback_stream.is_some(),
            capture_stream.is_some(),
        );

        Ok(Arc::new(Self {
            _transport: Arc::clone(transport),
            control_queue,
            tx_queue,
            rx_queue,
            staging: Mutex::new(staging),
            playback: Mutex::new(playback_stream),
            capture: Mutex::new(capture_stream),
            name,
        }))
    }

    /// Queries `VIRTIO_SND_R_PCM_INFO` for all available streams on `controlq`.
    fn query_pcm_streams(
        control_queue: &VirtQueue,
        staging: &SndStaging,
        total_streams: u32,
    ) -> Result<Vec<SndPcmInfo>> {
        let count = total_streams.min(MAX_STREAMS);
        let info_bytes = (count as usize) * size_of::<SndPcmInfo>();
        if size_of::<SndHdr>() + info_bytes > PAGE_SIZE {
            crate::return_errno!(EINVAL, "virtio-snd stream info table exceeds DMA page");
        }

        let req = SndQueryInfo {
            hdr: SndHdr::new(CMD_PCM_INFO),
            start_id: 0,
            count,
            size: size_of::<SndPcmInfo>() as u32,
        };
        staging
            .ctrl_req_dma
            .write_val(0, &req)
            .map_err(|_| Errno::EIO)?;

        let req_paddr = staging.ctrl_req_dma.paddr() as u64;
        let resp_paddr = staging.ctrl_resp_dma.paddr() as u64;

        control_queue.submit_wait(&[
            SubmitBuffer {
                addr: req_paddr,
                len: size_of::<SndQueryInfo>() as u32,
                writable: false,
            },
            SubmitBuffer {
                addr: resp_paddr,
                len: size_of::<SndHdr>() as u32,
                writable: true,
            },
            SubmitBuffer {
                addr: resp_paddr + size_of::<SndHdr>() as u64,
                len: info_bytes as u32,
                writable: true,
            },
        ])?;

        let resp_hdr: SndHdr = staging
            .ctrl_resp_dma
            .read_val(0)
            .map_err(|_| Errno::EIO)?;
        check_status(resp_hdr.code)?;

        let mut streams = Vec::with_capacity(count as usize);
        for i in 0..(count as usize) {
            let offset = size_of::<SndHdr>() + i * size_of::<SndPcmInfo>();
            let info: SndPcmInfo = staging
                .ctrl_resp_dma
                .read_val(offset)
                .map_err(|_| Errno::EIO)?;
            streams.push(info);
        }
        Ok(streams)
    }

    /// Sends a control request on `controlq` and checks the `SndHdr` status code.
    fn submit_control_cmd<T: IntoBytes + Immutable>(
        &self,
        staging: &SndStaging,
        cmd: &T,
    ) -> Result<()> {
        let bytes = cmd.as_bytes();
        if bytes.len() > PAGE_SIZE {
            crate::return_errno!(EINVAL, "virtio-snd control command too large");
        }
        staging
            .ctrl_req_dma
            .writer()
            .write(&mut VmReader::from(bytes));

        self.control_queue.submit_wait(&[
            SubmitBuffer {
                addr: staging.ctrl_req_dma.paddr() as u64,
                len: bytes.len() as u32,
                writable: false,
            },
            SubmitBuffer {
                addr: staging.ctrl_resp_dma.paddr() as u64,
                len: size_of::<SndHdr>() as u32,
                writable: true,
            },
        ])?;

        let resp: SndHdr = staging
            .ctrl_resp_dma
            .read_val(0)
            .map_err(|_| Errno::EIO)?;
        check_status(resp.code)
    }

    /// Stops and releases `stream` if it is currently prepared or running.
    fn tear_down_stream(&self, staging: &SndStaging, stream: &mut PcmStreamState) -> Result<()> {
        if stream.phase == StreamPhase::Running {
            let stop_cmd = SndPcmHdr::new(CMD_PCM_STOP, stream.stream_id);
            let _ = self.submit_control_cmd(staging, &stop_cmd);
            stream.phase = StreamPhase::Prepared;
        }
        if stream.phase == StreamPhase::Prepared {
            let release_cmd = SndPcmHdr::new(CMD_PCM_RELEASE, stream.stream_id);
            let _ = self.submit_control_cmd(staging, &release_cmd);
            stream.phase = StreamPhase::Idle;
        }
        Ok(())
    }

    /// Applies `requested` parameters to `stream` (`SET_PARAMS` + `PREPARE`).
    fn configure_stream(
        &self,
        staging: &SndStaging,
        stream: &mut PcmStreamState,
        requested: PcmConfig,
    ) -> Result<PcmConfig> {
        self.tear_down_stream(staging, stream)?;

        let (negotiated, fmt_code, rate_code) = stream.negotiate_params(requested);
        let set_params = SndPcmSetParams {
            hdr: SndPcmHdr::new(CMD_PCM_SET_PARAMS, stream.stream_id),
            buffer_bytes: negotiated.buffer_bytes,
            period_bytes: negotiated.period_bytes,
            features: 0,
            channels: negotiated.channels,
            format: fmt_code,
            rate: rate_code,
            padding: 0,
        };
        self.submit_control_cmd(staging, &set_params)?;

        let prepare_cmd = SndPcmHdr::new(CMD_PCM_PREPARE, stream.stream_id);
        self.submit_control_cmd(staging, &prepare_cmd)?;

        stream.config = negotiated;
        stream.phase = StreamPhase::Prepared;
        Ok(negotiated)
    }

    /// Ensures `stream` has been configured, prepared, and started (`StreamPhase::Running`).
    fn ensure_stream_running(
        &self,
        staging: &SndStaging,
        stream: &mut PcmStreamState,
    ) -> Result<()> {
        if stream.phase == StreamPhase::Idle {
            self.configure_stream(staging, stream, stream.config)?;
        }
        if stream.phase == StreamPhase::Prepared {
            let start_cmd = SndPcmHdr::new(CMD_PCM_START, stream.stream_id);
            self.submit_control_cmd(staging, &start_cmd)?;
            stream.phase = StreamPhase::Running;
        }
        Ok(())
    }
}

impl AudioDevice for VirtioSnd {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn playback_config(&self) -> PcmConfig {
        self.playback
            .lock()
            .as_ref()
            .map(|s| s.config)
            .unwrap_or_default()
    }

    fn set_playback_config(&self, config: PcmConfig) -> Result<PcmConfig> {
        let mut guard = self.playback.lock();
        let stream = guard.as_mut().ok_or(Errno::ENODEV)?;
        let staging = self.staging.lock();
        self.configure_stream(&staging, stream, config)
    }

    fn capture_config(&self) -> PcmConfig {
        self.capture
            .lock()
            .as_ref()
            .map(|s| s.config)
            .unwrap_or_default()
    }

    fn set_capture_config(&self, config: PcmConfig) -> Result<PcmConfig> {
        let mut guard = self.capture.lock();
        let stream = guard.as_mut().ok_or(Errno::ENODEV)?;
        let staging = self.staging.lock();
        self.configure_stream(&staging, stream, config)
    }

    fn write_pcm(&self, samples: &[u8]) -> Result<usize> {
        if samples.is_empty() {
            return Ok(0);
        }
        let Some(ref tx_queue) = self.tx_queue else {
            crate::return_errno!(ENODEV, "virtio-snd has no playback queue");
        };

        let mut playback_guard = self.playback.lock();
        let stream = playback_guard.as_mut().ok_or(Errno::ENODEV)?;
        let staging = self.staging.lock();

        self.ensure_stream_running(&staging, stream)?;

        let frame_bytes = stream.config.frame_bytes().max(1);
        let max_chunk = (stream.config.period_bytes as usize)
            .min(MAX_PCM_XFER_BYTES)
            .max(frame_bytes);
        let max_aligned = (max_chunk / frame_bytes) * frame_bytes;

        let xfer_hdr = SndPcmXfer {
            stream_id: stream.stream_id,
        };
        staging
            .xfer_meta_dma
            .write_val(0, &xfer_hdr)
            .map_err(|_| Errno::EIO)?;

        let meta_paddr = staging.xfer_meta_dma.paddr() as u64;
        let data_paddr = staging.pcm_data_dma.paddr() as u64;

        let mut offset = 0;
        while offset < samples.len() {
            let remaining = samples.len() - offset;
            let raw_chunk = remaining.min(max_aligned);
            let chunk_len = if raw_chunk >= frame_bytes {
                (raw_chunk / frame_bytes) * frame_bytes
            } else {
                raw_chunk
            };

            let mut reader = VmReader::from(&samples[offset..offset + chunk_len]);
            staging.pcm_data_dma.writer().write(&mut reader);

            let zero_status = SndPcmStatus::default();
            staging
                .xfer_meta_dma
                .write_val(XFER_STATUS_OFFSET, &zero_status)
                .map_err(|_| Errno::EIO)?;

            tx_queue.submit_wait(&[
                SubmitBuffer {
                    addr: meta_paddr,
                    len: size_of::<SndPcmXfer>() as u32,
                    writable: false,
                },
                SubmitBuffer {
                    addr: data_paddr,
                    len: chunk_len as u32,
                    writable: false,
                },
                SubmitBuffer {
                    addr: meta_paddr + XFER_STATUS_OFFSET as u64,
                    len: size_of::<SndPcmStatus>() as u32,
                    writable: true,
                },
            ])?;

            let status: SndPcmStatus = staging
                .xfer_meta_dma
                .read_val(XFER_STATUS_OFFSET)
                .map_err(|_| Errno::EIO)?;
            check_status(status.status)?;

            offset += chunk_len;
        }

        Ok(offset)
    }

    fn read_pcm(&self, buf: &mut [u8]) -> Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let Some(ref rx_queue) = self.rx_queue else {
            crate::return_errno!(ENODEV, "virtio-snd has no capture queue");
        };

        let mut capture_guard = self.capture.lock();
        let stream = capture_guard.as_mut().ok_or(Errno::ENODEV)?;
        let staging = self.staging.lock();

        self.ensure_stream_running(&staging, stream)?;

        let frame_bytes = stream.config.frame_bytes().max(1);
        let max_chunk = (stream.config.period_bytes as usize)
            .min(MAX_PCM_XFER_BYTES)
            .min(buf.len())
            .max(frame_bytes);
        let chunk_len = if max_chunk >= frame_bytes {
            (max_chunk / frame_bytes) * frame_bytes
        } else {
            buf.len().min(MAX_PCM_XFER_BYTES)
        };

        let xfer_hdr = SndPcmXfer {
            stream_id: stream.stream_id,
        };
        staging
            .xfer_meta_dma
            .write_val(0, &xfer_hdr)
            .map_err(|_| Errno::EIO)?;
        let zero_status = SndPcmStatus::default();
        staging
            .xfer_meta_dma
            .write_val(XFER_STATUS_OFFSET, &zero_status)
            .map_err(|_| Errno::EIO)?;

        let meta_paddr = staging.xfer_meta_dma.paddr() as u64;
        let data_paddr = staging.pcm_data_dma.paddr() as u64;

        rx_queue.submit_wait(&[
            SubmitBuffer {
                addr: meta_paddr,
                len: size_of::<SndPcmXfer>() as u32,
                writable: false,
            },
            SubmitBuffer {
                addr: data_paddr,
                len: chunk_len as u32,
                writable: true,
            },
            SubmitBuffer {
                addr: meta_paddr + XFER_STATUS_OFFSET as u64,
                len: size_of::<SndPcmStatus>() as u32,
                writable: true,
            },
        ])?;

        let status: SndPcmStatus = staging
            .xfer_meta_dma
            .read_val(XFER_STATUS_OFFSET)
            .map_err(|_| Errno::EIO)?;
        check_status(status.status)?;

        let mut reader = staging.pcm_data_dma.reader();
        let mut writer = VmWriter::from(&mut buf[..chunk_len]);
        reader.read(&mut writer);

        Ok(chunk_len)
    }

    fn drain(&self) -> Result<()> {
        let mut playback_guard = self.playback.lock();
        let staging = self.staging.lock();
        if let Some(ref mut stream) = *playback_guard
            && stream.phase == StreamPhase::Running
        {
            let stop_cmd = SndPcmHdr::new(CMD_PCM_STOP, stream.stream_id);
            self.submit_control_cmd(&staging, &stop_cmd)?;
            stream.phase = StreamPhase::Prepared;
        }
        Ok(())
    }

    fn reset(&self) -> Result<()> {
        let mut playback_guard = self.playback.lock();
        let mut capture_guard = self.capture.lock();
        let staging = self.staging.lock();
        if let Some(ref mut stream) = *playback_guard {
            self.tear_down_stream(&staging, stream)?;
        }
        if let Some(ref mut stream) = *capture_guard {
            self.tear_down_stream(&staging, stream)?;
        }
        Ok(())
    }
}

/// Allocates the next unused `dspN` device name.
fn allocate_audio_name() -> String {
    loop {
        let idx = NEXT_AUDIO_INDEX.fetch_add(1, Ordering::Relaxed);
        let candidate = format!("dsp{}", idx);
        if get_audio_device(&candidate).is_none() {
            return candidate;
        }
    }
}

/// VirtIO bus registration adapter for `virtio-snd`.
pub struct VirtioSndDriver;

impl VirtioDevice for VirtioSndDriver {
    fn device_id(&self) -> u16 {
        PCI_DEVICE_ID_VIRTIO_SND
    }

    fn name(&self) -> &'static str {
        "virtio-snd"
    }

    fn queue_count(&self) -> usize {
        QUEUE_COUNT
    }

    fn accept_features(&self, device_features: u64) -> u64 {
        device_features & VIRTIO_F_VERSION_1
    }

    fn on_init(&self, transport: &Arc<VirtioTransport>) -> Result<()> {
        let snd = VirtioSnd::new(transport)?;
        register_audio_device(snd)?;
        Ok(())
    }
}
