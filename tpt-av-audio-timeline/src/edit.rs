//! Edit operations: insert, delete, move, and split — each undoable.
//!
//! Every operation implements [`Edit`], carrying enough interior state to
//! `revert` itself. Operations are applied through a [`crate::history::History`]
//! so undo/redo bookkeeping is automatic.

use serde::{Deserialize, Serialize};

use crate::clip::{Clip, ClipId};
use crate::session::Session;
use crate::track::TrackId;
use tpt_av_audio_utils::AudioError;

/// An undoable operation on a [`Session`].
pub trait Edit: std::fmt::Debug + Send {
    /// Human-readable name for command palettes / logs.
    fn label(&self) -> &'static str;

    /// Applies the operation. Calling `apply` twice without an intervening
    /// `revert` is a logic error and may return [`AudioError::InvalidEdit`].
    fn apply(&mut self, session: &mut Session) -> Result<(), AudioError>;

    /// Reverts the most recent `apply`.
    fn revert(&mut self, session: &mut Session) -> Result<(), AudioError>;
}

/// Inserts a new clip onto a track.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InsertClipEdit {
    track_id: TrackId,
    clip: Clip,
}

impl InsertClipEdit {
    /// Prepares an insert of `clip` onto `track_id`.
    pub fn new(track_id: TrackId, clip: Clip) -> Self {
        Self { track_id, clip }
    }
}

impl Edit for InsertClipEdit {
    fn label(&self) -> &'static str {
        "insert clip"
    }

    fn apply(&mut self, session: &mut Session) -> Result<(), AudioError> {
        let track = session
            .track_mut(self.track_id)
            .ok_or(AudioError::TrackNotFound(self.track_id.0))?;
        track.insert_clip(self.clip.clone());
        Ok(())
    }

    fn revert(&mut self, session: &mut Session) -> Result<(), AudioError> {
        let track = session
            .track_mut(self.track_id)
            .ok_or(AudioError::TrackNotFound(self.track_id.0))?;
        track.remove_clip(self.clip.id)?;
        Ok(())
    }
}

/// Removes a clip from a track, capturing it so the removal can be undone.
#[derive(Debug, Serialize, Deserialize)]
pub struct RemoveClipEdit {
    track_id: TrackId,
    clip_id: ClipId,
    removed: Option<Clip>,
}

impl RemoveClipEdit {
    /// Prepares removal of `clip_id` from `track_id`.
    pub fn new(track_id: TrackId, clip_id: ClipId) -> Self {
        Self {
            track_id,
            clip_id,
            removed: None,
        }
    }
}

impl Edit for RemoveClipEdit {
    fn label(&self) -> &'static str {
        "remove clip"
    }

    fn apply(&mut self, session: &mut Session) -> Result<(), AudioError> {
        let track = session
            .track_mut(self.track_id)
            .ok_or(AudioError::TrackNotFound(self.track_id.0))?;
        let removed = track.remove_clip(self.clip_id)?;
        // Keep only the first capture: apply → revert → apply must not lose
        // the original clip contents.
        if self.removed.is_none() {
            self.removed = Some(removed);
        }
        Ok(())
    }

    fn revert(&mut self, session: &mut Session) -> Result<(), AudioError> {
        let clip = self
            .removed
            .clone()
            .ok_or_else(|| AudioError::InvalidEdit("remove clip reverted before apply".into()))?;
        let track = session
            .track_mut(self.track_id)
            .ok_or(AudioError::TrackNotFound(self.track_id.0))?;
        track.insert_clip(clip);
        Ok(())
    }
}

/// Moves a clip to a new timeline start frame.
#[derive(Debug, Serialize, Deserialize)]
pub struct MoveClipEdit {
    track_id: TrackId,
    clip_id: ClipId,
    to_frame: u64,
    from_frame: Option<u64>,
}

impl MoveClipEdit {
    /// Prepares a move of `clip_id` so that it starts at `to_frame`.
    pub fn new(track_id: TrackId, clip_id: ClipId, to_frame: u64) -> Self {
        Self {
            track_id,
            clip_id,
            to_frame,
            from_frame: None,
        }
    }
}

impl Edit for MoveClipEdit {
    fn label(&self) -> &'static str {
        "move clip"
    }

    fn apply(&mut self, session: &mut Session) -> Result<(), AudioError> {
        let track = session
            .track_mut(self.track_id)
            .ok_or(AudioError::TrackNotFound(self.track_id.0))?;
        let clip = track
            .clip_mut(self.clip_id)
            .ok_or(AudioError::ClipNotFound(self.clip_id.0))?;
        if self.from_frame.is_none() {
            self.from_frame = Some(clip.start_frame);
        }
        if clip.start_frame == self.to_frame {
            return Ok(());
        }
        clip.start_frame = self.to_frame;
        // Re-sort the clip list after the position change.
        track.clips.sort_by_key(|c| c.start_frame);
        Ok(())
    }

    fn revert(&mut self, session: &mut Session) -> Result<(), AudioError> {
        let from = self
            .from_frame
            .ok_or_else(|| AudioError::InvalidEdit("move clip reverted before apply".into()))?;
        let track = session
            .track_mut(self.track_id)
            .ok_or(AudioError::TrackNotFound(self.track_id.0))?;
        let clip = track
            .clip_mut(self.clip_id)
            .ok_or(AudioError::ClipNotFound(self.clip_id.0))?;
        clip.start_frame = from;
        track.clips.sort_by_key(|c| c.start_frame);
        Ok(())
    }
}

/// Splits a clip at a timeline frame, producing two clips.
///
/// The left half keeps the original clip id; the right half receives a fresh
/// id minted from the session's generator at apply time.
#[derive(Debug, Serialize, Deserialize)]
pub struct SplitClipEdit {
    track_id: TrackId,
    clip_id: ClipId,
    at_frame: u64,
    /// Captured original clip (for revert).
    original: Option<Clip>,
    /// Id minted for the right half at apply time.
    right_id: Option<ClipId>,
}

