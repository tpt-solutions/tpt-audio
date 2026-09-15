//! Channel count conversion: downmix/upmix matrices.
//!
//! Mapping rules (documented contract, applied by
//! [`linear_resample_map_into`]):
//!
//! - equal counts — identity pass-through;
//! - mono source — every destination channel receives the mono sample;
//! - fold to mono (dst == 1) — constant-power sum: `Σ src / √src_count`;
//! - general upmix — destination channel *d* copies source channel
//!   `min(d, src-1)`;
//! - general downmix (dst ≥ 2) — destination channel *d* averages the
//!   proportional block of source channels
//!   `[d·src/dst, (d+1)·src/dst)`.
//!
//! All of it is plain arithmetic over preallocated slices — safe for the
//! real-time path.

/// The mapping plan for one clip render, resolved once before the frame
/// loop so the per-sample path is branch-light and allocation-free.
#[derive(Clone, Copy)]
enum ChannelMap {
    /// Identity: dst channel d copies source channel d.
    Identity,
    /// Mono source feeds every destination channel.
    FromMono,
    /// Constant-power fold to mono: `Σ src / √src_count`.
    FoldToMono { weight: f32 },
    /// General upmix: dst d copies source `min(d, src-1)`.
    Upmix,
    /// General downmix (dst ≥ 2): proportional block average.
    Downmix,
}

impl ChannelMap {
    fn resolve(src_ch: usize, dst_ch: usize) -> Self {
        if src_ch == dst_ch {
            Self::Identity
        } else if src_ch == 1 {
            Self::FromMono
        } else if dst_ch == 1 {
            Self::FoldToMono {
                weight: 1.0 / (src_ch as f32).sqrt(),
            }
        } else if src_ch < dst_ch {
            Self::Upmix
        } else {
            Self::Downmix
        }
    }
}

