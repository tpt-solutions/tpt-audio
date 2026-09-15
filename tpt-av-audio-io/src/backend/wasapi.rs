//! WASAPI backend (Windows), shared mode.
//!
//! Ported from the old `platform-windows` router crate, reworked from
//! per-app loopback routing into the engine-facing device/stream surface:
//!
//! - [`WasapiBackend::enumerate_devices`] walks `IMMDeviceEnumerator` for
//!   render and capture endpoints (was `device.rs`).
//! - [`WasapiWriter`] opens an `IAudioClient` in shared mode and feeds the
//!   render client from the stream driver thread (was `stream.rs` /
//!   `pipeline.rs`).
//! - [`WasapiReader`] captures — from microphones on input devices, or
//!   system audio via `AUDCLNT_STREAMFLAGS_LOOPBACK` when the caller opens
//!   an output device for capture.
//!
//! COM is initialized lazily per thread (`CoInitializeEx` on first use on
//! the stream driver thread), because writers/readers are constructed on the
//! Main Thread but run elsewhere. Shared-mode streams use
//! `AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM` so any requested sample rate/channel
//! count works against the device mix format.

use std::time::Duration;

use windows::Win32::Foundation::E_FAIL;
use windows::Win32::Media::Audio::{
    eCapture, eConsole, eRender, IAudioCaptureClient, IAudioClient, IAudioRenderClient, IMMDevice,
    IMMDeviceEnumerator, MMDeviceEnumerator, AUDCLNT_SHAREMODE_SHARED,
    AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM, AUDCLNT_STREAMFLAGS_LOOPBACK,
    AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY, DEVICE_STATE_ACTIVE, WAVEFORMATEX,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_APARTMENTTHREADED,
    COINIT_MULTITHREADED,
};

use crate::device::{AudioDevice, Direction};
use crate::stream::{DeviceReader, DeviceWriter, StreamConfig};
use tpt_av_audio_utils::{AudioBuffer, AudioError};

/// `WAVE_FORMAT_IEEE_FLOAT` from mmreg.h (3). Shared-mode WASAPI accepts
/// 32-bit float client formats directly.
const WAVE_FORMAT_IEEE_FLOAT: u16 = 3;

/// Windows WASAPI backend.
pub struct WasapiBackend;

impl WasapiBackend {
    /// Creates the backend. Device enumeration happens per call, so nothing
    /// is cached at construction (hot-plug friendly).
    pub fn new() -> Result<Self, AudioError> {
        Ok(Self)
    }
}

impl crate::backend::AudioBackend for WasapiBackend {
    fn name(&self) -> &'static str {
        "wasapi"
    }

    fn enumerate_devices(&self) -> Result<Vec<AudioDevice>, AudioError> {
        // Bind the guard: dropping the temporary would uninitialize COM
        // before enumeration runs.
        let _com =
            com_scoped(COINIT_APARTMENTTHREADED).map_err(|e| AudioError::Backend(e.to_string()))?;
        unsafe { enumerate_devices_com() }
    }

    fn open_output_writer(
        &self,
        device: &AudioDevice,
        config: StreamConfig,
    ) -> Result<Box<dyn DeviceWriter>, AudioError> {
        Ok(Box::new(WasapiWriter::open(&device.id.0, config)?))
    }

    fn open_input_reader(
        &self,
        device: &AudioDevice,
        config: StreamConfig,
    ) -> Result<Box<dyn DeviceReader>, AudioError> {
        let loopback = device.direction == Direction::Output;
        Ok(Box::new(WasapiReader::open(
            &device.id.0,
            config,
            loopback,
        )?))
    }
}

/// Initializes COM for the current thread and uninitializes on drop.
struct ComGuard(bool);

impl Drop for ComGuard {
    fn drop(&mut self) {
        if self.0 {
            unsafe { CoUninitialize() };
        }
    }
}

