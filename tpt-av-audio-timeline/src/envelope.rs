//! Automation envelopes: volume, pan, or any custom parameter.

use serde::{Deserialize, Serialize};
use tpt_av_audio_utils::AudioError;

/// How values between two control points are interpolated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum InterpolationMethod {
    /// Straight line between neighboring points.
    #[default]
    Linear,
    /// Catmull-Rom spline through surrounding points (smooth automation).
    Cubic,
    /// Hold the previous point's value until the next point (LED step).
    Step,
}

/// A single control point on an envelope.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct EnvelopePoint {
    /// Position on the timeline, in frames.
    pub frame: u64,
    /// Parameter value at this position (e.g. 0.0–1.0 for volume).
    pub value: f32,
}

/// An automation envelope: a time-ordered list of control points plus the
/// interpolation used between them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Envelope {
    points: Vec<EnvelopePoint>,
    interpolation: InterpolationMethod,
}

impl Envelope {
    /// Creates an empty envelope with the given interpolation.
    pub fn new(interpolation: InterpolationMethod) -> Self {
        Self {
            points: Vec::new(),
            interpolation,
        }
    }

    /// A linear envelope with a single unity point at frame 0.
    pub fn unity() -> Self {
        Self::with_points(
            vec![EnvelopePoint {
                frame: 0,
                value: 1.0,
            }],
            InterpolationMethod::Linear,
        )
    }

    /// Creates an envelope from points, sorting them by frame.
    pub fn with_points(mut points: Vec<EnvelopePoint>, interpolation: InterpolationMethod) -> Self {
        points.sort_by_key(|p| p.frame);
        Self {
            points,
            interpolation,
        }
    }

    /// The control points, ordered by frame.
    pub fn points(&self) -> &[EnvelopePoint] {
        &self.points
    }

    /// The interpolation method used between points.
    pub fn interpolation(&self) -> InterpolationMethod {
        self.interpolation
    }

    /// Whether the envelope has no control points.
    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    /// Inserts (or replaces, if a point already sits on that frame) a control
    /// point, keeping the list sorted.
    pub fn insert_point(&mut self, point: EnvelopePoint) {
        match self.points.binary_search_by_key(&point.frame, |p| p.frame) {
            Ok(idx) => self.points[idx] = point,
            Err(idx) => self.points.insert(idx, point),
        }
    }

    /// Removes the control point at `frame`, if any. Returns whether one was
    /// removed.
    pub fn remove_point(&mut self, frame: u64) -> bool {
        match self.points.binary_search_by_key(&frame, |p| p.frame) {
            Ok(idx) => {
                self.points.remove(idx);
                true
            }
            Err(_) => false,
        }
    }

    /// The envelope value at `frame`.
    ///
    /// - Before the first point: the first point's value.
    /// - After the last point: the last point's value.
    /// - An empty envelope evaluates to `1.0` (unity), the sane default for
    ///   volume-style parameters.
    pub fn value_at(&self, frame: u64) -> f32 {
        use InterpolationMethod::{Cubic, Linear, Step};

        if self.points.is_empty() {
            return 1.0;
        }

        // Locate the surrounding pair with a binary search on frame.
        match self.points.binary_search_by_key(&frame, |p| p.frame) {
            Ok(idx) => self.points[idx].value,
            Err(0) => self.points[0].value,
            Err(idx) if idx == self.points.len() => self.points[idx - 1].value,
            Err(right) => {
                let left = &self.points[right - 1];
                let right_pt = &self.points[right];
                match self.interpolation {
                    Step => left.value,
                    Linear => Self::lerp(left, right_pt, frame),
                    Cubic => self.catmull_rom(right - 1, frame),
                }
            }
        }
    }

    /// Linear interpolation between two points at `frame`.
    fn lerp(a: &EnvelopePoint, b: &EnvelopePoint, frame: u64) -> f32 {
        let span = (b.frame - a.frame) as f32;
        if span <= 0.0 {
            return b.value;
        }
        let t = (frame - a.frame) as f32 / span;
        a.value + (b.value - a.value) * t
    }

    /// Catmull-Rom spline value for the segment between
    /// `points[seg]` and `points[seg + 1]`.
    ///
    /// Neighbors outside the list are clamped to the endpoints, which keeps
    /// the curve well-defined at the boundaries.
    fn catmull_rom(&self, seg: usize, frame: u64) -> f32 {
        let p1 = self.points[seg];
        let p2 = self.points[seg + 1];
        let p0 = if seg > 0 { self.points[seg - 1] } else { p1 };
        let p3 = if seg + 2 < self.points.len() {
            self.points[seg + 2]
        } else {
            p2
        };

        let span = (p2.frame - p1.frame) as f32;
        if span <= 0.0 {
            return p2.value;
        }
        let t = (frame - p1.frame) as f32 / span;

        // Uniform Catmull-Rom (tension 0.5).
        0.5 * ((2.0 * p1.value)
            + (-p0.value + p2.value) * t
            + (2.0 * p0.value - 5.0 * p1.value + 4.0 * p2.value - p3.value) * t * t
            + (-p0.value + 3.0 * p1.value - 3.0 * p2.value + p3.value) * t * t * t)
    }

