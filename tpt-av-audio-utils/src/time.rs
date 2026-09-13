//! Time and duration types expressed in sample frames, seconds, and
//! milliseconds, with sample-rate-aware conversions.

/// A position or duration measured in sample frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Frames(pub u64);

/// A position or duration measured in seconds.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default)]
pub struct Seconds(pub f64);

/// A position or duration measured in milliseconds.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default)]
pub struct Milliseconds(pub f64);

/// The engine's default sample rate (48 kHz).
pub const DEFAULT_SAMPLE_RATE: u32 = 48_000;

impl Frames {
    /// Converts to seconds at the given sample rate.
    #[inline]
    pub fn to_seconds(self, sample_rate: u32) -> Seconds {
        Seconds(self.0 as f64 / sample_rate as f64)
    }

    /// Converts to milliseconds at the given sample rate.
    #[inline]
    pub fn to_milliseconds(self, sample_rate: u32) -> Milliseconds {
        Milliseconds(self.0 as f64 * 1_000.0 / sample_rate as f64)
    }

    /// Wrapping-free addition saturating at `u64::MAX`.
    #[inline]
    pub fn saturating_add(self, other: Frames) -> Frames {
        Frames(self.0.saturating_add(other.0))
    }
}

impl Seconds {
    /// Converts to whole frames at the given sample rate, rounding to the
    /// nearest frame.
    #[inline]
    pub fn to_frames(self, sample_rate: u32) -> Frames {
        Frames((self.0 * sample_rate as f64).round().max(0.0) as u64)
    }

    /// Converts to milliseconds.
    #[inline]
    pub fn to_milliseconds(self) -> Milliseconds {
        Milliseconds(self.0 * 1_000.0)
    }
}

impl Milliseconds {
    /// Converts to seconds.
    #[inline]
    pub fn to_seconds(self) -> Seconds {
        Seconds(self.0 / 1_000.0)
    }

    /// Converts to whole frames at the given sample rate, rounding.
    #[inline]
    pub fn to_frames(self, sample_rate: u32) -> Frames {
        self.to_seconds().to_frames(sample_rate)
    }
}

/// Free-function form of [`Frames::to_seconds`].
#[inline]
pub fn frames_to_seconds(frames: u64, sample_rate: u32) -> f64 {
    frames as f64 / sample_rate as f64
}

/// Free-function form of [`Seconds::to_frames`], rounding to nearest.
#[inline]
pub fn seconds_to_frames(seconds: f64, sample_rate: u32) -> u64 {
    (seconds * sample_rate as f64).round().max(0.0) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_seconds_round_trip() {
        let frames = Frames(48_000);
        let secs = frames.to_seconds(48_000);
        assert!((secs.0 - 1.0).abs() < 1e-12);
        assert_eq!(secs.to_frames(48_000), frames);
    }

    #[test]
    fn rounding_is_nearest_not_truncating() {
        // 0.5 frame at 48 kHz should round to 1, not truncate to 0.
        assert_eq!(Seconds(0.5 / 48_000.0).to_frames(48_000), Frames(1));
        assert_eq!(Seconds(0.4 / 48_000.0).to_frames(48_000), Frames(0));
    }

    #[test]
    fn negative_seconds_clamp_to_zero() {
        assert_eq!(Seconds(-1.5).to_frames(44_100), Frames(0));
    }

    #[test]
    fn milliseconds_path() {
        let ms = Milliseconds(500.0);
        assert_eq!(ms.to_frames(48_000), Frames(24_000));
        assert_eq!(
            Frames(96_000).to_milliseconds(48_000),
            Milliseconds(2_000.0)
        );
        assert!((ms.to_seconds().0 - 0.5).abs() < 1e-12);
    }

    #[test]
    fn saturating_add_never_overflows() {
        assert_eq!(
            Frames(u64::MAX).saturating_add(Frames(10)),
            Frames(u64::MAX)
        );
        assert_eq!(Frames(1).saturating_add(Frames(2)), Frames(3));
    }

    #[test]
    fn free_functions_match_methods() {
        assert_eq!(
            frames_to_seconds(44_100, 44_100),
            Frames(44_100).to_seconds(44_100).0
        );
        assert_eq!(
            seconds_to_frames(2.0, 22_050),
            Seconds(2.0).to_frames(22_050).0
        );
    }
}
