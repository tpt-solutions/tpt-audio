//! Clips: non-destructive references into an audio asset.

use serde::{Deserialize, Serialize};

use crate::asset::AssetId;
use crate::envelope::Envelope;

/// Unique identifier for a [`Clip`] within a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ClipId(pub u64);

/// A non-destructive reference to a time range of an [`AudioAsset`].
///
/// A clip never copies or mutates audio: it names the asset, where it starts
/// in the source, where it sits on the timeline, and the transformations
/// (envelopes, fades) applied while it plays.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Clip {
    /// Unique clip identifier.
    pub id: ClipId,
    /// The source audio asset this clip plays.
    pub asset_id: AssetId,
    /// Start time of the clip on the timeline, in frames.
    pub start_frame: u64,
    /// Offset into the source asset, in frames (trim the head).
    pub source_offset: u64,
    /// Duration of the clip, in frames at the session sample rate.
    pub duration_frames: u64,
    /// Clip-level volume envelope (optional).
    pub volume_envelope: Option<Envelope>,
    /// Clip-level pan envelope (optional).
    pub pan_envelope: Option<Envelope>,
    /// Fade-in duration, in frames.
    pub fade_in_frames: u64,
    /// Fade-out duration, in frames.
    pub fade_out_frames: u64,
}

impl Clip {
    /// The first frame past the end of the clip on the timeline.
    pub fn end_frame(&self) -> u64 {
        self.start_frame.saturating_add(self.duration_frames)
    }

    /// Whether `frame` falls inside the clip's timeline range.
    pub fn contains_frame(&self, frame: u64) -> bool {
        frame >= self.start_frame && frame < self.end_frame()
    }

    /// Splits the clip at timeline frame `at`.
    ///
    /// Returns the left half (keeping this clip's id) and the right half
    /// (with `right_id`). Envelope points are re-based relative to each
    /// half's new start; the left half keeps the fade-in, the right half
    /// keeps the fade-out.
    ///
    /// Returns [`AudioError::InvalidEdit`] when `at` is not strictly inside
    /// the clip.
    pub fn split_at(
        &self,
        at: u64,
        right_id: ClipId,
    ) -> Result<(Clip, Clip), tpt_av_audio_utils::AudioError> {
        if at <= self.start_frame || at >= self.end_frame() {
            return Err(tpt_av_audio_utils::AudioError::InvalidEdit(format!(
                "split frame {at} is not inside clip range [{}, {})",
                self.start_frame,
                self.end_frame()
            )));
        }

        let left_duration = at - self.start_frame;
        let right_duration = self.duration_frames - left_duration;

        let mut left = self.clone();
        left.duration_frames = left_duration;

        let mut right = self.clone();
        right.id = right_id;
        right.start_frame = at;
        right.source_offset = self.source_offset + left_duration;
        right.duration_frames = right_duration;

        // Fades carry across the split: a fade-in longer than the left half
        // continues into the right half; a fade-out longer than the right
        // half started back in the left half.
        left.fade_in_frames = left.fade_in_frames.min(left_duration);
        left.fade_out_frames = self.fade_out_frames.saturating_sub(right_duration);
        right.fade_in_frames = self.fade_in_frames.saturating_sub(left_duration);
        right.fade_out_frames = right.fade_out_frames.min(right_duration);

        // Re-base envelope points onto each half's local timeline, inserting
        // a continuity point at the split so ramps don't jump.
        let right_end = right.end_frame();
        if let Some(env) = &mut left.volume_envelope {
            *env = rebase_env(env, self.start_frame, at, at, SplitSide::Left);
        }
        if let Some(env) = &mut left.pan_envelope {
            *env = rebase_env(env, self.start_frame, at, at, SplitSide::Left);
        }
        if let Some(env) = &mut right.volume_envelope {
            *env = rebase_env(env, at, right_end, at, SplitSide::Right);
        }
        if let Some(env) = &mut right.pan_envelope {
            *env = rebase_env(env, at, right_end, at, SplitSide::Right);
        }

        Ok((left, right))
    }
}

/// Which half of a split an envelope is being re-based onto.
enum SplitSide {
    Left,
    Right,
}

