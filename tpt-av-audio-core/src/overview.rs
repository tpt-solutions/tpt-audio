//! Waveform overviews: min/max peak buckets for editor rendering.
//!
//! Decoders hand back full PCM; drawing a waveform with one line per
//! sample is hopeless at zoom-out. [`WaveformOverview`] reduces an asset
//! to fixed-rate min/max buckets (computed once on a worker thread), which
//! a UI can then sample directly or rescale via [`WaveformOverview::min_max_window`].

use crate::asset::AssetPcm;

/// Peak bucket summary of one asset's PCM.
#[derive(Debug, Clone)]
pub struct WaveformOverview {
    /// Buckets per second of audio.
    pub buckets_per_second: u32,
    sample_rate: u32,
    channels: u16,
    /// Per-bucket minimum (across all channels).
    mins: Vec<f32>,
    /// Per-bucket maximum.
    maxs: Vec<f32>,
}

impl WaveformOverview {
    /// Builds an overview from interleaved PCM at `buckets_per_second`.
    ///
    /// `sample_rate == 0` (unknown yet, e.g. a queued decode) yields an
    /// empty overview.
    pub fn from_pcm(pcm: &AssetPcm, buckets_per_second: u32) -> Self {
        Self::from_interleaved(&pcm.data, pcm.channels, pcm.sample_rate, buckets_per_second)
    }

    /// Builds an overview from raw interleaved samples.
    pub fn from_interleaved(
        data: &[f32],
        channels: u16,
        sample_rate: u32,
        buckets_per_second: u32,
    ) -> Self {
        let ch = channels.max(1) as usize;
        let frames = data.len() / ch;
        let rate = sample_rate.max(1);
        let bps = buckets_per_second.max(1);

        // Frames per bucket may be fractional; use exact boundaries so the
        // bucket rate is accurate for any sample rate.
        let bucket_count =
            ((frames as u64 * bps as u64) / rate as u64).max(if frames > 0 { 1 } else { 0 });
        let mut mins = vec![f32::MAX; bucket_count as usize];
        let mut maxs = vec![f32::MIN; bucket_count as usize];

        for (frame, chunk) in data.chunks(ch).enumerate() {
            let bucket = ((frame as u64 * bps as u64) / rate as u64) as usize;
            if bucket >= bucket_count as usize {
                break;
            }
            for &s in chunk {
                mins[bucket] = mins[bucket].min(s);
                maxs[bucket] = maxs[bucket].max(s);
            }
        }

        // Empty buckets (possible at very high bucket rates) become silence.
        for b in 0..bucket_count as usize {
            if mins[b] == f32::MAX {
                mins[b] = 0.0;
                maxs[b] = 0.0;
            }
        }

        Self {
            buckets_per_second: bps,
            sample_rate,
            channels,
            mins,
            maxs,
        }
    }

    /// Number of buckets.
    pub fn bucket_count(&self) -> usize {
        self.mins.len()
    }

    /// Sample rate the overview was built from.
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Channel count of the source PCM.
    pub fn channels(&self) -> u16 {
        self.channels
    }

    /// The audio duration covered, in seconds.
    pub fn duration_seconds(&self) -> f64 {
        self.bucket_count() as f64 / self.buckets_per_second.max(1) as f64
    }

    /// Min/max of bucket `i` (index clamped to the valid range; empty
    /// overviews read silence).
    pub fn min_max(&self, bucket: usize) -> (f32, f32) {
        if self.mins.is_empty() {
            return (0.0, 0.0);
        }
        let i = bucket.min(self.mins.len() - 1);
        (self.mins[i], self.maxs[i])
    }

    /// Min/max spanning source frame range `[start, end)` — the zoom-to-
    /// selection primitive: give it the visible frame window and draw one
    /// line per returned extent.
    pub fn min_max_window(&self, start_frame: u64, end_frame: u64) -> (f32, f32) {
        if self.mins.is_empty() {
            return (0.0, 0.0);
        }
        let first = (start_frame * self.buckets_per_second as u64)
            .checked_div(self.sample_rate.max(1) as u64)
            .unwrap_or(0) as usize;
        let last = (end_frame * self.buckets_per_second as u64)
            .checked_div(self.sample_rate.max(1) as u64)
            .unwrap_or(0) as usize;
        let (first, last) = (first.min(self.mins.len() - 1), last.min(self.mins.len()));
        if first >= last {
            return self.min_max(first);
        }
        let mut min = f32::MAX;
        let mut max = f32::MIN;
        for i in first..last {
            min = min.min(self.mins[i]);
            max = max.max(self.maxs[i]);
        }
        (min, max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step_pcm() -> AssetPcm {
        // 1 s stereo: first half quiet, second half loud.
        let mut data = Vec::new();
        for i in 0..48_000 {
            let v = if i < 24_000 { 0.1 } else { 0.9 };
            data.extend_from_slice(&[v, -v]);
        }
        AssetPcm {
            sample_rate: 48_000,
            channels: 2,
            data,
        }
    }

    #[test]
    fn buckets_cover_duration() {
        let overview = WaveformOverview::from_pcm(&step_pcm(), 100);
        assert_eq!(overview.bucket_count(), 100);
        assert_eq!(overview.sample_rate(), 48_000);
    }

    #[test]
    fn quiet_then_loud_halves() {
        let overview = WaveformOverview::from_pcm(&step_pcm(), 100);

        let (min, max) = overview.min_max(10); // first half
        assert!((min - -0.1).abs() < 1e-6);
        assert!((max - 0.1).abs() < 1e-6);

        let (min, max) = overview.min_max(90); // second half
        assert!((min - -0.9).abs() < 1e-6);
        assert!((max - 0.9).abs() < 1e-6);
    }

    #[test]
    fn window_spans_buckets() {
        let overview = WaveformOverview::from_pcm(&step_pcm(), 100);
        // The whole file: quiet AND loud.
        let (min, max) = overview.min_max_window(0, 48_000);
        assert!((min - -0.9).abs() < 1e-6);
        assert!((max - 0.9).abs() < 1e-6);

        // Only the quiet half.
        let (min, max) = overview.min_max_window(0, 12_000);
        assert!((max - 0.1).abs() < 1e-6);
        assert!((min - -0.1).abs() < 1e-6);
    }

    #[test]
    fn empty_and_unknown_rate_are_silent() {
        let empty = AssetPcm {
            sample_rate: 0,
            channels: 2,
            data: vec![],
        };
        let overview = WaveformOverview::from_pcm(&empty, 100);
        assert_eq!(overview.bucket_count(), 0);
        assert_eq!(overview.min_max(5), (0.0, 0.0));
        assert_eq!(overview.min_max_window(0, 48_000), (0.0, 0.0));
    }

    #[test]
    fn high_bucket_rate_handles_short_files() {
        let short = AssetPcm {
            sample_rate: 48_000,
            channels: 1,
            data: vec![0.5; 10],
        };
        let overview = WaveformOverview::from_pcm(&short, 100);
        // 10 frames at 48 kHz = 0.0002 s → 0 full buckets, clamped to 1 so
        // the waveform is still drawable.
        assert_eq!(overview.bucket_count(), 1);
        assert_eq!(overview.min_max(0), (0.5, 0.5));
    }
}
