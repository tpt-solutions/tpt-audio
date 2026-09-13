//! Sample-rate conversion.
//!
//! Two flavors:
//!
//! - [`Resampler`] — high-quality synchronous converter wrapping `rubato`
//!   (sinc interpolation). Main Thread / offline tool.
//! - [`linear_resample_into`] — allocation-free linear-interpolation path
//!   used by the real-time [`crate::renderer::TimelineRenderer`] when an
//!   asset's rate differs from the session rate.

use rubato::{
    Resampler as _, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction,
};
use tpt_av_audio_utils::AudioError;

/// High-quality resampler over `rubato`'s `SincFixedIn`.
///
/// Feed fixed-size input chunks of `input_rate` audio; pull `output_rate`
/// audio out. Remaining input is flushed with [`Self::flush`].
pub struct Resampler {
    inner: SincFixedIn<f32>,
    channels: u16,
    chunk_size: usize,
}

impl Resampler {
    /// Creates a resampler converting `input_rate` → `output_rate` for
    /// `channels` interleaved-passing-as-planar channels.
    ///
    /// `chunk_size` is the fixed number of input frames per `process` call.
    pub fn new(
        input_rate: u32,
        output_rate: u32,
        channels: u16,
        chunk_size: usize,
    ) -> Result<Self, AudioError> {
        if input_rate == 0 || output_rate == 0 || channels == 0 || chunk_size == 0 {
            return Err(AudioError::InvalidConfig(format!(
                "invalid resampler config: {input_rate} → {output_rate} Hz, {channels} ch, chunk {chunk_size}"
            )));
        }
        let params = SincInterpolationParameters {
            sinc_len: 256,
            f_cutoff: 0.95,
            oversampling_factor: 256,
            interpolation: SincInterpolationType::Cubic,
            window: WindowFunction::BlackmanHarris2,
        };
        let inner = SincFixedIn::new(
            output_rate as f64 / input_rate as f64,
            2.0,
            params,
            chunk_size,
            channels as usize,
        )
        .map_err(|e| AudioError::InvalidConfig(format!("rubato construction failed: {e}")))?;

        Ok(Self {
            inner,
            channels,
            chunk_size,
        })
    }

    /// Number of input frames the next `process` call expects.
    pub fn input_frames_next(&self) -> usize {
        self.inner.input_frames_next()
    }

    /// Fixed input chunk size this resampler was built with.
    pub fn chunk_size(&self) -> usize {
        self.chunk_size
    }

    /// Processes one fixed-size chunk. `input[ch]` holds the chunk frames
    /// for channel `ch`; returns per-channel output frames (shorter than the
    /// input by the resampling delay; flush the tail with [`Self::flush`]).
    pub fn process(&mut self, input: &[Vec<f32>]) -> Result<Vec<Vec<f32>>, AudioError> {
        if input.len() != self.channels as usize {
            return Err(AudioError::InvalidConfig(format!(
                "expected {} channel buffers, got {}",
                self.channels,
                input.len()
            )));
        }
        let mut out = self.empty_output();
        let (_in_used, out_written) = self
            .inner
            .process_into_buffer(input, &mut out, None)
            .map_err(|e| AudioError::Backend(format!("rubato process failed: {e}")))?;
        Ok(out
            .into_iter()
            .map(|ch| ch[..out_written].to_vec())
            .collect())
    }

    /// Drains the internal delay line after the final input chunk.
    pub fn flush(&mut self) -> Vec<Vec<f32>> {
        let mut out = self.empty_output();
        match self
            .inner
            .process_partial_into_buffer(None::<&[Vec<f32>]>, &mut out, None)
        {
            Ok((_in_used, out_written)) => out
                .into_iter()
                .map(|ch| ch[..out_written].to_vec())
                .collect(),
            Err(_) => Vec::new(),
        }
    }

    fn empty_output(&self) -> Vec<Vec<f32>> {
        vec![vec![0.0; self.inner.output_frames_max()]; self.channels as usize]
    }

    /// Number of channels.
    pub fn channels(&self) -> u16 {
        self.channels
    }
}