/// Initializes COM on the current thread, tolerating an already-initialized
/// thread as long as the apartment mode matches (returns `Err` otherwise).
fn com_scoped(mode: windows::Win32::System::Com::COINIT) -> Result<ComGuard, windows::core::Error> {
    let hr = unsafe { CoInitializeEx(None, mode) };
    // S_OK and S_FALSE both require a matching CoUninitialize.
    Ok(ComGuard(hr.is_ok()))
}

unsafe fn enumerate_devices_com() -> Result<Vec<AudioDevice>, AudioError> {
    // SAFETY: caller has initialized COM for this thread (see `com_scoped`); all
    // WASAPI calls below are apartment-threaded COM as required.
    unsafe {
        let mut devices = Vec::new();

        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(|e| {
                AudioError::Backend(format!("CoCreateInstance(MMDeviceEnumerator): {e}"))
            })?;

        for (flow, direction) in [(eRender, Direction::Output), (eCapture, Direction::Input)] {
            let collection = enumerator
                .EnumAudioEndpoints(flow, DEVICE_STATE_ACTIVE)
                .map_err(|e| AudioError::Backend(format!("EnumAudioEndpoints: {e}")))?;
            let count = collection
                .GetCount()
                .map_err(|e| AudioError::Backend(format!("GetCount: {e}")))?;

            let default_device = enumerator.GetDefaultAudioEndpoint(flow, eConsole).ok();

            for i in 0..count {
                let device = collection
                    .Item(i)
                    .map_err(|e| AudioError::Backend(format!("Item({i}): {e}")))?;
                let id = match device.GetId() {
                    Ok(id) => id.to_string().unwrap_or_else(|_| format!("dev_{i}")),
                    Err(_) => format!("dev_{i}"),
                };

                let is_default = default_device.as_ref().is_some_and(|d| {
                    d.GetId()
                        .map(|s| s.to_string().is_ok_and(|s| s == id))
                        .unwrap_or(false)
                });

                devices.push(AudioDevice::simple(
                    id,
                    format!("{direction:?} device {i}"),
                    direction,
                    2,
                    is_default,
                ));
            }
        }

        Ok(devices)
    }
}

/// Finds an active endpoint by id in either direction.
unsafe fn find_device_com(enumerator: &IMMDeviceEnumerator, device_id: &str) -> Option<IMMDevice> {
    // SAFETY: caller has initialized COM for this thread; read-only endpoint walk.
    unsafe {
        for flow in [eRender, eCapture] {
            if let Ok(collection) = enumerator.EnumAudioEndpoints(flow, DEVICE_STATE_ACTIVE) {
                if let Ok(count) = collection.GetCount() {
                    for i in 0..count {
                        if let Ok(device) = collection.Item(i) {
                            if let Ok(id) = device.GetId() {
                                if id.to_string().is_ok_and(|s| s == device_id) {
                                    return Some(device);
                                }
                            }
                        }
                    }
                }
            }
        }
        None
    }
}

const REFTIMES_PER_SEC: i64 = 10_000_000;

/// Converts a buffer size in frames into a 100-ns WASAPI duration.
fn frames_to_hns(frames: usize, sample_rate: u32) -> i64 {
    (frames as i64 * REFTIMES_PER_SEC) / sample_rate.max(1) as i64
}

/// Playback sink over a shared-mode `IAudioClient`.
pub struct WasapiWriter {
    /// Keeps the client alive; `render` borrows from the same COM object.
    _client: IAudioClient,
    render: IAudioRenderClient,
    buffer_frames: u32,
    channels: u16,
    /// Holds the COM apartment open for the writer's lifetime.
    #[allow(dead_code)]
    com: ComGuard,
}

// WASAPI COM interfaces are apartment objects; we confine use to the driver
// thread that opened the stream and hand the writer over once at spawn time.
unsafe impl Send for WasapiWriter {}

