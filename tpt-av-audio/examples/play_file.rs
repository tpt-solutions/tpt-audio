//! The smallest possible playback program with the umbrella crate.
//!
//! ```text
//! cargo run  -p tpt-av-audio --example play_file -- path/to/audio.wav
//! cargo run --features cadence --example play_file -- song.flac
//! ```
//!
//! Set `TPT_AUDIO_BACKEND=null` for device-free playback.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).unwrap_or_else(|| "demo.wav".into());
    tpt_av_audio::play_file_blocking(path)?;
    Ok(())
}
