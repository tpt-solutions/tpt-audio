//! Headless renderer: reads a timeline JSON, decodes its assets, and
//! renders the mix to a WAV file — no UI, no live audio stream.
//!
//! Usage:
//!
//! ```text
//! cargo run -p tpt-av-audio-core --example headless_render -- timeline.json output.wav
//! ```
//!
//! The timeline JSON is a serialized `tpt_av_audio_timeline::Session`; see
//! `examples/podcast_demo.json`. Asset `file_path`s resolve relative to the
//! JSON file's directory.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tpt_av_audio_core::decode::DecodeRegistry;
use tpt_av_audio_core::renderer::TimelineRenderer;
use tpt_av_audio_core::scheduler::TimelineState;
use tpt_av_audio_core::{AssetPcm, AssetStore};
use tpt_av_audio_timeline::{AssetId, Session};
use tpt_av_audio_utils::wav::{SampleFormat, WavSpec, WavWriter};
use tpt_av_audio_utils::AudioBuffer;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let timeline_path = args
        .next()
        .unwrap_or_else(|| "examples/podcast_demo.json".into());
    let output_path = args.next().unwrap_or_else(|| "output.wav".into());

    let timeline_path = PathBuf::from(&timeline_path);
    let base_dir = timeline_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();

    let json = std::fs::read_to_string(&timeline_path)?;
    let session: Session = serde_json::from_str(&json)?;
    println!(
        "loaded session \"{}\" ({} Hz, {} tracks)",
        session.name,
        session.sample_rate,
        session.tracks.len()
    );

    // Decode every asset up front (Main Thread work).
    let store = Arc::new(AssetStore::new());
    let registry = Arc::new(DecodeRegistry::with_builtins());
    let assets: Vec<(AssetId, PathBuf)> = session
        .assets
        .iter()
        .map(|a| (a.id, base_dir.join(&a.file_path)))
        .collect();
    for (id, path) in assets {
        let decoded = tpt_av_audio_core::decode::decode_file(&registry, &path)?;
        println!(
            "decoded asset {}: {} frames @ {} Hz, {} ch",
            id.0,
            decoded.data.len() / decoded.channels.max(1) as usize,
            decoded.sample_rate,
            decoded.channels
        );
        store.insert(
            id,
            AssetPcm {
                sample_rate: decoded.sample_rate,
                channels: decoded.channels,
                data: decoded.data,
            },
        );
    }

    let state = Arc::new(TimelineState::new(session.clone()));
    let mut renderer = TimelineRenderer::new(Arc::clone(&state), Arc::clone(&store));
    renderer.prepare(1024, 2);

    // Offline render loop.
    let rate = session.sample_rate;
    let channels = 2;
    let spec = WavSpec {
        channels,
        sample_rate: rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut writer = WavWriter::create(&output_path, spec)?;

    let total_frames = session.duration_frames();
    let mut buffer = AudioBuffer::new(1024, channels);
    let mut rendered = 0u64;
    while rendered < total_frames {
        renderer.render(&mut buffer)?;
        for frame in 0..buffer.frames {
            for ch in 0..channels as usize {
                let sample = buffer.data[frame * channels as usize + ch];
                let s = (sample.clamp(-1.0, 1.0) * 32_767.0) as i16;
                writer.write_sample(s)?;
            }
        }
        rendered += buffer.frames as u64;
    }
    writer.finalize()?;

    println!(
        "wrote {} ({} frames, {:.2}s)",
        output_path,
        total_frames,
        total_frames as f64 / rate as f64
    );
    Ok(())
}
