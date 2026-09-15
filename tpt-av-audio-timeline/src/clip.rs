//! Clips: non-destructive references into an audio asset.

use serde::{Deserialize, Serialize};

use crate::asset::AssetId;
use crate::envelope::Envelope;

/// Shape of a clip fade ramp.
///
/// - [`FadeCurve::Linear`] — constant amplitude ramp; correct for
///   phase-coherent material and short utility fades.
/// - [`FadeCurve::EqualPower`] — sine/cosine ramp; the standard for
///   crossfades, keeping perceived loudness flat when the two sides are
///   uncorrelated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum FadeCurve {
    #[default]
    Linear,
    EqualPower,
}

impl FadeCurve {
    /// Gain at progress `t` into a fade-in (0.0 → 1.0).
    pub fn fade_in_gain(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            FadeCurve::Linear => t,
            FadeCurve::EqualPower => silence_floor((t * std::f32::consts::FRAC_PI_2).sin()),
        }
    }

    /// Gain at progress `t` into a fade-out (0.0 → 1.0); inverse of the
    /// fade-in shape.
    pub fn fade_out_gain(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            FadeCurve::Linear => 1.0 - t,
            FadeCurve::EqualPower => silence_floor((t * std::f32::consts::FRAC_PI_2).cos()),
        }
    }
}

/// Snaps float noise around zero (cos(π/2) = -4e-8 in f32) to true silence
/// so a completed fade-out is bit-exact digital silence.
fn silence_floor(gain: f32) -> f32 {
    if gain.abs() < 1e-6 {
        0.0
    } else {
        gain
    }
}

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
    /// Loop region start, in clip-local frames (`None` = no loop).
    #[serde(default)]
    pub loop_start: Option<u64>,
    /// Loop region end, in clip-local frames; playback wraps from here
    /// back to `loop_start` until the clip duration is exhausted.
    #[serde(default)]
    pub loop_end: Option<u64>,
    /// Shape of the fade-in ramp.
    #[serde(default)]
    pub fade_in_curve: FadeCurve,
    /// Shape of the fade-out ramp.
    #[serde(default)]
    pub fade_out_curve: FadeCurve,
}

impl Clip {
    /// Creates a clip covering `duration_frames` of `asset_id` starting at
    /// `start_frame` on the timeline, with no envelopes and no fades — the
    /// common case in one call.
    pub fn new(
        id: ClipId,
        asset_id: crate::asset::AssetId,
        start_frame: u64,
        duration_frames: u64,
    ) -> Self {
        Self {
            id,
            asset_id,
            start_frame,
            source_offset: 0,
            duration_frames,
            volume_envelope: None,
            pan_envelope: None,
            fade_in_frames: 0,
            fade_out_frames: 0,
            loop_start: None,
            loop_end: None,
            fade_in_curve: FadeCurve::Linear,
            fade_out_curve: FadeCurve::Linear,
        }
    }

    /// Sets a loop region in clip-local frames (builder style).
    ///
    /// The renderer wraps playback from `loop_end` back to `loop_start`
    /// until the clip duration is exhausted.
    #[must_use]
    pub fn with_loop(mut self, loop_start: u64, loop_end: u64) -> Self {
        self.loop_start = Some(loop_start);
        self.loop_end = Some(loop_end);
        self
    }

    /// Validates and sets the loop region. Requires
    /// `loop_start < loop_end <= duration_frames`.
    pub fn set_loop_region(
        &mut self,
        loop_start: u64,
        loop_end: u64,
    ) -> Result<(), tpt_av_audio_utils::AudioError> {
        if loop_start >= loop_end || loop_end > self.duration_frames {
            return Err(tpt_av_audio_utils::AudioError::InvalidEdit(format!(
                "invalid loop region [{loop_start}, {loop_end}) for clip of {} frames",
                self.duration_frames
            )));
        }
        self.loop_start = Some(loop_start);
        self.loop_end = Some(loop_end);
        Ok(())
    }

    /// Maps a clip-local playback position to its (possibly wrapped)
    /// clip-local source position.
    pub fn source_position(&self, clip_local_frame: u64) -> u64 {
        match (self.loop_start, self.loop_end) {
            (Some(ls), Some(le)) if le > ls && clip_local_frame >= le => {
                ls + (clip_local_frame - le) % (le - ls)
            }
            _ => clip_local_frame,
        }
    }

    /// Sets the clip id (builder style, for `Clip::new(..).with_fades(..)` chains).
    #[must_use]
    pub fn with_id(mut self, id: ClipId) -> Self {
        self.id = id;
        self
    }

    /// Sets the fade-in/fade-out durations (builder style).
    #[must_use]
    pub fn with_fades(mut self, fade_in: u64, fade_out: u64) -> Self {
        self.fade_in_frames = fade_in;
        self.fade_out_frames = fade_out;
        self
    }

    /// Sets the source offset into the asset, in frames (builder style).
    #[must_use]
    pub fn with_source_offset(mut self, offset: u64) -> Self {
        self.source_offset = offset;
        self
    }

    /// Attaches a volume envelope (builder style).
    #[must_use]
    pub fn with_volume_envelope(mut self, envelope: Envelope) -> Self {
        self.volume_envelope = Some(envelope);
        self
    }

    /// Attaches a pan envelope (builder style).
    #[must_use]
    pub fn with_pan_envelope(mut self, envelope: Envelope) -> Self {
        self.pan_envelope = Some(envelope);
        self
    }

