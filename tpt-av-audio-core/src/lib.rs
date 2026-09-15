//! # tpt-av-audio-core
//!
//! The real-time engine: mixer graph, built-in DSP, timeline rendering, and
//! lock-free Main-Thread → Audio-Thread state synchronization (spec2 §4.2/§5).
//!
//! Real-time contract (enforced by design and by `tests/rt_safety.rs`):
//! everything that runs inside [`AudioNode::process`] or
//! [`TimelineRenderer::render`] is allocation-free, lock-free, and
//! panic-free. All setup, allocation, and blocking happens on the Main
//! Thread through the `prepare`/`update` entry points.
//!
//! Layer map:
//!
//! - [`graph`] — [`AudioNode`] trait and the summing-bus [`AudioGraph`].
//! - [`mixer`] — multi-track [`TrackMixer`] (gain/pan/mute/solo).
//! - [`renderer`] — [`TimelineRenderer`]: snapshot → PCM → envelopes → mix.
//! - [`scheduler`] — [`TimelineState`] lock-free double-buffered snapshots.
//! - [`dsp`] — gain, pan, fade, channel mapping, resampling (rubato +
//!   inline linear), real-time metering.
//! - [`overview`] — waveform peak-bucket caches for editor rendering.
//! - [`asset`] / [`decode`] / [`pool`] / [`ring`] — asset caches, WAV
//!   decoding, background decode pool, lock-free SPSC handoff.

// The crate's only unsafe code is the SPSC ring buffer (`ring.rs`), which
// is the canonical UnsafeCell producer/consumer layout and is documented
// there. Everything else is safe by construction.
#![deny(unsafe_op_in_unsafe_fn)]

pub mod asset;
pub mod decode;
pub mod dsp;
pub mod graph;
pub mod mixer;
pub mod overview;
pub mod pool;
pub mod renderer;
pub mod ring;
pub mod scheduler;

pub use asset::{AssetPcm, AssetStore};
pub use decode::{DecodeRegistry, DecodedAudio, Decoder};
pub use graph::{AudioGraph, AudioNode, NodeId};
pub use mixer::{TrackBus, TrackMixer};
pub use overview::WaveformOverview;
pub use pool::DecodePool;
pub use renderer::TimelineRenderer;
pub use scheduler::{SessionSnapshot, TimelineState};

pub use tpt_av_audio_utils::{AudioBuffer, AudioError};
