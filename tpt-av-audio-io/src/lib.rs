//! # tpt-av-audio-io
//!
//! OS audio I/O for the `tpt-av-audio-*` engine: device enumeration, stream
//! management, and the platform backends (WASAPI on Windows, PipeWire on
//! Linux, CoreAudio on macOS, plus a research-gated Archon stub and a
//! dependency-free [`backend::NullBackend`] for tests).
//!
//! Replaces the old router-app `AudioBackend` trait (per-app loopback routes)
//! with the engine-facing surface from spec2 §4.3: [`device::enumerate_devices`],
//! [`stream::OutputStream`], and [`stream::InputStream`].
//!
//! Backend selection: [`backend::default_backend`] picks the platform backend
//! at runtime; `TPT_AUDIO_BACKEND=null` forces the null backend (useful in CI
//! and tests).

pub mod backend;
pub mod device;
pub mod router;
pub mod stream;

pub use backend::{null::NullBackend, AudioBackend};
pub use device::{default_output_device, enumerate_devices, AudioDevice, DeviceId, Direction};
pub use stream::{InputStream, OutputCallback, OutputStream, StreamConfig, StreamHandle};
pub use tpt_av_audio_utils::AudioError;
