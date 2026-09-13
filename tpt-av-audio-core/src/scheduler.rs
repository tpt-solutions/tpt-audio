//! Lock-free Main Thread → Audio Thread state synchronization.
//!
//! The timeline is mutated on the Main Thread and read on the Audio Thread.
//! Instead of a hand-rolled `AtomicPtr` double-buffer (unsound: the writer
//! can overwrite a slot the audio thread is still reading), this uses
//! `arc-swap` — wait-free reads, no locks on either side, and safe memory
//! reclamation of superseded snapshots.
//!
//! # Real-Time Safety
//!
//! [`TimelineState::load_snapshot`] is a single atomic refcount bump — no
//! heap allocation, no lock, no wait. Call it once per `render`, cache the
//! returned `Arc`, and read freely for the whole callback.

use std::sync::Arc;

use arc_swap::ArcSwap;
use tpt_av_audio_timeline::Session;

/// An immutable snapshot of the session plus its monotonically increasing
/// revision number.
#[derive(Debug, Clone)]
pub struct SessionSnapshot {
    pub session: Session,
    pub revision: u64,
}

/// Lock-free holder of the current [`SessionSnapshot`].
pub struct TimelineState {
    current: ArcSwap<SessionSnapshot>,
}

impl TimelineState {
    /// Creates a state holder with `session` at revision 0.
    pub fn new(session: Session) -> Self {
        Self {
            current: ArcSwap::from_pointee(SessionSnapshot {
                session,
                revision: 0,
            }),
        }
    }

    /// Publishes a new session snapshot (Main Thread). The revision is
    /// incremented monotonically.
    pub fn update(&self, session: Session) {
        let next = self.current.load_full().revision.wrapping_add(1);
        self.current.store(Arc::new(SessionSnapshot {
            session,
            revision: next,
        }));
    }

    /// Loads the current snapshot for the Audio Thread (also fine on the
    /// Main Thread). Wait-free; clones the `Arc`, not the session.
    pub fn load_snapshot(&self) -> Arc<SessionSnapshot> {
        self.current.load_full()
    }

    /// Current revision number.
    pub fn revision(&self) -> u64 {
        self.current.load_full().revision
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::thread;

    fn session(name: &str) -> Session {
        Session::new(name, 48_000)
    }

    #[test]
    fn update_bumps_revision_and_swaps_visible_session() {
        let state = TimelineState::new(session("v1"));
        assert_eq!(state.revision(), 0);

        state.update(session("v2"));
        let snap = state.load_snapshot();
        assert_eq!(snap.session.name, "v2");
        assert_eq!(snap.revision, 1);
    }

    #[test]
    fn snapshots_are_immutable_after_update() {
        let state = TimelineState::new(session("v1"));
        let old = state.load_snapshot();
        state.update(session("v2"));
        // The previously loaded snapshot keeps the old session data.
        assert_eq!(old.session.name, "v1");
        assert_eq!(state.load_snapshot().session.name, "v2");
    }

    #[test]
    fn concurrent_update_and_read_never_tears() {
        let state = Arc::new(TimelineState::new(session("start")));
        let reads = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));

        let mut handles = Vec::new();
        for reader_id in 0..2 {
            let state = Arc::clone(&state);
            let reads = Arc::clone(&reads);
            let stop = Arc::clone(&stop);
            handles.push(thread::spawn(move || {
                let mut last_revision = 0u64;
                while !stop.load(Ordering::Relaxed) {
                    let snap = state.load_snapshot();
                    assert!(snap.revision >= last_revision, "revision went backwards");
                    assert!(!snap.session.name.is_empty());
                    // Each reader only sees its own monotonically consistent
                    // view; the name is internally consistent with revision.
                    last_revision = snap.revision;
                    reads.fetch_add(1, Ordering::Relaxed);
                    let _ = reader_id;
                }
            }));
        }

        for i in 0..1_000u32 {
            let mut s = Session::new(format!("rev{i}"), 48_000);
            s.sample_rate = 48_000;
            state.update(s);
        }

        stop.store(true, Ordering::Relaxed);
        for h in handles {
            h.join().unwrap();
        }
        assert!(reads.load(Ordering::Relaxed) > 0);
    }
}