/// Resamples *and* channel-maps in one allocation-free pass: reads
/// `dst_frames` interleaved frames from `src` starting at fractional source
/// frame `src_start` (advancing `ratio` source frames per output frame),
/// maps source channels onto `dst_channels`, and writes into `dst` at
/// `dst_offset_frames`. Unwritten `dst` region is untouched; reads past the
/// source end produce silence.
///
/// # Real-Time Safety
///
/// No allocation, no locking, no panics — the mapping plan is resolved once
/// up front and everything below is slice arithmetic.
#[allow(clippy::too_many_arguments)]
pub fn linear_resample_map_into(
    src: &[f32],
    src_channels: u16,
    src_start: f64,
    ratio: f64,
    dst: &mut [f32],
    dst_channels: u16,
    dst_offset_frames: usize,
    dst_frames: usize,
) {
    let src_ch = src_channels.max(1) as usize;
    let dst_ch = dst_channels.max(1) as usize;
    let map = ChannelMap::resolve(src_ch, dst_ch);

    for i in 0..dst_frames {
        let src_pos = src_start + i as f64 * ratio;
        let frame_index = src_pos.floor();
        if frame_index < 0.0 {
            continue; // before source start: leave silence
        }
        let f0 = frame_index as usize;
        let t = (src_pos - frame_index) as f32;
        let dst_base = (dst_offset_frames + i) * dst_ch;

        // Lerped source sample for channel j (silence past the source end).
        #[inline(always)]
        fn lerped(src: &[f32], f0: usize, j: usize, src_ch: usize, t: f32) -> f32 {
            let a = *src.get(f0 * src_ch + j).unwrap_or(&0.0);
            let b = *src.get((f0 + 1) * src_ch + j).unwrap_or(&0.0);
            a + (b - a) * t
        }

        for d in 0..dst_ch {
            let value = match map {
                ChannelMap::Identity => lerped(src, f0, d, src_ch, t),
                ChannelMap::FromMono => lerped(src, f0, 0, src_ch, t),
                ChannelMap::FoldToMono { weight } => {
                    let mut acc = 0.0f32;
                    for j in 0..src_ch {
                        acc += lerped(src, f0, j, src_ch, t);
                    }
                    acc * weight
                }
                ChannelMap::Upmix => lerped(src, f0, d.min(src_ch - 1), src_ch, t),
                ChannelMap::Downmix => {
                    let start = (d * src_ch) / dst_ch;
                    let end = (((d + 1) * src_ch) / dst_ch).max(start + 1).min(src_ch);
                    let mut acc = 0.0f32;
                    for j in start..end {
                        acc += lerped(src, f0, j, src_ch, t);
                    }
                    acc / (end - start) as f32
                }
            };
            let idx = dst_base + d;
            if idx < dst.len() {
                dst[idx] = value;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(src: &[f32], src_ch: u16, dst_ch: u16, frames: usize) -> Vec<f32> {
        let mut dst = vec![0.0f32; frames * dst_ch as usize];
        linear_resample_map_into(src, src_ch, 0.0, 1.0, &mut dst, dst_ch, 0, frames);
        dst
    }

    #[test]
    fn identity_when_counts_match() {
        let out = map(&[0.1, 0.2, 0.3, 0.4], 2, 2, 2);
        assert_eq!(out, vec![0.1, 0.2, 0.3, 0.4]);
    }

    #[test]
    fn mono_upmix_feeds_every_channel() {
        let out = map(&[0.5, 0.25], 1, 2, 2);
        assert_eq!(out, vec![0.5, 0.5, 0.25, 0.25]);
    }

    #[test]
    fn stereo_folds_to_mono_constant_power() {
        let out = map(&[1.0, 1.0], 2, 1, 1);
        let expected = (1.0 + 1.0) / 2.0f32.sqrt();
        assert!((out[0] - expected).abs() < 1e-6);
        // A centered stereo signal keeps ~unity energy after fold.
        assert!((expected - 2.0f32.sqrt()).abs() < 1e-6);
    }

    #[test]
    fn five_point_one_folds_to_mono() {
        let src = [1.0f32; 6];
        let out = map(&src, 6, 1, 1);
        assert!((out[0] - 6.0 / 6.0f32.sqrt()).abs() < 1e-6);
    }

    #[test]
    fn stereo_downmix_of_opposite_phases_cancels() {
        let out = map(&[1.0, -1.0], 2, 1, 1);
        assert!(out[0].abs() < 1e-6);
    }

    #[test]
    fn upmix_copies_last_channel_when_exhausted() {
        // stereo → 3 channels: L, R, R
        let out = map(&[0.1, 0.2], 2, 3, 1);
        assert!((out[0] - 0.1).abs() < 1e-6);
        assert!((out[1] - 0.2).abs() < 1e-6);
        assert!((out[2] - 0.2).abs() < 1e-6);
    }

    #[test]
    fn proportional_block_downmix() {
        // 4 → 2: block {0,1} → ch0, block {2,3} → ch1 (averaged).
        let out = map(&[0.0, 1.0, 2.0, 3.0], 4, 2, 1);
        assert!((out[0] - 0.5).abs() < 1e-6);
        assert!((out[1] - 2.5).abs() < 1e-6);
    }

    #[test]
    fn offset_writes_only_requested_region() {
        let src = [1.0f32; 4];
        let mut dst = vec![9.0f32; 4 * 2];
        linear_resample_map_into(&src, 1, 0.0, 1.0, &mut dst, 2, 2, 2);
        // Frames 0..2 untouched (still 9.0), frames 2..4 = mono copy.
        assert_eq!(&dst[..4], &[9.0; 4]);
        assert_eq!(&dst[4..], &[1.0; 4]);
    }

    #[test]
    fn resamples_across_the_mapping() {
        // Mono 2-frame source [0, 1] at ratio 0.5 → 4 stereo frames:
        // positions 0, .5, 1, 1.5 → 0, .5, 1, .5 (toward zero padding).
        let mut dst = vec![0.0f32; 4 * 2];
        linear_resample_map_into(&[0.0f32, 1.0], 1, 0.0, 0.5, &mut dst, 2, 0, 4);
        let expected = [0.0f32, 0.0, 0.5, 0.5, 1.0, 1.0, 0.5, 0.5];
        for (got, want) in dst.iter().zip(expected) {
            assert!((got - want).abs() < 1e-6);
        }
    }
}

/// Source positioning for [`linear_resample_map_placed_into`]: where the
/// clip sits in the asset and whether playback loops.
#[derive(Clone, Copy, Debug, Default)]
pub struct SourcePlacement {
    /// Clip source offset, in source frames.
    pub source_offset_frames: u64,
    /// Clip-local frame of the first output frame to render.
    pub clip_local_start: u64,
    /// Loop region in clip-local frames; playback wraps from `1` back to
    /// `0` until the clip duration is exhausted.
    pub loop_region: Option<(u64, u64)>,
}

/// Like [`linear_resample_map_into`], but positions each output frame via
/// clip-local coordinates with optional loop wrapping — the shape the
/// timeline renderer needs (a loop makes source position piecewise-linear,
/// so a single start + ratio no longer describes it).
///
/// # Real-Time Safety
///
/// No allocation, no locking, no panics.
#[allow(clippy::too_many_arguments)]
pub fn linear_resample_map_placed_into(
    src: &[f32],
    src_channels: u16,
    placement: SourcePlacement,
    ratio: f64,
    dst: &mut [f32],
    dst_channels: u16,
    dst_offset_frames: usize,
    dst_frames: usize,
) {
    let src_ch = src_channels.max(1) as usize;
    let dst_ch = dst_channels.max(1) as usize;
    let map = ChannelMap::resolve(src_ch, dst_ch);
    let offset = placement.source_offset_frames as f64;
    let start = placement.clip_local_start;
    let (loop_start, loop_len) = match placement.loop_region {
        Some((ls, le)) if le > ls => (ls, le - ls),
        _ => (0, 0),
    };

    for i in 0..dst_frames {
        let logical = start + i as u64;
        let wrapped = if loop_len > 0 && logical >= loop_start + loop_len {
            loop_start + (logical - (loop_start + loop_len)) % loop_len
        } else {
            logical
        };
        let src_pos = offset + wrapped as f64 * ratio;
        let frame_index = src_pos.floor().max(0.0);
        let f0 = frame_index as usize;
        let t = (src_pos - frame_index) as f32;
        let dst_base = (dst_offset_frames + i) * dst_ch;

        #[inline(always)]
        fn lerped(src: &[f32], f0: usize, j: usize, src_ch: usize, t: f32) -> f32 {
            let a = *src.get(f0 * src_ch + j).unwrap_or(&0.0);
            let b = *src.get((f0 + 1) * src_ch + j).unwrap_or(&0.0);
            a + (b - a) * t
        }

        for d in 0..dst_ch {
            let value = match map {
                ChannelMap::Identity => lerped(src, f0, d, src_ch, t),
                ChannelMap::FromMono => lerped(src, f0, 0, src_ch, t),
                ChannelMap::FoldToMono { weight } => {
                    let mut acc = 0.0f32;
                    for j in 0..src_ch {
                        acc += lerped(src, f0, j, src_ch, t);
                    }
                    acc * weight
                }
                ChannelMap::Upmix => lerped(src, f0, d.min(src_ch - 1), src_ch, t),
                ChannelMap::Downmix => {
                    let block_start = (d * src_ch) / dst_ch;
                    let block_end = (((d + 1) * src_ch) / dst_ch)
                        .max(block_start + 1)
                        .min(src_ch);
                    let mut acc = 0.0f32;
                    for j in block_start..block_end {
                        acc += lerped(src, f0, j, src_ch, t);
                    }
                    acc / (block_end - block_start) as f32
                }
            };
            let idx = dst_base + d;
            if idx < dst.len() {
                dst[idx] = value;
            }
        }
    }
}

#[cfg(test)]
mod placed_tests {
    use super::*;

    #[test]
    fn placed_without_loop_matches_plain_positioning() {
        let src: Vec<f32> = (0..40).map(|i| i as f32).collect(); // mono ramp
        let mut a = vec![0.0f32; 8];
        linear_resample_map_into(&src, 1, 5.0, 1.0, &mut a, 1, 0, 8);

        let placement = SourcePlacement {
            source_offset_frames: 0,
            clip_local_start: 5,
            loop_region: None,
        };
        let mut b = vec![0.0f32; 8];
        linear_resample_map_placed_into(&src, 1, placement, 1.0, &mut b, 1, 0, 8);
        assert_eq!(a, b);
    }

    #[test]
    fn placed_loop_wraps_source_position() {
        // Mono ramp 0..24 as the asset; clip loops source frames [4, 8).
        let src: Vec<f32> = (0..24).map(|i| i as f32).collect();
        let placement = SourcePlacement {
            source_offset_frames: 0,
            clip_local_start: 0,
            loop_region: Some((4, 8)),
        };
        let mut dst = vec![0.0f32; 10];
        linear_resample_map_placed_into(&src, 1, placement, 1.0, &mut dst, 1, 0, 10);

        // Output reads logical 0..10; positions ≥ 8 wrap into [4, 8):
        // logical 8 → 4, logical 9 → 5.
        let expected = [0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 4.0, 5.0];
        for (got, want) in dst.iter().zip(expected) {
            assert!((got - want).abs() < 1e-6);
        }
    }

    #[test]
    fn placed_source_offset_shifts_reads() {
        let src: Vec<f32> = (0..24).map(|i| i as f32).collect();
        let placement = SourcePlacement {
            source_offset_frames: 10,
            clip_local_start: 0,
            loop_region: None,
        };
        let mut dst = vec![0.0f32; 3];
        linear_resample_map_placed_into(&src, 1, placement, 1.0, &mut dst, 1, 0, 3);
        assert_eq!(dst, vec![10.0, 11.0, 12.0]);
    }

    #[test]
    fn placed_empty_loop_region_is_no_loop() {
        let src: Vec<f32> = (0..24).map(|i| i as f32).collect();
        let placement = SourcePlacement {
            source_offset_frames: 0,
            clip_local_start: 6,
            loop_region: Some((5, 5)), // degenerate
        };
        let mut dst = vec![0.0f32; 3];
        linear_resample_map_placed_into(&src, 1, placement, 1.0, &mut dst, 1, 0, 3);
        assert_eq!(dst, vec![6.0, 7.0, 8.0]);
    }
}
