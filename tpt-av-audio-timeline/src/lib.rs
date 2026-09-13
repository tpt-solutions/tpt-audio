//! The non-destructive timeline data model.
//!
//! This crate describes *what* audio plays, *when*, and *how* — it never
//! processes audio itself. The original media files on disk are never
//! mutated; clips are metadata references into [`AudioAsset`]s.
//!
//! Layer map (spec2 §4.1):
//!
//! - [`Session`] — top-level collection of [`Track`]s plus sample rate/metadata.
//! - [`Track`] — named strip of [`Clip`]s with volume/pan/mute/solo.
//! - [`Clip`] — a time range referencing an asset, with envelopes and fades.
//! - [`AudioAsset`] — a file on disk plus its decoded-format facts.
//! - [`Envelope`] — automation points with linear/cubic/step interpolation.
//! - [`edit`] / [`History`] — undoable operations over a `Session`.
//!
//! Everything implements `serde` so a session round-trips as JSON for
//! headless rendering.

pub mod asset;
pub mod clip;
pub mod edit;
pub mod envelope;
pub mod history;
pub mod session;
pub mod track;

pub use asset::{AssetId, AudioAsset};
pub use clip::{Clip, ClipId};
pub use edit::{Edit, InsertClipEdit, MoveClipEdit, RemoveClipEdit, SplitClipEdit};
pub use envelope::{Envelope, EnvelopePoint, InterpolationMethod};
pub use history::History;
pub use session::{IdGenerator, Session, SessionId, SessionMetadata};
pub use track::{Track, TrackId};
