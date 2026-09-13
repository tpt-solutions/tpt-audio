//! Platform audio backends.
//!
//! [`AudioBackend`] is the abstraction over OS audio APIs. Available
//! implementations:
//!
//! | Module | Platform | Status |
//! |---|---|---|
//! | [`wasapi`] | Windows | Shared-mode render/capture (ported from the old `platform-windows` crate) |
//! | [`pipewire`] | Linux | Device enumeration via `pw-dump`; streams pending native `pipewire-rs` |
//! | [`coreaudio`] | macOS | Stub — implementation pending |
//! | [`archon`] | Archon | Research-gated stub (blocked on `tpt-archon-bridge`) |
//! | [`NullBackend`] | all | Test/CI sink, no OS dependency |

#[cfg(target_os = "macos")]
pub mod coreaudio;
#[cfg(target_os = "linux")]
pub mod pipewire;
#[cfg(target_os = "windows")]
pub mod wasapi;

pub mod archon;
pub mod null;

use crate::device::AudioDevice;
use crate::stream::{DeviceReader, DeviceWriter, StreamConfig};
use tpt_av_audio_utils::AudioError;

/// Abstraction over an OS audio API.
///
/// Implementations are constructed on the Main Thread; device I/O objects
/// ([`DeviceWriter`]/[`DeviceReader`]) are handed to stream driver threads
/// and must therefore be `Send` and perform any per-thread API setup
/// (e.g. COM initialization) lazily on the driver thread.
pub trait AudioBackend: Send {
    /// Backend name for diagnostics ("wasapi", "pipewire", …).
    fn name(&self) -> &'static str;

    /// Lists active audio devices (outputs and inputs).
    fn enumerate_devices(&self) -> Result<Vec<AudioDevice>, AudioError>;

    /// Opens a sink that receives interleaved f32 audio for playback.
    fn open_output_writer(
        &self,
        device: &AudioDevice,
        config: StreamConfig,
    ) -> Result<Box<dyn DeviceWriter>, AudioError>;

    /// Opens a source that yields interleaved f32 audio from capture.
    fn open_input_reader(
        &self,
        device: &AudioDevice,
        config: StreamConfig,
    ) -> Result<Box<dyn DeviceReader>, AudioError>;
}

/// Selects the best backend for the current platform.
///
/// Setting `TPT_AUDIO_BACKEND=null` forces the [`NullBackend`], which is what
/// most CI and unit tests want.
pub fn default_backend() -> Result<Box<dyn AudioBackend>, AudioError> {
    if std::env::var("TPT_AUDIO_BACKEND").as_deref() == Ok("null") {
        return Ok(Box::new(null::NullBackend::new()));
    }

    #[cfg(target_os = "windows")]
    {
        Ok(Box::new(wasapi::WasapiBackend::new()?))
    }

    #[cfg(target_os = "linux")]
    {
        Ok(Box::new(pipewire::PipewireBackend::new()?))
    }

    #[cfg(target_os = "macos")]
    {
        Err(AudioError::Unsupported(
            "CoreAudio backend is not yet implemented; use TPT_AUDIO_BACKEND=null".into(),
        ))
    }

    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        Err(AudioError::Unsupported(
            "no audio backend for this platform; use TPT_AUDIO_BACKEND=null".into(),
        ))
    }
}
