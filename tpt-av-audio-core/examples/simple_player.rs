//! Simple player: decodes a WAV file, places it as a single clip on a
//! one-track session, and plays it through the platform audio backend.
//!
//! Usage:
//!
//! ```text
//! cargo run -p tpt-av-audio-core --example simple_player -- path/to/audio.wav
//! ```
//!
//! Set `TPT_AUDIO_BACKEND=null` to run without an audio device.

use std::sync::Arc;
use std::time::Duration;

use tpt_av_audio_core::decode::{decode_file, DecodeRegistry};
use tpt_av_audio_core::renderer::TimelineRenderer;
use tpt_av_audio_core::scheduler::TimelineState;
use tpt_av_audio_core::{AssetPcm, AssetStore};
use tpt_av_audio_io::device::{default_output_device, Direction};
use tpt_av_audio_io::stream::{OutputStream, StreamConfig};
use tpt_av_audio_io::AudioBackend;
use tpt_av_audio_timeline::{Clip, Session, TrackId};
use tpt_av_audio_utils::AudioBuffer;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "examples/podcast_demo_assets/voice.wav".into());

    // 1. Decode the file (Main Thread).
    let decoded = decode_file(&DecodeRegistry::with_builtins(), path.as_ref())?;
    let frames = (decoded.data.len() / decoded.channels.max(1) as usize) as u64;
    println!(
        "decoded {path}: {frames} frames @ {} Hz, {} ch",
        decoded.sample_rate, decoded.channels
    );

    // 2. Build a minimal non-destructive session around it.
    let rate = decoded.sample_rate;
    let mut session = Session::new("simple player", rate);
    let track = session.add_track("clip");
    let asset_id = session.register_asset(tpt_av_audio_timeline::AudioAsset {
        id: tpt_av_audio_timeline::AssetId(0),
        file_path: path.clone().into(),
        duration_frames: frames,
        sample_rate: rate,
        channels: decoded.channels,
    });
    let clip_id = session.generate_clip_id();
    session
        .track_mut(TrackId(track.0))
        .unwrap()
        .insert_clip(Clip {
            id: clip_id,
            asset_id,
            start_frame: 0,
            source_offset: 0,
            duration_frames: frames,
            volume_envelope: None,
            pan_envelope: None,
            fade_in_frames: 0,
            fade_out_frames: 0,
            loop_start: None,
            loop_end: None,
            fade_in_curve: Default::default(),
            fade_out_curve: Default::default(),
        });

    // 3. Cache the PCM and start the renderer.
    let store = Arc::new(AssetStore::new());
    store.insert(
        asset_id,
        AssetPcm {
            sample_rate: rate,
            channels: decoded.channels,
            data: decoded.data,
        },
    );
    let state = Arc::new(TimelineState::new(session));
    let mut renderer = TimelineRenderer::new(Arc::clone(&state), Arc::clone(&store));
    renderer.prepare(256, decoded.channels.max(1));

    // 4. Open the output stream.
    let device = match std::env::var("TPT_AUDIO_BACKEND").as_deref() {
        Ok("null") => tpt_av_audio_io::NullBackend::new()
            .enumerate_devices()?
            .into_iter()
            .find(|d| d.direction == Direction::Output)
            .expect("null output device"),
        _ => default_output_device()?,
    };
    let backend: Box<dyn tpt_av_audio_io::AudioBackend> = if device.id.0.starts_with("null") {
        Box::new(tpt_av_audio_io::NullBackend::new())
    } else {
        tpt_av_audio_io::backend::default_backend()?
    };
    let config = StreamConfig {
        sample_rate: rate,
        channels: decoded.channels.max(1),
        buffer_size: 256,
    };
    let writer = backend.open_output_writer(&device, config)?;

    let mut handle = OutputStream::start(
        &device,
        config,
        writer,
        Box::new(move |buf: &mut AudioBuffer| {
            let _ = renderer.render(buf);
        }),
    )?;

    println!("playing \"{}\" … (Ctrl-C to stop)", device.name);
    let duration = Duration::from_secs_f64(frames as f64 / rate as f64);
    std::thread::sleep(duration + Duration::from_millis(250));
    handle.stop();
    println!("done");
    Ok(())
}
