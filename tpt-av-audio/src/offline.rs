//! Offline rendering: session → WAV file, no audio device needed.

use std::path::Path;
use std::sync::Arc;

use tpt_av_audio_core::decode::decode_file;
use tpt_av_audio_core::{AssetPcm, AssetStore, DecodeRegistry, TimelineRenderer, TimelineState};
use tpt_av_audio_timeline::Session;
use tpt_av_audio_utils::wav::{SampleFormat, WavSpec, WavWriter};
use tpt_av_audio_utils::{AudioBuffer, AudioError};

/// Output format for the offline renderer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WavExportFormat {
    /// 16-bit PCM (the default: universally compatible).
    #[default]
    Pcm16,
    /// 24-bit PCM.
    Pcm24,
    /// 32-bit IEEE float (full engine precision, no quantization).
    Float32,
}

impl WavExportFormat {
    fn spec_for(self, sample_rate: u32, channels: u16) -> WavSpec {
        let (bits_per_sample, sample_format) = match self {
            Self::Pcm16 => (16, SampleFormat::Int),
            Self::Pcm24 => (24, SampleFormat::Int),
            Self::Float32 => (32, SampleFormat::Float),
        };
        WavSpec {
            channels,
            sample_rate,
            bits_per_sample,
            sample_format,
        }
    }
}

/// Renders `session` (decoding every registered asset through the default
/// [`DecodeRegistry`]) and writes the mix to `output` as 16-bit PCM WAV.
///
/// Asset `file_path`s are resolved as-is (make them absolute or relative to
/// the process CWD; [`Session::save`]/[`Session::load`] round-trips them).
pub fn render_session_to_wav(
    session: &Session,
    output: impl AsRef<Path>,
) -> Result<(), AudioError> {
    render_session_to_wav_opts(session, output, WavExportFormat::default())
}

/// [`render_session_to_wav`] with an explicit output format.
pub fn render_session_to_wav_opts(
    session: &Session,
    output: impl AsRef<Path>,
    format: WavExportFormat,
) -> Result<(), AudioError> {
    let store = Arc::new(AssetStore::new());
    let registry = DecodeRegistry::with_builtins();
    for asset in &session.assets {
        let decoded = decode_file(&registry, &asset.file_path)?;
        store.insert(
            asset.id,
            AssetPcm {
                sample_rate: decoded.sample_rate,
                channels: decoded.channels,
                data: decoded.data,
            },
        );
    }
    render_with_store_opts(session, store, output, format)
}

/// Renders `session` using an already-populated [`AssetStore`] (synthetic
/// audio, preloaded caches) and writes the mix to `output` as 16-bit PCM.
pub fn render_with_store(
    session: &Session,
    store: Arc<AssetStore>,
    output: impl AsRef<Path>,
) -> Result<(), AudioError> {
    render_with_store_opts(session, store, output, WavExportFormat::default())
}

/// [`render_with_store`] with an explicit output format.
pub fn render_with_store_opts(
    session: &Session,
    store: Arc<AssetStore>,
    output: impl AsRef<Path>,
    format: WavExportFormat,
) -> Result<(), AudioError> {
    const CHANNELS: u16 = 2;
    const BUFFER_FRAMES: usize = 1_024;

    let state = Arc::new(TimelineState::new(session.clone()));
    let mut renderer = TimelineRenderer::new(Arc::clone(&state), store);
    renderer.prepare(BUFFER_FRAMES, CHANNELS);

    let mut writer = WavWriter::create(
        output.as_ref(),
        format.spec_for(session.sample_rate, CHANNELS),
    )?;

    let total = session.duration_frames();
    let mut buffer = AudioBuffer::new(BUFFER_FRAMES, CHANNELS);
    while renderer.position() < total {
        // Compute the tail BEFORE rendering: render() advances the playhead.
        // Trim so the file ends exactly at the session duration instead of
        // at a buffer boundary.
        let frames_to_write = ((total - renderer.position()).min(buffer.frames as u64)) as usize;
        renderer.render(&mut buffer)?;
        for frame in 0..frames_to_write {
            for ch in 0..CHANNELS as usize {
                let sample = buffer.data[frame * CHANNELS as usize + ch].clamp(-1.0, 1.0);
                match format {
                    WavExportFormat::Pcm16 => writer.write_sample((sample * 32_767.0) as i16)?,
                    WavExportFormat::Pcm24 => writer.write_sample((sample * 8_388_607.0) as i32)?,
                    WavExportFormat::Float32 => writer.write_sample(sample)?,
                }
            }
        }
    }
    writer.finalize()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tpt_av_audio_timeline::{Clip, TrackId};

    fn tone_session() -> (Session, AssetStore) {
        let mut session = Session::new("offline", 48_000);
        let asset_id = session.register_asset(tpt_av_audio_timeline::AudioAsset {
            id: tpt_av_audio_timeline::AssetId(0),
            file_path: "synth://tone".into(),
            duration_frames: 4_800,
            sample_rate: 48_000,
            channels: 2,
        });
        let track = session.add_track("tone");
        let clip_id = session.generate_clip_id();
        session
            .track_mut(TrackId(track.0))
            .unwrap()
            .insert_clip(Clip::new(clip_id, asset_id, 0, 4_800));

        let store = AssetStore::new();
        store.insert(
            asset_id,
            AssetPcm {
                sample_rate: 48_000,
                channels: 2,
                data: vec![0.5; 9_600],
            },
        );
        (session, store)
    }

    #[test]
    fn renders_synth_session_to_wav() {
        let (session, store) = tone_session();

        let dir = std::env::temp_dir().join("tpt-av-audio-offline-tests");
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("offline.wav");
        render_with_store(&session, Arc::new(store), &out).unwrap();

        // Peak matches the synthesized amplitude (0.5), quantized to i16.
        let mut reader = tpt_av_audio_utils::wav::WavReader::open(&out).unwrap();
        let peak: f32 = reader
            .samples::<i16>()
            .map(|s| s.unwrap().abs() as f32 / 32_768.0)
            .fold(0.0f32, f32::max);
        assert!((peak - 0.5).abs() < 1e-3);
        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn exports_24_bit_and_float() {
        let dir = std::env::temp_dir().join("tpt-av-audio-offline-tests");
        std::fs::create_dir_all(&dir).unwrap();

        for (format, expected_bits) in [
            (WavExportFormat::Pcm24, 24u16),
            (WavExportFormat::Float32, 32),
        ] {
            let (session, store) = tone_session();

            let out = dir.join(format!("{expected_bits}_export.wav"));
            render_with_store_opts(&session, Arc::new(store), &out, format).unwrap();

            let reader = tpt_av_audio_utils::wav::WavReader::open(&out).unwrap();
            assert_eq!(reader.spec().bits_per_sample, expected_bits);
            assert_eq!(reader.duration(), 4_800);
            let _ = std::fs::remove_file(&out);
        }
    }
}