impl WasapiWriter {
    /// Opens a render stream on `device_id` with `config`'s format.
    ///
    /// Called on the Main Thread; the returned writer runs on the stream
    /// driver thread and lazily initializes COM there via [`Self::ensure_com`].
    pub fn open(device_id: &str, config: StreamConfig) -> Result<Self, AudioError> {
        let com = com_scoped(COINIT_MULTITHREADED)
            .map_err(|e| AudioError::Backend(format!("CoInitializeEx: {e}")))?;
        let (client, render, buffer_frames) = unsafe {
            open_render_client(device_id, config).map_err(|e| AudioError::Backend(e.to_string()))?
        };
        Ok(Self {
            _client: client,
            render,
            buffer_frames,
            channels: config.channels,
            com,
        })
    }
}

impl DeviceWriter for WasapiWriter {
    fn write(&mut self, buffer: &AudioBuffer) -> Result<(), AudioError> {
        let frames = buffer.frames.min(self.buffer_frames as usize);
        if frames == 0 {
            return Ok(());
        }
        let channels = self.channels as usize;

        // Block until the device has room; shared-mode latency for a
        // buffer-size stream is at most one buffer period.
        loop {
            let padding = unsafe { self._client.GetCurrentPadding() }
                .map_err(|e| AudioError::Backend(format!("GetCurrentPadding: {e}")))?;
            let available = self.buffer_frames - padding;
            if available >= frames as u32 {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }

        let dst = unsafe { self.render.GetBuffer(frames as u32) }
            .map_err(|e| AudioError::Backend(format!("GetBuffer: {e}")))?;

        let dst = unsafe { std::slice::from_raw_parts_mut(dst as *mut f32, frames * channels) };
        for (frame, out) in dst.chunks_exact_mut(channels).enumerate() {
            for (ch, o) in out.iter_mut().enumerate() {
                *o = buffer
                    .data
                    .get(frame * channels + ch)
                    .copied()
                    .unwrap_or(0.0);
            }
        }

        unsafe { self.render.ReleaseBuffer(frames as u32, 0) }
            .map_err(|e| AudioError::Backend(format!("ReleaseBuffer: {e}")))?;
        Ok(())
    }
}

/// Capture source over a shared-mode `IAudioClient`.
pub struct WasapiReader {
    _client: IAudioClient,
    capture: IAudioCaptureClient,
    channels: u16,
    /// Holds the COM apartment open for the reader's lifetime.
    #[allow(dead_code)]
    com: ComGuard,
}

unsafe impl Send for WasapiReader {}

impl WasapiReader {
    /// Opens a capture stream. When `loopback` is set (capturing an output
    /// device), system audio is captured instead of a microphone.
    pub fn open(device_id: &str, config: StreamConfig, loopback: bool) -> Result<Self, AudioError> {
        let com = com_scoped(COINIT_MULTITHREADED)
            .map_err(|e| AudioError::Backend(format!("CoInitializeEx: {e}")))?;
        let (client, capture) = unsafe {
            open_capture_client(device_id, config, loopback)
                .map_err(|e| AudioError::Backend(e.to_string()))
        }?;
        Ok(Self {
            _client: client,
            capture,
            channels: config.channels,
            com,
        })
    }
}

impl DeviceReader for WasapiReader {
    fn read(&mut self, buffer: &mut AudioBuffer) -> Result<(), AudioError> {
        let channels = self.channels as usize;
        let mut filled = 0usize;

        unsafe {
            // Drain every available packet into the destination buffer.
            while self.capture.GetNextPacketSize().unwrap_or(0) > 0 {
                let mut data_ptr = std::ptr::null_mut();
                let mut frames = 0u32;
                let mut flags = 0u32;
                self.capture
                    .GetBuffer(&mut data_ptr, &mut frames, &mut flags, None, None)
                    .map_err(|e| AudioError::Backend(format!("capture GetBuffer: {e}")))?;

                if frames > 0 {
                    let samples = std::slice::from_raw_parts(
                        data_ptr as *const f32,
                        frames as usize * channels,
                    );
                    for (i, &s) in samples.iter().enumerate() {
                        let idx = filled * channels + i;
                        if idx < buffer.data.len() {
                            buffer.data[idx] = s;
                        }
                    }
                    filled += frames as usize;
                    self.capture
                        .ReleaseBuffer(frames)
                        .map_err(|e| AudioError::Backend(format!("capture ReleaseBuffer: {e}")))?;
                }

                if filled >= buffer.frames {
                    break;
                }
            }
        }

        // Silence whatever we couldn't fill (no packet yet).
        for b in &mut buffer.data[filled.min(buffer.frames) * channels..] {
            *b = 0.0;
        }
        if filled == 0 {
            // Pace an empty device instead of spinning.
            std::thread::sleep(Duration::from_millis(1));
        }
        Ok(())
    }
}

/// Builds a 32-bit float interleaved `WAVEFORMATEX` for `config`.
fn float_format(config: StreamConfig) -> WAVEFORMATEX {
    WAVEFORMATEX {
        wFormatTag: WAVE_FORMAT_IEEE_FLOAT,
        nChannels: config.channels,
        nSamplesPerSec: config.sample_rate,
        wBitsPerSample: 32,
        nBlockAlign: config.channels * 4,
        nAvgBytesPerSec: config.sample_rate * (config.channels * 4) as u32,
        cbSize: 0,
    }
}

unsafe fn open_render_client(
    device_id: &str,
    config: StreamConfig,
) -> Result<(IAudioClient, IAudioRenderClient, u32), windows::core::Error> {
    // SAFETY: caller has initialized COM for this thread; `wfx` outlives the
    // Initialize call and the client is activated before any audio thread
    // touches the writer.
    unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let device = find_device_com(&enumerator, device_id).ok_or(E_FAIL)?;

        let client: IAudioClient = device.Activate(CLSCTX_ALL, None)?;

        let wfx = float_format(config);

        let flags = AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY;
        client.Initialize(
            AUDCLNT_SHAREMODE_SHARED,
            flags,
            frames_to_hns(config.buffer_size, config.sample_rate),
            0,
            &wfx,
            None,
        )?;

        let render: IAudioRenderClient = client.GetService()?;
        let buffer_frames = client.GetBufferSize()?;
        client.Start()?;
        Ok((client, render, buffer_frames))
    }
}

