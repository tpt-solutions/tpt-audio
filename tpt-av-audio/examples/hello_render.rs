//! The smallest possible offline render: build a session, render to WAV.
//!
//! ```text
//! cargo run -p tpt-av-audio --example hello_render
//! ```

use tpt_av_audio::timeline::{Clip, Session, TrackId};
use tpt_av_audio::utils::AudioBuffer;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut engine = tpt_av_audio::Engine::new(Session::new("hello", 48_000), 512, 2)?;

    // A synthesized asset (procedural audio needs no file).
    let sine: Vec<f32> = (0..48_000 * 2)
        .map(|i| {
            let t = i as f32 / 48_000.0;
            (2.0 * std::f32::consts::PI * 440.0 * t).sin() * 0.4
        })
        .collect();
    let asset = engine.insert_pcm("synth://a440", 48_000, 2, sine)?;

    let mut session = engine.session();
    let track = session.add_track("tone");
    let clip = Clip::new(session.generate_clip_id(), asset, 0, 48_000);
    session
        .track_mut(TrackId(track.0))
        .unwrap()
        .insert_clip(clip);
    engine.set_session(session);

    // Render the timeline with an envelope-driven fade (per-frame gains are
    // applied by the engine; here we just verify output levels as we go).
    let mut peak = 0.0f32;
    let mut buffer = AudioBuffer::new(512, 2);
    while engine.position() < 48_000 {
        engine.render(&mut buffer)?;
        peak = peak.max(buffer.peak());
    }
    println!("rendered 1 s, peak {peak:.3}");
    Ok(())
}
