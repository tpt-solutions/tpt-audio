//! The top-level session: a collection of tracks plus global state.

use serde::{Deserialize, Serialize};

use crate::asset::{AssetId, AudioAsset};
use crate::clip::ClipId;
use crate::track::{Track, TrackId};
use tpt_av_audio_utils::AudioError;

/// Unique identifier for a [`Session`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(pub u64);

/// Global session metadata (tempo, time signature, free-form notes).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionMetadata {
    /// Human-readable project title, e.g. "My Podcast Episode 1".
    pub title: String,
    /// Tempo in beats per minute (informational; the engine is frame-based).
    pub tempo_bpm: f64,
    /// Time signature numerator (e.g. 4 for 4/4).
    pub time_signature_numerator: u32,
    /// Time signature denominator.
    pub time_signature_denominator: u32,
}

impl Default for SessionMetadata {
    fn default() -> Self {
        Self {
            title: String::new(),
            tempo_bpm: 120.0,
            time_signature_numerator: 4,
            time_signature_denominator: 4,
        }
    }
}

/// Monotonic id generators for tracks, clips, and assets. Kept on the session
/// so edits (e.g. split) can mint unique ids deterministically.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdGenerator {
    next_track: u64,
    next_clip: u64,
    next_asset: u64,
}

impl Default for IdGenerator {
    fn default() -> Self {
        Self {
            next_track: 1,
            next_clip: 1,
            next_asset: 1,
        }
    }
}

impl IdGenerator {
    /// Next track id.
    pub fn next_track_id(&mut self) -> TrackId {
        let id = TrackId(self.next_track);
        self.next_track += 1;
        id
    }

    /// Next clip id.
    pub fn next_clip_id(&mut self) -> ClipId {
        let id = ClipId(self.next_clip);
        self.next_clip += 1;
        id
    }

    /// Next asset id.
    pub fn next_asset_id(&mut self) -> AssetId {
        let id = AssetId(self.next_asset);
        self.next_asset += 1;
        id
    }

    /// Reserves ids (used when loading foreign documents that may collide).
    pub fn reserve_track_id(&mut self, id: TrackId) {
        self.next_track = self.next_track.max(id.0 + 1);
    }

    /// Reserves clip ids.
    pub fn reserve_clip_id(&mut self, id: ClipId) {
        self.next_clip = self.next_clip.max(id.0 + 1);
    }

    /// Reserves asset ids.
    pub fn reserve_asset_id(&mut self, id: AssetId) {
        self.next_asset = self.next_asset.max(id.0 + 1);
    }
}

/// A complete audio editing session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Session {
    /// Unique session identifier.
    pub id: SessionId,
    /// Session name (e.g. "My Podcast Episode 1").
    pub name: String,
    /// Sample rate for the entire session (e.g. 44100, 48000).
    pub sample_rate: u32,
    /// All tracks in the session.
    pub tracks: Vec<Track>,
    /// Global metadata (tempo, time signature, etc.).
    pub metadata: SessionMetadata,
    /// Id generators for new tracks/clips/assets.
    pub ids: IdGenerator,
    /// Registered assets for this session.
    pub assets: Vec<AudioAsset>,
}

impl Session {
    /// Creates an empty session at `sample_rate` named `name`.
    pub fn new(name: impl Into<String>, sample_rate: u32) -> Self {
        Self {
            id: SessionId(1),
            name: name.into(),
            sample_rate,
            tracks: Vec::new(),
            metadata: SessionMetadata::default(),
            ids: IdGenerator::default(),
            assets: Vec::new(),
        }
    }

    /// Adds a new empty track with a generated id and returns it.
    pub fn add_track(&mut self, name: impl Into<String>) -> TrackId {
        let id = self.ids.next_track_id();
        self.tracks.push(Track::new(id, name));
        id
    }

    /// Removes the track with `id`. Returns the removed track.
    pub fn remove_track(&mut self, id: TrackId) -> Result<Track, AudioError> {
        let idx = self
            .tracks
            .iter()
            .position(|t| t.id == id)
            .ok_or(AudioError::TrackNotFound(id.0))?;
        Ok(self.tracks.remove(idx))
    }

    /// Borrows a track by id.
    pub fn track(&self, id: TrackId) -> Option<&Track> {
        self.tracks.iter().find(|t| t.id == id)
    }

