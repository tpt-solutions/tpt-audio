//! Undo/redo history over a [`Session`].

use crate::edit::Edit;
use crate::session::Session;
use tpt_av_audio_utils::AudioError;

/// Bounded undo/redo stack of boxed [`Edit`] operations.
///
/// `undo` and `redo` never drop a failed edit: if an operation errors, it is
/// returned to the front of its stack so history stays consistent.
#[derive(Default)]
pub struct History {
    undo_stack: Vec<Box<dyn Edit>>,
    redo_stack: Vec<Box<dyn Edit>>,
    limit: usize,
}

impl History {
    /// Creates a history keeping at most `limit` undo steps (0 = unbounded).
    pub fn new(limit: usize) -> Self {
        Self {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            limit,
        }
    }

    /// Applies `edit` to `session`, pushing it onto the undo stack and
    /// clearing the redo stack (standard editor semantics).
    pub fn apply(
        &mut self,
        mut edit: Box<dyn Edit>,
        session: &mut Session,
    ) -> Result<(), AudioError> {
        edit.apply(session)?;
        self.undo_stack.push(edit);
        self.redo_stack.clear();
        self.trim();
        Ok(())
    }

    /// Undoes the most recent edit. Returns `false` if there is nothing to
    /// undo.
    pub fn undo(&mut self, session: &mut Session) -> Result<bool, AudioError> {
        let Some(mut edit) = self.undo_stack.pop() else {
            return Ok(false);
        };
        match edit.revert(session) {
            Ok(()) => {
                self.redo_stack.push(edit);
                Ok(true)
            }
            Err(e) => {
                // Put it back so history is not corrupted by a failed revert.
                self.undo_stack.push(edit);
                Err(e)
            }
        }
    }

    /// Redoes the most recently undone edit. Returns `false` if there is
    /// nothing to redo.
    pub fn redo(&mut self, session: &mut Session) -> Result<bool, AudioError> {
        let Some(mut edit) = self.redo_stack.pop() else {
            return Ok(false);
        };
        match edit.apply(session) {
            Ok(()) => {
                self.undo_stack.push(edit);
                Ok(true)
            }
            Err(e) => {
                self.redo_stack.push(edit);
                Err(e)
            }
        }
    }

    /// Whether an undo is available.
    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    /// Whether a redo is available.
    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    /// Number of undoable edits currently held.
    pub fn undo_depth(&self) -> usize {
        self.undo_stack.len()
    }

    fn trim(&mut self) {
        if self.limit > 0 && self.undo_stack.len() > self.limit {
            let overflow = self.undo_stack.len() - self.limit;
            self.undo_stack.drain(..overflow);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asset::AssetId;
    use crate::clip::Clip;
    use crate::edit::{InsertClipEdit, MoveClipEdit, SplitClipEdit};
    use crate::track::TrackId;

    fn make_clip(session: &mut Session, start: u64, dur: u64) -> Clip {
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
    fn apply_undo_redo_cycle() {
        let mut s = Session::new("H", 48_000);
        let t = TrackId(1);
        s.add_track("A");
        let c = make_clip(&mut s, 0, 100);

        let mut h = History::default();
        h.apply(Box::new(InsertClipEdit::new(t, c.clone())), &mut s)
            .unwrap();
        assert_eq!(s.track(t).unwrap().clips.len(), 1);
        assert!(h.can_undo());
        assert!(!h.can_redo());

        assert!(h.undo(&mut s).unwrap());
        assert_eq!(s.track(t).unwrap().clips.len(), 0);

        assert!(h.redo(&mut s).unwrap());
        assert_eq!(s.track(t).unwrap().clips.len(), 1);

        assert!(!h.undo(&mut s).is_err());
        assert!(h.can_redo());
    }

    #[test]
    fn new_edit_clears_redo_stack() {
        let mut s = Session::new("H", 48_000);
        let t = TrackId(1);
        s.add_track("A");
        let c1 = make_clip(&mut s, 0, 100);
        let c2 = make_clip(&mut s, 500, 100);

        let mut h = History::default();
        h.apply(Box::new(InsertClipEdit::new(t, c1)), &mut s)
            .unwrap();
        h.undo(&mut s).unwrap();
        assert!(h.can_redo());

        h.apply(Box::new(InsertClipEdit::new(t, c2)), &mut s)
            .unwrap();
        assert!(!h.can_redo());
    }

    #[test]
    fn limit_bounded() {
        let mut s = Session::new("H", 48_000);
        let t = TrackId(1);
        s.add_track("A");
        let mut h = History::new(2);

        for i in 0..5u64 {
            let c = make_clip(&mut s, i * 1_000, 100);
            h.apply(Box::new(InsertClipEdit::new(t, c)), &mut s)
                .unwrap();
        }
        assert_eq!(h.undo_depth(), 2);

        // Undo twice lands on a state with 3 clips remaining.
        h.undo(&mut s).unwrap();
        h.undo(&mut s).unwrap();
        assert_eq!(s.track(t).unwrap().clips.len(), 3);
        assert!(!h.can_undo());
    }

    #[test]
    fn undo_nothing_is_false_not_error() {
        let mut s = Session::new("H", 48_000);
        let mut h = History::default();
        assert!(!h.undo(&mut s).unwrap());
        assert!(!h.redo(&mut s).unwrap());
    }

    #[test]
    fn move_split_undo_redo_sequence() {
        let mut s = Session::new("H", 48_000);
        let t = TrackId(1);
        s.add_track("A");
        let c = make_clip(&mut s, 1_000, 1_000);

        let mut h = History::default();
        let clip_id = c.id;
        h.apply(Box::new(InsertClipEdit::new(t, c)), &mut s)
            .unwrap();
        h.apply(Box::new(SplitClipEdit::new(t, clip_id, 1_500)), &mut s)
            .unwrap();

        // After split: two clips.
        assert_eq!(s.track(t).unwrap().clips.len(), 2);
        h.undo(&mut s).unwrap();
        assert_eq!(s.track(t).unwrap().clips.len(), 1);
        h.redo(&mut s).unwrap();
        assert_eq!(s.track(t).unwrap().clips.len(), 2);

        // Move the left half past the right half, then undo the move only.
        // Note the sort-by-start reorders the vec, so look up by id.
        let left_id = s.track(t).unwrap().clips[0].id;
        h.apply(Box::new(MoveClipEdit::new(t, left_id, 5_000)), &mut s)
            .unwrap();
        let left = s
            .track(t)
            .unwrap()
            .clips
            .iter()
            .find(|c| c.id == left_id)
            .unwrap();
        assert_eq!(left.start_frame, 5_000);
        h.undo(&mut s).unwrap();
        let left = s
            .track(t)
            .unwrap()
            .clips
            .iter()
            .find(|c| c.id == left_id)
            .unwrap();
        assert_eq!(left.start_frame, 1_000);
    }
}