    /// Sets both fade curves (builder style).
    #[must_use]
    pub fn with_fade_curves(mut self, fade_in: FadeCurve, fade_out: FadeCurve) -> Self {
        self.fade_in_curve = fade_in;
        self.fade_out_curve = fade_out;
        self
    }

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

        // The loop region survives only on the half that fully contains it
        // (a region crossing the split would double-play its head).
        let (left_ls, left_le) = match (self.loop_start, self.loop_end) {
            (Some(ls), Some(le)) if le <= left_duration => (Some(ls), Some(le)),
            _ => (None, None),
        };
        left.loop_start = left_ls;
        left.loop_end = left_le;
        let (right_ls, right_le) = match (self.loop_start, self.loop_end) {
            (Some(ls), Some(le)) if ls >= left_duration => {
                (Some(ls - left_duration), Some(le - left_duration))
            }
            _ => (None, None),
        };
        right.loop_start = right_ls;
        right.loop_end = right_le;

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
            loop_start: None,
            loop_end: None,
            fade_in_curve: FadeCurve::Linear,
            fade_out_curve: FadeCurve::Linear,
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
    fn builder_constructs_common_case() {
        let c = Clip::new(ClipId(5), AssetId(9), 1_000, 2_000)
            .with_fades(100, 200)
            .with_source_offset(50)
            .with_volume_envelope(Envelope::unity());

        assert_eq!(c.id, ClipId(5));
        assert_eq!(c.asset_id, AssetId(9));
        assert_eq!(c.start_frame, 1_000);
        assert_eq!(c.duration_frames, 2_000);
        assert_eq!(c.fade_in_frames, 100);
        assert_eq!(c.fade_out_frames, 200);
        assert_eq!(c.source_offset, 50);
        assert!(c.volume_envelope.is_some());
        assert!(c.pan_envelope.is_none());
    }

    #[test]
    fn loop_wraps_positions() {
        let c = clip().with_loop(20, 60);
        assert_eq!(c.source_position(0), 0);
        assert_eq!(c.source_position(19), 19);
        assert_eq!(c.source_position(59), 59); // last pre-wrap frame
        assert_eq!(c.source_position(60), 20); // wraps to loop start
        assert_eq!(c.source_position(70), 30); // 10 frames into the loop
        assert_eq!(c.source_position(140), 20); // wraps twice (140-60=80, %40)
                                                // No loop: identity.
        assert_eq!(clip().source_position(70), 70);
    }

    #[test]
    fn set_loop_region_validates() {
        let mut c = clip(); // 2000 frames
        c.set_loop_region(100, 500).unwrap();
        assert_eq!(c.loop_start, Some(100));

        assert!(c.set_loop_region(500, 500).is_err()); // empty
        assert!(c.set_loop_region(600, 500).is_err()); // backwards
        assert!(c.set_loop_region(1500, 2500).is_err()); // past clip end
    }

    #[test]
    fn split_keeps_loop_only_when_fully_contained() {
        // Loop regions are clip-local: this clip spans local [0, 2000).
        let mut c = clip();
        c.set_loop_region(100, 300).unwrap(); // wholly in the left half
        let (left, right) = c.split_at(2_000, ClipId(2)).unwrap();
        assert_eq!(left.loop_start, Some(100));
        assert_eq!(left.loop_end, Some(300));
        assert_eq!(right.loop_start, None);
        assert_eq!(right.loop_end, None);

        let mut c = clip();
        c.set_loop_region(1_200, 1_400).unwrap(); // wholly in the right half
        let (left, right) = c.split_at(2_000, ClipId(2)).unwrap();
        assert_eq!(left.loop_start, None);
        assert_eq!(right.loop_start, Some(200)); // 1200 - 1000 (split local)
        assert_eq!(right.loop_end, Some(400));

        // A region crossing the split is dropped on both sides.
        let mut c = clip();
        c.set_loop_region(900, 1_100).unwrap();
        let (left, right) = c.split_at(2_000, ClipId(2)).unwrap();
        assert_eq!(left.loop_start, None);
        assert_eq!(right.loop_start, None);
    }

    #[test]
    fn fade_curve_math() {
        // Linear is linear.
        assert!((FadeCurve::Linear.fade_in_gain(0.25) - 0.25).abs() < 1e-6);
        assert!((FadeCurve::Linear.fade_out_gain(0.25) - 0.75).abs() < 1e-6);
        // Equal-power midpoint is cos/sin(45°) ≈ 0.707, power-sums to 1.
        let mid_in = FadeCurve::EqualPower.fade_in_gain(0.5);
        let mid_out = FadeCurve::EqualPower.fade_out_gain(0.5);
        assert!((mid_in - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6);
        assert!((mid_out - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6);
        assert!((mid_in * mid_in + mid_out * mid_out - 1.0).abs() < 1e-6);
        // Endpoints are exact.
        assert_eq!(FadeCurve::EqualPower.fade_in_gain(0.0), 0.0);
        assert!((FadeCurve::EqualPower.fade_in_gain(1.0) - 1.0).abs() < 1e-6);
        assert!((FadeCurve::EqualPower.fade_out_gain(0.0) - 1.0).abs() < 1e-6);
        assert_eq!(FadeCurve::EqualPower.fade_out_gain(1.0), 0.0);
        // Out-of-range progress clamps.
        assert_eq!(FadeCurve::EqualPower.fade_in_gain(2.0), 1.0);
        assert_eq!(FadeCurve::Linear.fade_out_gain(-1.0), 1.0);
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