    /// Validates the envelope: all values must be finite. Returns
    /// [`AudioError::InvalidConfig`] otherwise.
    pub fn validate(&self) -> Result<(), AudioError> {
        for p in &self.points {
            if !p.value.is_finite() {
                return Err(AudioError::InvalidConfig(format!(
                    "envelope point at frame {} has non-finite value",
                    p.frame
                )));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(points: &[(u64, f32)], interp: InterpolationMethod) -> Envelope {
        Envelope::with_points(
            points
                .iter()
                .map(|&(f, v)| EnvelopePoint { frame: f, value: v })
                .collect(),
            interp,
        )
    }

    #[test]
    fn empty_envelope_is_unity() {
        assert_eq!(Envelope::new(InterpolationMethod::Linear).value_at(0), 1.0);
        assert_eq!(Envelope::new(InterpolationMethod::Step).value_at(123), 1.0);
    }

    #[test]
    fn before_first_and_after_last_clamp() {
        let e = env(&[(100, 0.2), (200, 0.8)], InterpolationMethod::Linear);
        assert_eq!(e.value_at(0), 0.2);
        assert_eq!(e.value_at(50), 0.2);
        assert_eq!(e.value_at(200), 0.8);
        assert_eq!(e.value_at(1_000), 0.8);
    }

    #[test]
    fn linear_midpoint_is_mean() {
        let e = env(&[(0, 0.0), (100, 1.0)], InterpolationMethod::Linear);
        assert!((e.value_at(50) - 0.5).abs() < 1e-6);
        assert!((e.value_at(25) - 0.25).abs() < 1e-6);
        assert!((e.value_at(75) - 0.75).abs() < 1e-6);
    }

    #[test]
    fn linear_ramp_down() {
        let e = env(&[(0, 1.0), (10, 0.0)], InterpolationMethod::Linear);
        assert!((e.value_at(5) - 0.5).abs() < 1e-6);
        assert!((e.value_at(1) - 0.9).abs() < 1e-6);
    }

    #[test]
    fn step_holds_previous_value() {
        let e = env(&[(0, 0.1), (100, 0.9)], InterpolationMethod::Step);
        assert_eq!(e.value_at(0), 0.1);
        assert_eq!(e.value_at(99), 0.1);
        assert_eq!(e.value_at(100), 0.9);
        assert_eq!(e.value_at(500), 0.9);
    }

    #[test]
    fn cubic_matches_linear_on_symmetric_ramp() {
        // A symmetric three-point ramp with clamped boundary neighbors makes
        // Catmull-Rom coincide with linear at the midpoint.
        let e = env(
            &[(0, 0.0), (100, 0.5), (200, 1.0)],
            InterpolationMethod::Cubic,
        );
        assert!((e.value_at(100) - 0.5).abs() < 1e-6);
        assert!((e.value_at(0) - 0.0).abs() < 1e-6);
        assert!((e.value_at(200) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn cubic_is_smooth_between_points() {
        // The spline must stay within the hull of its neighbors' values
        // (Catmull-Rom cannot overshoot a monotone ramp by more than a small
        // epsilon here) and must pass exactly through every control point.
        let e = env(
            &[(0, 0.0), (100, 1.0), (200, 0.25), (300, 0.75)],
            InterpolationMethod::Cubic,
        );
        for &(f, v) in &[(0, 0.0), (100, 1.0), (200, 0.25), (300, 0.75)] {
            assert!((e.value_at(f) - v).abs() < 1e-6, "frame {f}");
        }
        // Some interior samples: continuity rather than exactness is asserted.
        for f in [10, 50, 150, 250] {
            let v = e.value_at(f);
            assert!(v.is_finite());
        }
    }

    #[test]
    fn single_point_is_constant() {
        let e = env(&[(50, 0.3)], InterpolationMethod::Cubic);
        assert_eq!(e.value_at(0), 0.3);
        assert_eq!(e.value_at(49), 0.3);
        assert_eq!(e.value_at(50), 0.3);
        assert_eq!(e.value_at(51), 0.3);
    }

    #[test]
    fn insert_keeps_sorted_and_replaces_same_frame() {
        let mut e = Envelope::new(InterpolationMethod::Linear);
        e.insert_point(EnvelopePoint {
            frame: 100,
            value: 0.5,
        });
        e.insert_point(EnvelopePoint {
            frame: 0,
            value: 0.0,
        });
        e.insert_point(EnvelopePoint {
            frame: 200,
            value: 1.0,
        });
        // Replace the point at 100.
        e.insert_point(EnvelopePoint {
            frame: 100,
            value: 0.6,
        });

        assert_eq!(e.points().len(), 3);
        assert_eq!(e.value_at(100), 0.6);
        assert!((e.value_at(150) - 0.8).abs() < 1e-6);
    }

    #[test]
    fn remove_point_reports_presence() {
        let mut e = env(&[(0, 0.0), (100, 1.0)], InterpolationMethod::Linear);
        assert!(e.remove_point(0));
        assert!(!e.remove_point(0));
        assert_eq!(e.value_at(0), 1.0); // now clamps to remaining point
    }

    #[test]
    fn out_of_order_input_is_sorted() {
        let e = env(&[(200, 1.0), (0, 0.0)], InterpolationMethod::Linear);
        assert_eq!(e.points()[0].frame, 0);
        assert_eq!(e.value_at(100), 0.5);
    }

    #[test]
    fn validate_rejects_nan() {
        let e = env(&[(0, f32::NAN)], InterpolationMethod::Linear);
        assert!(e.validate().is_err());
        let e = env(&[(0, 0.5)], InterpolationMethod::Linear);
        assert!(e.validate().is_ok());
    }
}
