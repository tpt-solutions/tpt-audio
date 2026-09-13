//! Device enumeration and selection (spec2 §4.3).

use serde::{Deserialize, Serialize};

use crate::backend;
use tpt_av_audio_utils::AudioError;

/// Unique identifier for an [`AudioDevice`].
///
/// Backend-defined: a WASAPI endpoint string on Windows, a PipeWire node id
/// on Linux, a UID on CoreAudio. Treat it as opaque.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DeviceId(pub String);

impl DeviceId {
    /// Creates a device id from a raw string.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

/// Which directions a device can serve.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Direction {
    /// Playback (render) device.
    Output,
    /// Recording (capture) device.
    Input,
}

/// An audio device exposed by the host OS.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioDevice {
    /// Unique device identifier.
    pub id: DeviceId,
    /// Human-readable device name (e.g. "Built-in Output", "Focusrite USB").
    pub name: String,
    /// Whether this is an output (render) or input (capture) device.
    pub direction: Direction,
    /// Number of input channels (0 for pure output devices).
    pub input_channels: u16,
    /// Number of output channels (0 for pure input devices).
    pub output_channels: u16,
    /// Supported sample rates, in Hz.
    pub sample_rates: Vec<u32>,
    /// Supported buffer sizes, in frames per callback.
    pub buffer_sizes: Vec<usize>,
    /// Whether this is the OS default device for its direction.
    pub is_default: bool,
}

impl AudioDevice {
    /// Convenience constructor for a simple fixed-format device (used by
    /// lightweight backends that can't query full capability lists).
    pub fn simple(
        id: impl Into<String>,
        name: impl Into<String>,
        direction: Direction,
        channels: u16,
        is_default: bool,
    ) -> Self {
        Self {
            id: DeviceId::new(id),
            name: name.into(),
            direction,
            input_channels: if direction == Direction::Input {
                channels
            } else {
                0
            },
            output_channels: if direction == Direction::Output {
                channels
            } else {
                0
            },
            sample_rates: vec![44_100, 48_000],
            buffer_sizes: vec![128, 256, 512, 1_024],
            is_default,
        }
    }
}

/// Enumerates available audio devices via the default backend.
pub fn enumerate_devices() -> Result<Vec<AudioDevice>, AudioError> {
    backend::default_backend()?.enumerate_devices()
}

/// Returns the OS default output device, if one exists.
pub fn default_output_device() -> Result<AudioDevice, AudioError> {
    let devices = enumerate_devices()?;
    devices
        .iter()
        .find(|d| d.direction == Direction::Output && d.is_default)
        .or_else(|| devices.iter().find(|d| d.direction == Direction::Output))
        .cloned()
        .ok_or(AudioError::DeviceNotFound("default output".into()))
}