impl SplitClipEdit {
    /// Prepares a split of `clip_id` at timeline frame `at_frame`.
    pub fn new(track_id: TrackId, clip_id: ClipId, at_frame: u64) -> Self {
        Self {
            track_id,
            clip_id,
            at_frame,
            original: None,
            right_id: None,
        }
    }
}

impl Edit for SplitClipEdit {
    fn label(&self) -> &'static str {
        "split clip"
    }

    fn apply(&mut self, session: &mut Session) -> Result<(), AudioError> {
        let track = session
            .track_mut(self.track_id)
            .ok_or(AudioError::TrackNotFound(self.track_id.0))?;
        let clip = track
            .clip(self.clip_id)
            .ok_or(AudioError::ClipNotFound(self.clip_id.0))?
            .clone();

        let right_id = match self.right_id {
            Some(id) => id,
            None => {
                let id = session.generate_clip_id();
                self.right_id = Some(id);
                id
            }
        };

        let (left, right) = clip.split_at(self.at_frame, right_id)?;

        if self.original.is_none() {
            self.original = Some(clip);
        }

        let track = session
            .track_mut(self.track_id)
            .ok_or(AudioError::TrackNotFound(self.track_id.0))?;
        track.remove_clip(self.clip_id)?;
        track.insert_clip(left);
        track.insert_clip(right);
        Ok(())
    }

    fn revert(&mut self, session: &mut Session) -> Result<(), AudioError> {
        let original = self
            .original
            .clone()
            .ok_or_else(|| AudioError::InvalidEdit("split clip reverted before apply".into()))?;
        let right_id = self
            .right_id
            .ok_or_else(|| AudioError::InvalidEdit("split clip reverted before apply".into()))?;

        let track = session
            .track_mut(self.track_id)
            .ok_or(AudioError::TrackNotFound(self.track_id.0))?;
        track.remove_clip(right_id)?;
        track.remove_clip(original.id)?;
        track.insert_clip(original);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asset::AssetId;

    fn session() -> Session {
        let mut s = Session::new("Edit test", 48_000);
        s.add_track("A");
        s
    }

    fn clip(session: &mut Session, start: u64, dur: u64) -> Clip {
        Clip {
            id: session.generate_clip_id(),
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
    fn insert_then_revert() {
        let mut s = session();
        let t = TrackId(1);
        let c = clip(&mut s, 100, 200);
        let mut edit = InsertClipEdit::new(t, c.clone());

        edit.apply(&mut s).unwrap();
        assert_eq!(s.track(t).unwrap().clips.len(), 1);
        edit.revert(&mut s).unwrap();
        assert_eq!(s.track(t).unwrap().clips.len(), 0);
    }

    #[test]
    fn remove_captures_for_revert() {
        let mut s = session();
        let t = TrackId(1);
        let c = clip(&mut s, 0, 500);
        let clip_id = c.id;
        s.track_mut(t).unwrap().insert_clip(c);

        let mut edit = RemoveClipEdit::new(t, clip_id);
        edit.apply(&mut s).unwrap();
        assert!(s.track(t).unwrap().clips.is_empty());
        edit.revert(&mut s).unwrap();
        assert_eq!(
            s.track(t).unwrap().clip(clip_id).unwrap().duration_frames,
            500
        );
    }

    #[test]
    fn move_updates_position_and_reverts() {
        let mut s = session();
        let t = TrackId(1);
        let c1 = clip(&mut s, 5_000, 100);
        let c2 = clip(&mut s, 1_000, 100);
        let c1_id = c1.id;
        s.track_mut(t).unwrap().insert_clip(c1);
        s.track_mut(t).unwrap().insert_clip(c2);

        let mut edit = MoveClipEdit::new(t, c1_id, 500);
        edit.apply(&mut s).unwrap();

        let starts: Vec<u64> = s
            .track(t)
            .unwrap()
            .clips
            .iter()
            .map(|c| c.start_frame)
            .collect();
        assert_eq!(starts, [500, 1_000]); // sorted after move

        edit.revert(&mut s).unwrap();
        let starts: Vec<u64> = s
            .track(t)
            .unwrap()
            .clips
            .iter()
            .map(|c| c.start_frame)
            .collect();
        assert_eq!(starts, [1_000, 5_000]);
    }

    #[test]
    fn split_creates_two_and_revert_restores_one() {
        let mut s = session();
        let t = TrackId(1);
        let c = clip(&mut s, 1_000, 1_000);
        let original_id = c.id;
        s.track_mut(t).unwrap().insert_clip(c);

        let mut edit = SplitClipEdit::new(t, original_id, 1_400);
        edit.apply(&mut s).unwrap();

        let clips = &s.track(t).unwrap().clips;
        assert_eq!(clips.len(), 2);
        assert_eq!(clips[0].duration_frames, 400);
        assert_eq!(clips[1].start_frame, 1_400);
        assert_ne!(clips[0].id, clips[1].id);

        edit.revert(&mut s).unwrap();
        let clips = &s.track(t).unwrap().clips;
        assert_eq!(clips.len(), 1);
        assert_eq!(clips[0].id, original_id);
        assert_eq!(clips[0].duration_frames, 1_000);
    }

    #[test]
    fn operations_on_missing_track_fail_cleanly() {
        let mut s = session();
        let missing = TrackId(99);
        let c = clip(&mut s, 0, 10);
        let mut e = InsertClipEdit::new(missing, c);
        assert!(matches!(
            e.apply(&mut s),
            Err(AudioError::TrackNotFound(99))
        ));
    }
}
