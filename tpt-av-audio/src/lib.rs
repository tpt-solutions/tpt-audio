//! # tpt-av-audio
//!
//! The one-crate facade over the `tpt-av-audio-*` engine workspace: add
//! this single dependency and get the whole non-destructive audio engine —
//! timeline, real-time mixer, OS I/O, and cadence-backed decoding.
//!
//! ## The 20-second version
//!
//! ```no_run
//! use tpt_av_audio::{Engine, Session};
//!
//! # fn main() -> Result<(), tpt_av_audio_utils::AudioError> {
//! let mut engine = Engine::new(Session::new("demo", 48_000), 512, 2)?;
//! let voice = engine.load_asset("voice.wav")?; // decode + cache + register
//!
//! let mut session = engine.session();
//! let track = session.add_track("vocals");
//! let clip = tpt_av_audio::Clip::new(session.generate_clip_id(), voice, 0, 48_000);
//! session.track_mut(track).unwrap().insert_clip(clip);
//! engine.set_session(session);
//!
//! let mut out = tpt_av_audio::AudioBuffer::new(512, 2);
//! engine.render(&mut out)?; // 512 frames of mixed audio
//! # Ok(())
//! # }
//! ```
//!
//! One-call helpers:
//!
//! - [`offline::render_session_to_wav`] — timeline JSON-style session → WAV file.
//! - [`play_file`] — decode an audio file and play it through the OS backend.
//!
//! Full control (custom graphs, DSP nodes, plugin buses, backends) lives in
//! the re-exported workspace crates below.

pub mod engine;
pub mod offline;
pub mod player;

pub use offline::WavExportFormat;

pub use engine::Engine;
pub use player::{play_file, play_file_blocking, Playback};

// The workspace surface, re-exported for one-stop imports.
pub use tpt_av_audio_core as core_engine;
pub use tpt_av_audio_io as io;
pub use tpt_av_audio_timeline as timeline;
pub use tpt_av_audio_utils as utils;

pub use tpt_av_audio_core::{
    AssetPcm, AssetStore, AudioGraph, AudioNode, DecodeRegistry, TimelineRenderer, TimelineState,
};
pub use tpt_av_audio_io::{
    enumerate_devices, AudioBackend, AudioDevice, AudioError, NullBackend, OutputStream,
    StreamConfig, StreamHandle,
};
pub use tpt_av_audio_timeline::{AudioAsset, Clip, Envelope, Session, Track};
pub use tpt_av_audio_utils::AudioBuffer;
