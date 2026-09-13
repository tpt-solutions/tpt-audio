//! CoreAudio backend (macOS) — **stub**.
//!
//! New surface for the engine pivot: the old router repo never shipped a
//! macOS backend. The real implementation (AUHAL / `AudioUnit` render
//! callbacks over `coreaudio-rs` or a hand-rolled `objc2` binding) is future
//! work; everything returns [`AudioError::Unsupported`] for now so the API
//! shape is exercised on macOS builds and CI can select the null backend.

use crate::device::AudioDevice;
use crate::stream::{DeviceReader, DeviceWriter, StreamConfig};
use tpt_av_audio_utils::AudioError;

/// macOS CoreAudio backend (stub).
pub struct CoreAudioBackend;

impl CoreAudioBackend {
    /// Creates the stub backend.
    pub fn new() -> Result<Self, AudioError> {
        Ok(Self)
    }
}

impl crate::backend::AudioBackend for CoreAudioBackend {
    fn name(&self) -> &'static str {
        "coreaudio"
    }

    fn enumerate_devices(&self) -> Result<Vec<AudioDevice>, AudioError> {
        Err(AudioError::Unsupported(
            "CoreAudio device enumeration is not yet implemented",
        ))
    }

    fn open_output_writer(
        &self,
        _device: &AudioDevice,
        _config: StreamConfig,
    ) -> Result<Box<dyn DeviceWriter>, AudioError> {
        Err(AudioError::Unsupported(
            "CoreAudio playback is not yet implemented",
        ))
    }

    fn open_input_reader(
        &self,
        _device: &AudioDevice,
        _config: StreamConfig,
    ) -> Result<Box<dyn DeviceReader>, AudioError> {
        Err(AudioError::Unsupported(
            "CoreAudio capture is not yet implemented",
        ))
    }
}