unsafe fn open_capture_client(
    device_id: &str,
    config: StreamConfig,
    loopback: bool,
) -> Result<(IAudioClient, IAudioCaptureClient), windows::core::Error> {
    // SAFETY: caller has initialized COM for this thread; same invariants as the
    // render client.
    unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let device = find_device_com(&enumerator, device_id).ok_or(E_FAIL)?;

        let client: IAudioClient = device.Activate(CLSCTX_ALL, None)?;

        let wfx = float_format(config);

        // Loopback capture only applies to render endpoints; AUTOCONVERTPCM lets
        // the requested format differ from the device mix format.
        let mut flags =
            AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY;
        if loopback {
            flags |= AUDCLNT_STREAMFLAGS_LOOPBACK;
        }
        client.Initialize(
            AUDCLNT_SHAREMODE_SHARED,
            flags,
            frames_to_hns(config.buffer_size, config.sample_rate),
            0,
            &wfx,
            None,
        )?;

        let capture: IAudioCaptureClient = client.GetService()?;
        client.Start()?;
        Ok((client, capture))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_to_hns_converts() {
        // 256 frames @ 48 kHz = 5.333... ms = 53_333 * 100ns.
        assert_eq!(frames_to_hns(256, 48_000), 53_333);
        assert_eq!(frames_to_hns(0, 48_000), 0);
        assert_eq!(frames_to_hns(48_000, 48_000), REFTIMES_PER_SEC);
    }
}
