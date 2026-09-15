//! Generates two synthesized "tracks" (a bass pulse and a bright arpeggio)
//! on separate timeline tracks, then plays them through the audio backend.
//! Demonstrates: procedural assets, envelopes, fades, and live playback.
//!
//! Usage:
//!
//! ```text
//! cargo run -p tpt-av-audio-core --example mixer_demo
//! ```
//!
//! Set `TPT_AUDIO_BACKEND=null` to run without an audio device.

use std::sync::Arc;
use std::time::Duration;

use tpt_av_audio_core::renderer::TimelineRenderer;
use tpt_av_audio_core::scheduler::TimelineState;
use tpt_av_audio_core::{AssetPcm, AssetStore};
use tpt_av_audio_io::device::default_output_device;
use tpt_av_audio_io::stream::{OutputStream, StreamConfig};
use tpt_av_audio_io::AudioBackend;
use tpt_av_audio_timeline::{
    AssetId, Clip, ClipId, Envelope, EnvelopePoint, InterpolationMethod, Session,
};
use tpt_av_audio_utils::AudioBuffer;

const SAMPLE_RATE: u32 = 48_000;
const CLIP_FRAMES: u64 = SAMPLE_RATE as u64 * 4; // 4 s per track

fn synth(freq: f32, frames: u64, decay: bool) -> Vec<f32> {
    (0..frames)
        .flat_map(|i| {
            let t = i as f32 / SAMPLE_RATE as f32;
            let amp = if decay {
                (1.0 - (i as f32 / frames as f32)).max(0.0) * 0.6
            } else {
                0.4
            };
            let l = (2.0 * std::f32::consts::PI * freq * t).sin() * amp;
            let r = (2.0 * std::f32::consts::PI * (freq * 1.5) * t).sin() * amp;
            [l, r]
        })
        .collect()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut session = Session::new("mixer demo", SAMPLE_RATE);
    session.add_track("bass");
    session.add_track("arp");

    // Register synthetic assets and build clips referencing them.
    let bass_id = session.register_asset(tpt_av_audio_timeline::AudioAsset {
        id: AssetId(0),
        file_path: "synth://bass".into(),
        duration_frames: CLIP_FRAMES,
        sample_rate: SAMPLE_RATE,
        channels: 2,
    });
    let arp_id = session.register_asset(tpt_av_audio_timeline::AudioAsset {
        id: AssetId(0),
        file_path: "synth://arp".into(),
        duration_frames: CLIP_FRAMES,
        sample_rate: SAMPLE_RATE,
        channels: 2,
    });

    let clip_for =
        |id: tpt_av_audio_timeline::ClipId, asset: AssetId, start: u64, fade_out: u64| Clip {
            id,
            asset_id: asset,
            start_frame: start,
            source_offset: 0,
            duration_frames: CLIP_FRAMES,
            loop_start: None,
            loop_end: None,
            fade_in_curve: Default::default(),
            fade_out_curve: Default::default(),
            volume_envelope: Some(Envelope::with_points(
                vec![
                    EnvelopePoint {
                        frame: 0,
                        value: 0.0,
                    },
                    EnvelopePoint {
                        frame: CLIP_FRAMES / 4,
                        value: 1.0,
                    },
                    EnvelopePoint {
                        frame: CLIP_FRAMES,
                        value: 0.7,
                    },
                ],
                InterpolationMethod::Linear,
            )),
            pan_envelope: None,
            fade_in_frames: 4_800,
            fade_out_frames: fade_out,
        };

    session
        .track_mut(tpt_av_audio_timeline::TrackId(1))
        .unwrap()
        .insert_clip(clip_for(ClipId(1), bass_id, 0, 48_000));
    session
        .track_mut(tpt_av_audio_timeline::TrackId(2))
        .unwrap()
        .insert_clip(clip_for(ClipId(2), arp_id, SAMPLE_RATE as u64 / 2, 48_000));

    // Store the PCM.
    let store = Arc::new(AssetStore::new());
    store.insert(
        bass_id,
        AssetPcm {
            sample_rate: SAMPLE_RATE,
            channels: 2,
            data: synth(110.0, CLIP_FRAMES, true),
        },
    );
    store.insert(
        arp_id,
        AssetPcm {
            sample_rate: SAMPLE_RATE,
            channels: 2,
            data: synth(660.0, CLIP_FRAMES, false),
        },
    );

    let state = Arc::new(TimelineState::new(session));
    let mut renderer = TimelineRenderer::new(Arc::clone(&state), Arc::clone(&store));
    renderer.prepare(256, 2);

    // Pick an output device.
    let device = match std::env::var("TPT_AUDIO_BACKEND").as_deref() {
        Ok("null") => tpt_av_audio_io::NullBackend::new()
            .enumerate_devices()?
            .into_iter()
            .find(|d| d.direction == tpt_av_audio_io::device::Direction::Output)
            .expect("null backend output device"),
        _ => default_output_device()?,
    };

    let backend: Box<dyn tpt_av_audio_io::AudioBackend> = match device.id.0.as_str() {
        id if id.starts_with("null") => Box::new(tpt_av_audio_io::NullBackend::new()),
        _ => tpt_av_audio_io::backend::default_backend()?,
    };
    let config = StreamConfig {
        sample_rate: SAMPLE_RATE,
        channels: 2,
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

    println!("playing 5 s of the mix on \"{}\"…", device.name);
    std::thread::sleep(Duration::from_secs(5));
    handle.stop();
    println!("done");
    Ok(())
}