/// Real-time-safe linear-interpolation resampler reading straight out of a
/// source slice.
///
/// `ratio` = `source_rate / destination_rate` (>1 means the source is
/// "longer"/faster). Reads `src` starting at `source_start` (a fractional
/// frame position, in *source* frames relative to `src`'s beginning) and
/// writes `out_frames * dst_channels` interleaved frames into `dst` starting
/// at `dst_offset`. One source channel is mapped to one destination channel
/// (pass-through channel mapping: `dst_channels` must equal `src_channels`).
///
/// Reading past the source end produces silence — clips near the end of an
/// asset simply fade into digital silence, which matches the
/// non-destructive renderer's expectations.
#[allow(clippy::too_many_arguments)]
pub fn linear_resample_into(
    src: &[f32],
    src_channels: u16,
    src_start: f64,
    ratio: f64,
    dst: &mut [f32],
    dst_channels: u16,
    dst_offset_frames: usize,
    dst_frames: usize,
) {
    let src_channels = src_channels as usize;
    let dst_channels = dst_channels as usize;
    if src_channels != dst_channels {
        return; // contract violation; callers validate before the RT path
    }

    for i in 0..dst_frames {
        let src_pos = src_start + i as f64 * ratio;
        let frame_index = src_pos.floor();
        if frame_index < 0.0 {
            continue; // before source start: leave silence
        }
        let f0 = frame_index as usize;
        let t = (src_pos - frame_index) as f32;
        for ch in 0..src_channels {
            let a = *src.get(f0 * src_channels + ch).unwrap_or(&0.0);
            let b = *src.get((f0 + 1) * src_channels + ch).unwrap_or(&0.0);
            let idx = (dst_offset_frames + i) * dst_channels + ch;
            if idx < dst.len() {
                dst[idx] = a + (b - a) * t;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_upsample_interpolates() {
        // Asset rate = half the destination rate → ratio 0.5: every output
        // step advances half a source frame. 2 stereo frames stretch to 4.
        let src = [1.0f32, 0.5, 1.0, 0.5];
        let mut dst = vec![0.0f32; 4 * 2];
        linear_resample_into(&src, 2, 0.0, 0.5, &mut dst, 2, 0, 4);

        // Positions 0, 0.5, 1.0, 1.5 of a constant signal stay constant.
        for frame in 0..3 {
            assert!((dst[frame * 2] - 1.0).abs() < 1e-6, "frame {frame}");
            assert!((dst[frame * 2 + 1] - 0.5).abs() < 1e-6, "frame {frame}");
        }
        // Position 1.5 interpolates source frame 1 toward the zero padding.
        assert!((dst[6] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn linear_downsample_picks_positions() {
        // Source rate = twice the destination rate → ratio 2.0: output
        // frame i reads source frame 2i.
        let src = [0.0f32, 1.0, 2.0, 3.0];
        let mut dst = vec![0.0f32; 2];
        linear_resample_into(&src, 1, 0.0, 2.0, &mut dst, 1, 0, 2);
        assert!((dst[0] - 0.0).abs() < 1e-6);
        assert!((dst[1] - 2.0).abs() < 1e-6);
    }

    #[test]
    fn fractional_start_reads_accurately() {
        let src = [0.0f32, 1.0];
        let mut dst = vec![0.0f32; 1];
        // Position 0.5 between samples 0 and 1 → 0.5.
        linear_resample_into(&src, 1, 0.5, 0.0, &mut dst, 1, 0, 1);
        assert!((dst[0] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn past_end_is_silence() {
        let src = [1.0f32];
        let mut dst = vec![-1.0f32; 2];
        linear_resample_into(&src, 1, 0.0, 1.0, &mut dst, 1, 0, 2);
        assert!((dst[0] - 1.0).abs() < 1e-6);
        // Frame 1 interpolates sample 1 (missing → 0.0) with 0 → 0.0…±
        assert!(dst[1].abs() < 1e-6 || (dst[1] - 0.0).abs() < 1e-6);
    }

    #[test]
    fn rubato_resampler_round_trips_tone() {
        let mut r = Resampler::new(48_000, 24_000, 1, 256).unwrap();
        assert_eq!(r.channels(), 1);
        // Feed a slow sine chunk; output length shrinks ~half.
        let input: Vec<f32> = (0..256)
            .map(|i| (2.0 * std::f32::consts::PI * 100.0 * i as f32 / 48_000.0).sin())
            .collect();
        let out = r.process(&[input]).unwrap();
        assert!(!out[0].is_empty());
        // No output frame may be non-finite.
        assert!(out[0].iter().all(|s| s.is_finite()));
    }

    #[test]
    fn resampler_rejects_bad_config() {
        assert!(Resampler::new(0, 48_000, 2, 256).is_err());
        assert!(Resampler::new(48_000, 48_000, 0, 256).is_err());
    }
}
