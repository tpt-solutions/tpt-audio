//! Live playback: decode a file and hear it through the OS audio backend.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use tpt_av_audio_core::{AssetPcm, AssetStore, DecodeRegistry, TimelineRenderer, TimelineState};
use tpt_av_audio_io::device::default_output_device;
use tpt_av_audio_io::{AudioBackend, NullBackend, OutputStream, StreamConfig, StreamHandle};
use tpt_av_audio_timeline::{Clip, Session};
use tpt_av_audio_utils::{AudioBuffer, AudioError};

/// A playing stream; drop or [`Playback::stop`] to end it.
pub struct Playback {
    handle: StreamHandle,
    device_name: String,
    duration: Duration,
}

impl Playback {
    /// Stops playback and joins the driver thread.
    pub fn stop(&mut self) {
        self.handle.stop();
    }

    /// Whether the stream driver is still running.
    pub fn is_playing(&self) -> bool {
        self.handle.is_running()
    }

    /// The output device name playback was opened on.
    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    /// Total duration of the playing file.
    pub fn duration(&self) -> Duration {
        self.duration
    }
}

/// Decodes `path` and plays it through the default OS output device.
///
/// Set `TPT_AUDIO_BACKEND=null` to run on the device-free null sink (CI,
/// headless boxes). Non-blocking: use the returned [`Playback`] to stop,
/// or [`play_file_blocking`] to block until the file finishes.
pub fn play_file(path: impl AsRef<Path>) -> Result<Playback, AudioError> {
    let path = path.as_ref();

    // 1. Decode (Main Thread).
    let registry = DecodeRegistry::with_builtins();
    let decoded = tpt_av_audio_core::decode::decode_file(&registry, path)?;
    let rate = decoded.sample_rate;
    let channels = decoded.channels.max(1);
    let frames = decoded.frames();

    // 2. Build a minimal non-destructive session around the file.
    let mut session = Session::new(
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("playback"),
        rate,
    );
    let track = session.add_track("clip");
    let asset_id = session.register_asset(tpt_av_audio_timeline::AudioAsset {
        id: tpt_av_audio_timeline::AssetId(0),
        file_path: path.to_path_buf(),
        duration_frames: frames,
        sample_rate: rate,
        channels,
    });
    let clip_id = session.generate_clip_id();
    session
        .track_mut(track)
        .unwrap()
        .insert_clip(Clip::new(clip_id, asset_id, 0, frames));

    // 3. Cache PCM + renderer.
    let store = Arc::new(AssetStore::new());
    store.insert(
        asset_id,
        AssetPcm {
            sample_rate: rate,
            channels,
            data: decoded.data,
        },
    );
    let state = Arc::new(TimelineState::new(session));
    let mut renderer = TimelineRenderer::new(Arc::clone(&state), store);
    renderer.prepare(256, channels);

    // 4. Output stream.
    let device = match std::env::var("TPT_AUDIO_BACKEND").as_deref() {
        Ok("null") => NullBackend::new()
            .enumerate_devices()?
            .into_iter()
            .find(|d| tpt_av_audio_io::Direction::Output == d.direction)
            .ok_or_else(|| AudioError::DeviceNotFound("null output".into()))?,
        _ => default_output_device()?,
    };
    let backend: Box<dyn AudioBackend> = if device.id.0.starts_with("null") {
        Box::new(NullBackend::new())
    } else {
        tpt_av_audio_io::backend::default_backend()?
    };
    let config = StreamConfig {
        sample_rate: rate,
        channels,
        buffer_size: 256,
    };
    let writer = backend.open_output_writer(&device, config)?;

    let handle = OutputStream::start(
        &device,
        config,
        writer,
        Box::new(move |buf: &mut AudioBuffer| {
            let _ = renderer.render(buf);
        }),
    )?;

    Ok(Playback {
        handle,
        device_name: device.name,
        duration: Duration::from_secs_f64(frames as f64 / rate as f64),
    })
}

/// Plays `path` to completion (blocks; Ctrl-C still interrupts the process).
pub fn play_file_blocking(path: impl AsRef<Path>) -> Result<(), AudioError> {
    let mut playback = play_file(path)?;
    let duration = playback.duration();
    std::thread::sleep(duration + Duration::from_millis(250));
    playback.stop();
    Ok(())
}
