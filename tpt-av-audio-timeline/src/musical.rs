//! Musical time: bars, beats, and grid snapping driven by the session
//! metadata (`tempo_bpm`, time signature).
//!
//! Tempo is defined per quarter note (the DAW convention); the *beat* unit
//! is the time-signature denominator, so 4/4 at 120 BPM has 24 000-frame
//! beats at 48 kHz while 6/8 at 120 BPM has eighth-note beats half that
//! length. All positions are session frames.

use crate::session::Session;

/// Musical divisions available for grid snapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GridDivision {
    /// Snap to whole bars.
    Bar,
    /// Snap to beats (the time-signature denominator unit).
    Beat,
    /// Half-beat (eighth notes in 4/4).
    Half,
    /// Quarter-beat (sixteenths in 4/4).
    Quarter,
}

impl Session {
    /// Frames in one beat (denominator unit), given the session tempo and
    /// sample rate. Falls back to 1 frame/beat when the tempo is unset.
    pub fn frames_per_beat(&self) -> f64 {
        if self.metadata.tempo_bpm <= 0.0 || self.sample_rate == 0 {
            return 1.0;
        }
        let quarter = 60.0 / self.metadata.tempo_bpm;
        let beat_units = 4.0 / self.metadata.time_signature_denominator.max(1) as f64;
        quarter * beat_units * self.sample_rate as f64
    }

    /// Frames in one bar (numerator × beat length).
    pub fn frames_per_bar(&self) -> f64 {
        self.frames_per_beat() * self.metadata.time_signature_numerator.max(1) as f64
    }

    /// The position of `frame` as a beat count from the session start
    /// (fractional; beat 0 is frame 0).
    pub fn beat_at_frame(&self, frame: u64) -> f64 {
        frame as f64 / self.frames_per_beat()
    }

    /// The frame of beat position `beat` (rounded to the nearest frame).
    pub fn frame_at_beat(&self, beat: f64) -> u64 {
        ((beat * self.frames_per_beat()).round().max(0.0)) as u64
    }

    /// The 1-based `(bar, beat)` of `frame`. Frame 0 is bar 1, beat 1.
    pub fn bar_and_beat(&self, frame: u64) -> (u32, u32) {
        let beat_len = self.frames_per_beat();
        let bar_len = beat_len * self.metadata.time_signature_numerator.max(1) as f64;
        if bar_len <= 0.0 {
            return (1, 1);
        }
        let bar = (frame as f64 / bar_len).floor() as u32 + 1;
        let within = frame as f64 - (bar as f64 - 1.0) * bar_len;
        let beat = (within / beat_len).floor() as u32 + 1;
        (bar, beat)
    }

    /// The frame of the 1-based `(bar, beat)` position.
    pub fn frame_at_bar_beat(&self, bar: u32, beat: u32) -> u64 {
        let beat_len = self.frames_per_beat();
        let bar_index = bar.saturating_sub(1) as f64;
        let beat_index = beat.saturating_sub(1) as f64;
        ((bar_index * self.metadata.time_signature_numerator.max(1) as f64 + beat_index) * beat_len)
            .round()
            .max(0.0) as u64
    }

    /// Snaps `frame` to the nearest `division` grid line.
    pub fn snap_to_grid(&self, frame: u64, division: GridDivision) -> u64 {
        let step = match division {
            GridDivision::Bar => self.frames_per_bar(),
            GridDivision::Beat => self.frames_per_beat(),
            GridDivision::Half => self.frames_per_beat() / 2.0,
            GridDivision::Quarter => self.frames_per_beat() / 4.0,
        };
        if step <= 0.0 {
            return frame;
        }
        (frame as f64 / step).round().max(0.0) as u64 * step.round().max(1.0) as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::SessionMetadata;

    fn session_4_4_120() -> Session {
        let mut s = Session::new("grid", 48_000);
        s.metadata = SessionMetadata {
            title: String::new(),
            tempo_bpm: 120.0,
            time_signature_numerator: 4,
            time_signature_denominator: 4,
        };
        s
    }

    #[test]
    fn beat_and_bar_lengths_at_120bpm_4_4() {
        let s = session_4_4_120();
        // 120 BPM → 0.5 s per quarter → 24 000 frames at 48 kHz.
        assert!((s.frames_per_beat() - 24_000.0).abs() < 1e-9);
        assert!((s.frames_per_bar() - 96_000.0).abs() < 1e-9);
    }

    #[test]
    fn compound_meter_shortens_beats() {
        let mut s = session_4_4_120();
        s.metadata.time_signature_numerator = 6;
        s.metadata.time_signature_denominator = 8;
        // The beat unit is an eighth = half a quarter.
        assert!((s.frames_per_beat() - 12_000.0).abs() < 1e-9);
        // A 6/8 bar is 6 eighths = 3 quarters = 72 000 frames.
        assert!((s.frames_per_bar() - 72_000.0).abs() < 1e-9);
    }

    #[test]
    fn bar_and_beat_round_trip() {
        let s = session_4_4_120();
        // Frame 0 = bar 1 beat 1.
        assert_eq!(s.bar_and_beat(0), (1, 1));
        // One bar in = bar 2 beat 1.
        assert_eq!(s.bar_and_beat(96_000), (2, 1));
        // Half a bar past that = beat 3 of bar 2.
        assert_eq!(s.bar_and_beat(96_000 + 48_000), (2, 3));

        assert_eq!(s.frame_at_bar_beat(1, 1), 0);
        assert_eq!(s.frame_at_bar_beat(2, 1), 96_000);
        assert_eq!(s.frame_at_bar_beat(2, 3), 96_000 + 48_000);
    }

    #[test]
    fn beat_frame_conversions_round_trip() {
        let s = session_4_4_120();
        for beat in [0.0, 1.5, 7.25, 33.0] {
            assert!((s.beat_at_frame(s.frame_at_beat(beat)) - beat).abs() < 1e-9);
        }
    }

    #[test]
    fn snapping_rounds_to_nearest_grid_line() {
        let s = session_4_4_120();
        // 30 000 is closer to the second beat (24 000) than the third
        // (48 000).
        assert_eq!(s.snap_to_grid(30_000, GridDivision::Beat), 24_000);
        // Bars: 100 000 → nearest bar line 96 000.
        assert_eq!(s.snap_to_grid(100_000, GridDivision::Bar), 96_000);
        // Sixteenths: 6 000 → nearest 6 000 (quarter-beat = 6 000 exactly).
        assert_eq!(s.snap_to_grid(6_000, GridDivision::Quarter), 6_000);
        assert_eq!(s.snap_to_grid(7_000, GridDivision::Quarter), 6_000);
    }

    #[test]
    fn unset_tempo_degrades_to_identity() {
        let mut s = Session::new("no tempo", 48_000);
        s.metadata.tempo_bpm = 0.0;
        assert_eq!(s.snap_to_grid(123_456, GridDivision::Bar), 123_456);
        assert_eq!(s.frames_per_beat(), 1.0);
    }
}