/// Keeps the points that fall in `[origin, end)`, re-times them relative to
/// `origin`, and adds a boundary point carrying the envelope's value at
/// `split_frame` so the two halves stay continuous.
fn rebase_env(
    env: &Envelope,
    origin: u64,
    end: u64,
    split_frame: u64,
    side: SplitSide,
) -> Envelope {
    let split_value = env.value_at(split_frame);
    let points = env
        .points()
        .iter()
        .filter(|p| p.frame >= origin && p.frame < end)
        .map(|p| crate::envelope::EnvelopePoint {
            frame: p.frame - origin,
            value: p.value,
        })
        .collect();
    let mut out = Envelope::with_points(points, env.interpolation());
    out.insert_point(crate::envelope::EnvelopePoint {
        frame: match side {
            SplitSide::Right => 0,
            SplitSide::Left => split_frame - origin,
        },
        value: split_value,
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip() -> Clip {
        Clip {
            id: ClipId(1),
            asset_id: AssetId(10),
            start_frame: 1_000,
            source_offset: 500,
            duration_frames: 2_000,
            volume_envelope: None,
            pan_envelope: None,
            fade_in_frames: 0,
            fade_out_frames: 0,
        }
    }

    #[test]
    fn end_and_contains() {
        let c = clip();
        assert_eq!(c.end_frame(), 3_000);
        assert!(!c.contains_frame(999));
        assert!(c.contains_frame(1_000));
        assert!(c.contains_frame(2_999));
        assert!(!c.contains_frame(3_000));
    }

    #[test]
    fn split_at_midpoint() {
        let (left, right) = clip().split_at(2_000, ClipId(2)).unwrap();

        assert_eq!(left.id, ClipId(1));
        assert_eq!(left.start_frame, 1_000);
        assert_eq!(left.duration_frames, 1_000);
        assert_eq!(left.source_offset, 500);

        assert_eq!(right.id, ClipId(2));
        assert_eq!(right.start_frame, 2_000);
        assert_eq!(right.duration_frames, 1_000);
        assert_eq!(right.source_offset, 1_500);
        assert_eq!(right.end_frame(), 3_000);
    }

    #[test]
    fn split_outside_clip_is_invalid() {
        let c = clip();
        assert!(c.split_at(1_000, ClipId(2)).is_err()); // at start
        assert!(c.split_at(3_000, ClipId(2)).is_err()); // at end
        assert!(c.split_at(500, ClipId(2)).is_err()); // before
    }

    #[test]
    fn split_keeps_fade_in_left_and_fade_out_right() {
        let mut c = clip();
        c.fade_in_frames = 200;
        c.fade_out_frames = 300;

        let (left, right) = c.split_at(1_500, ClipId(2)).unwrap();
        assert_eq!(left.fade_in_frames, 200);
        assert_eq!(left.fade_out_frames, 0);
        assert_eq!(right.fade_out_frames, 300);
        assert_eq!(right.fade_in_frames, 0);
    }

    #[test]
    fn split_rebases_envelopes_continuously() {
        let mut c = clip();
        c.volume_envelope = Some(Envelope::with_points(
            vec![
                crate::envelope::EnvelopePoint {
                    frame: 1_000,
                    value: 0.0,
                },
                crate::envelope::EnvelopePoint {
                    frame: 1_500,
                    value: 0.5,
                },
                crate::envelope::EnvelopePoint {
                    frame: 2_500,
                    value: 1.0,
                },
            ],
            crate::envelope::InterpolationMethod::Linear,
        ));

        let (left, right) = c.split_at(2_000, ClipId(2)).unwrap();

        // Left: original points at 1000/1500 rebased to 0/500, plus a
        // continuity point at the split carrying the interpolated value 0.75.
        let lv = left.volume_envelope.as_ref().unwrap();
        assert_eq!(lv.points().len(), 3);
        assert_eq!(lv.points()[0].frame, 0);
        assert_eq!(lv.points()[1].frame, 500);
        assert_eq!(lv.points()[2].frame, 1_000);
        assert!((lv.points()[2].value - 0.75).abs() < 1e-6);

        // Right: continuity point at frame 0, original point at 2500 rebased
        // to 500 — so the two halves join with no value jump.
        let rv = right.volume_envelope.as_ref().unwrap();
        assert_eq!(rv.points().len(), 2);
        assert_eq!(rv.points()[0].frame, 0);
        assert!((rv.points()[0].value - 0.75).abs() < 1e-6);
        assert_eq!(rv.points()[1].frame, 500);
        assert!((rv.value_at(0) - lv.value_at(1_000)).abs() < 1e-6);
    }

    #[test]
    fn split_is_lossless_in_duration() {
        let c = clip();
        let (left, right) = c.split_at(1_234, ClipId(9)).unwrap();
        assert_eq!(
            left.duration_frames + right.duration_frames,
            c.duration_frames
        );
        assert_eq!(left.end_frame(), right.start_frame);
    }
}
