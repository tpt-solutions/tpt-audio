//! Tracks: ordered collections of clips with strip-level state.

use serde::{Deserialize, Serialize};

use crate::clip::{Clip, ClipId};
use tpt_av_audio_utils::AudioError;

/// Unique identifier for a [`Track`] within a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TrackId(pub u64);

/// A single track containing multiple clips.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Track {
    /// Unique track identifier.
    pub id: TrackId,
    /// Track name (e.g. "Vocals", "Guitar").
    pub name: String,
    /// All clips on this track, kept sorted by start time.
    pub clips: Vec<Clip>,
    /// Track-level volume (0.0 = silent, 1.0 = unity gain).
    pub volume: f32,
    /// Track-level pan (-1.0 = left, 0.0 = center, 1.0 = right).
    pub pan: f32,
    /// Whether the track is muted.
    pub muted: bool,
    /// Whether the track is soloed.
    pub soloed: bool,
}

impl Track {
    /// Creates an empty track with unity gain, centered pan, no mute/solo.
    pub fn new(id: TrackId, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            clips: Vec::new(),
            volume: 1.0,
            pan: 0.0,
            muted: false,
            soloed: false,
        }
    }

    /// Inserts a clip, keeping the list sorted by start time. Replaces any
    /// existing clip with the same id.
    pub fn insert_clip(&mut self, clip: Clip) {
        match self.clips.iter().position(|c| c.id == clip.id) {
            Some(idx) => self.clips[idx] = clip,
            None => {
                let idx = self
                    .clips
                    .binary_search_by_key(&clip.start_frame, |c| c.start_frame)
                    .unwrap_or_else(|idx| idx);
                self.clips.insert(idx, clip);
            }
        }
    }

    /// Removes and returns the clip with `clip_id`.
    pub fn remove_clip(&mut self, clip_id: ClipId) -> Result<Clip, AudioError> {
        let idx = self
            .clips
            .iter()
            .position(|c| c.id == clip_id)
            .ok_or(AudioError::ClipNotFound(clip_id.0))?;
        Ok(self.clips.remove(idx))
    }

    /// Borrows the clip with `clip_id`, if present.
    pub fn clip(&self, clip_id: ClipId) -> Option<&Clip> {
        self.clips.iter().find(|c| c.id == clip_id)
    }

    /// Mutable borrow of the clip with `clip_id`, if present.
    pub fn clip_mut(&mut self, clip_id: ClipId) -> Option<&mut Clip> {
        self.clips.iter_mut().find(|c| c.id == clip_id)
    }

    /// All clips overlapping the half-open timeline range `[start, end)`.
    pub fn clips_in_range(&self, start: u64, end: u64) -> Vec<&Clip> {
        self.clips
            .iter()
            .filter(|c| c.start_frame < end && c.end_frame() > start)
            .collect()
    }

    /// The end of the last clip on the track (0 for an empty track).
    pub fn duration_frames(&self) -> u64 {
        self.clips.iter().map(|c| c.end_frame()).max().unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asset::AssetId;

    fn clip(id: u64, start: u64, dur: u64) -> Clip {
        Clip {
            id: ClipId(id),
            asset_id: AssetId(1),
            start_frame: start,
            source_offset: 0,
            duration_frames: dur,
            volume_envelope: None,
            pan_envelope: None,
            fade_in_frames: 0,
            fade_out_frames: 0,
        }
    }

    #[test]
    fn inserts_sorted() {
        let mut t = Track::new(TrackId(1), "Vocals");
        t.insert_clip(clip(1, 5_000, 100));
        t.insert_clip(clip(2, 1_000, 100));
        t.insert_clip(clip(3, 3_000, 100));

        let starts: Vec<u64> = t.clips.iter().map(|c| c.start_frame).collect();
        assert_eq!(starts, [1_000, 3_000, 5_000]);
    }

    #[test]
    fn replace_same_id_on_insert() {
        let mut t = Track::new(TrackId(1), "Vocals");
        t.insert_clip(clip(1, 1_000, 100));
        t.insert_clip(clip(1, 9_000, 500));

        assert_eq!(t.clips.len(), 1);
        assert_eq!(t.clips[0].start_frame, 9_000);
    }

    #[test]
    fn remove_missing_clip_is_clip_not_found() {
        let mut t = Track::new(TrackId(1), "Vocals");
        let e = t.remove_clip(ClipId(42)).unwrap_err();
        assert!(matches!(e, AudioError::ClipNotFound(42)));
    }

    #[test]
    fn clips_in_range_is_half_open() {
        let mut t = Track::new(TrackId(1), "Vocals");
        t.insert_clip(clip(1, 1_000, 500)); // [1000, 1500)
        t.insert_clip(clip(2, 2_000, 500)); // [2000, 2500)

        assert_eq!(t.clips_in_range(0, 999).len(), 0);
        assert_eq!(t.clips_in_range(0, 1_000).len(), 0); // half-open: clip starts AT 1000
        assert_eq!(t.clips_in_range(0, 1_001).len(), 1);
        assert_eq!(t.clips_in_range(1_000, 1_500).len(), 1);
        assert_eq!(t.clips_in_range(1_500, 2_000).len(), 0);
        assert_eq!(t.clips_in_range(1_500, 2_001).len(), 1);
        assert_eq!(t.clips_in_range(2_500, 9_999).len(), 0);
        assert_eq!(t.clips_in_range(0, 9_999).len(), 2);
    }

    #[test]
    fn duration_is_last_clip_end() {
        let mut t = Track::new(TrackId(1), "Vocals");
        assert_eq!(t.duration_frames(), 0);
        t.insert_clip(clip(1, 1_000, 500));
        t.insert_clip(clip(2, 4_000, 500));
        assert_eq!(t.duration_frames(), 4_500);
    }
}
