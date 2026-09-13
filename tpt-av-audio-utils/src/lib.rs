//! Shared foundation types for the `tpt-av-audio-*` engine workspace.
//!
//! This crate is the dependency-free bottom layer of the stack:
//!
//! - [`sample`] — [`Sample`] conversions between PCM widths (`f32`, `i16`, …).
//! - [`buffer`] — the canonical interleaved [`buffer::AudioBuffer`].
//! - [`time`] — frame/second/[`time::Milliseconds`] conversions.
//! - [`error`] — the shared [`error::AudioError`] enum.
//!
//! It intentionally has no external dependencies so every other crate in the
//! workspace can build on it without licensing or compile-time risk.

pub mod buffer;
pub mod error;
pub mod sample;
pub mod time;

pub use buffer::AudioBuffer;
pub use error::AudioError;
pub use sample::Sample;
pub use time::{Frames, Milliseconds, Seconds};