    /// Mutably borrows a track by id.
    pub fn track_mut(&mut self, id: TrackId) -> Option<&mut Track> {
        self.tracks.iter_mut().find(|t| t.id == id)
    }

    /// Registers an asset and assigns it the next free id.
    pub fn register_asset(&mut self, mut asset: AudioAsset) -> AssetId {
        if asset.id.0 == 0 {
            asset.id = self.ids.next_asset_id();
        } else {
            self.ids.reserve_asset_id(asset.id);
        }
        let id = asset.id;
        self.assets.push(asset);
        id
    }

    /// Borrows a registered asset by id.
    pub fn asset(&self, id: AssetId) -> Option<&AudioAsset> {
        self.assets.iter().find(|a| a.id == id)
    }

    /// Whether any track has solo enabled. When this is true, non-soloed
    /// tracks are silent.
    pub fn any_soloed(&self) -> bool {
        self.tracks.iter().any(|t| t.soloed)
    }

    /// The duration of the longest track, in frames.
    pub fn duration_frames(&self) -> u64 {
        self.tracks
            .iter()
            .map(|t| t.duration_frames())
            .max()
            .unwrap_or(0)
    }

    /// Mints a fresh clip id.
    pub fn generate_clip_id(&mut self) -> ClipId {
        self.ids.next_clip_id()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_and_remove_tracks() {
        let mut s = Session::new("Test", 48_000);
        let t1 = s.add_track("Vocals");
        let t2 = s.add_track("Guitar");
        assert_ne!(t1, t2);

        s.remove_track(t1).unwrap();
        assert!(s.track(t1).is_none());
        assert!(s.track(t2).is_some());
        assert!(matches!(
            s.remove_track(t1),
            Err(AudioError::TrackNotFound(_))
        ));
    }

    #[test]
    fn asset_registration_assigns_unique_ids() {
        let mut s = Session::new("Test", 48_000);
        let mk = || AudioAsset {
            id: AssetId(0),
            file_path: "a.wav".into(),
            duration_frames: 10,
            sample_rate: 48_000,
            channels: 2,
        };
        let a = s.register_asset(mk());
        let b = s.register_asset(mk());
        assert_ne!(a, b);
        assert!(s.asset(a).is_some());
    }

    #[test]
    fn explicit_asset_ids_are_reserved() {
        let mut s = Session::new("Test", 48_000);
        let asset = AudioAsset {
            id: AssetId(50),
            file_path: "b.wav".into(),
            duration_frames: 1,
            sample_rate: 48_000,
            channels: 1,
        };
        s.register_asset(asset);
        let next = s.ids.next_asset_id();
        assert!(next.0 > 50);
    }

    #[test]
    fn duration_and_solo_state() {
        let mut s = Session::new("Test", 48_000);
        let t1 = s.add_track("A");
        let t2 = s.add_track("B");
        assert!(!s.any_soloed());
        assert_eq!(s.duration_frames(), 0);

        s.track_mut(t1).unwrap().soloed = true;
        assert!(s.any_soloed());

        let clip = crate::clip::Clip {
            id: s.generate_clip_id(),
            asset_id: AssetId(1),
            start_frame: 100,
            source_offset: 0,
            duration_frames: 500,
            volume_envelope: None,
            pan_envelope: None,
            fade_in_frames: 0,
            fade_out_frames: 0,
        };
        s.track_mut(t2).unwrap().insert_clip(clip);
        assert_eq!(s.duration_frames(), 600);
    }

    #[test]
    fn session_json_round_trip() {
        let mut s = Session::new("Podcast", 44_100);
        let t = s.add_track("Host");
        let c = crate::clip::Clip {
            id: s.generate_clip_id(),
            asset_id: AssetId(1),
            start_frame: 0,
            source_offset: 0,
            duration_frames: 100,
            volume_envelope: Some(crate::envelope::Envelope::unity()),
            pan_envelope: None,
            fade_in_frames: 10,
            fade_out_frames: 20,
        };
        s.track_mut(t).unwrap().insert_clip(c);

        let json = serde_json::to_string_pretty(&s).unwrap();
        let back: Session = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
        assert_eq!(back.name, "Podcast");
        assert_eq!(back.sample_rate, 44_100);
    }
}
