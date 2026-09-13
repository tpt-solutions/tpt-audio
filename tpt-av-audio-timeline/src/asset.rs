//! Audio assets: references to source files on disk.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Unique identifier for an [`AudioAsset`] within a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct AssetId(pub u64);

/// A reference to an audio file on disk.
///
/// Strictly non-destructive: the engine only ever reads from `file_path`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioAsset {
    /// Unique asset identifier.
    pub id: AssetId,
    /// Path to the source audio file (never written to).
    pub file_path: PathBuf,
    /// Duration of the asset, in frames at `sample_rate`.
    pub duration_frames: u64,
    /// Sample rate of the asset, in Hz.
    pub sample_rate: u32,
    /// Number of interleaved channels in the asset.
    pub channels: u16,
}

impl AudioAsset {
    /// The asset duration as [`tpt_av_audio_utils::Seconds`].
    pub fn duration_seconds(&self) -> f64 {
        if self.sample_rate == 0 {
            return 0.0;
        }
        self.duration_frames as f64 / self.sample_rate as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duration_seconds_handles_zero_rate() {
        let a = AudioAsset {
            id: AssetId(1),
            file_path: "x.wav".into(),
            duration_frames: 48_000,
            sample_rate: 0,
            channels: 2,
        };
        assert_eq!(a.duration_seconds(), 0.0);

        let a = AudioAsset {
            sample_rate: 48_000,
            ..a
        };
        assert!((a.duration_seconds() - 1.0).abs() < 1e-12);
    }

    #[test]
    fn serializes_to_json() {
        let a = AudioAsset {
            id: AssetId(7),
            file_path: "/audio/voice.wav".into(),
            duration_frames: 100,
            sample_rate: 44_100,
            channels: 1,
        };
        let json = serde_json::to_string(&a).unwrap();
        let back: AudioAsset = serde_json::from_str(&json).unwrap();
        assert_eq!(back, a);
        assert_eq!(back.id, AssetId(7));
    }
}
