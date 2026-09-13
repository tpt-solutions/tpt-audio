//! The shared error type for the whole engine.

use std::fmt;

/// Errors produced anywhere in the `tpt-av-audio-*` stack.
#[derive(Debug)]
pub enum AudioError {
    /// The operation (backend, feature, or format) is not supported by this
    /// build or platform.
    Unsupported(String),
    /// A configuration value is invalid (bad channel count, frame range…).
    InvalidConfig(String),
    /// The requested audio device does not exist (or was unplugged).
    DeviceNotFound(String),
    /// A backend-level failure (OS audio API returned an error).
    Backend(String),
    /// Filesystem failure.
    Io(std::io::Error),
    /// An audio file could not be decoded.
    Decode(String),
    /// A destination buffer is smaller than the data that must fit in it.
    BufferTooSmall {
        /// Number of samples required.
        needed: usize,
        /// Number of samples available.
        available: usize,
    },
    /// No track with this id exists in the session.
    TrackNotFound(u64),
    /// No clip with this id exists on the track (or in the session).
    ClipNotFound(u64),
    /// No asset with this id is registered/loaded.
    AssetNotFound(u64),
    /// The edit operation is invalid for the current session state.
    InvalidEdit(String),
}

impl fmt::Display for AudioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AudioError::Unsupported(what) => write!(f, "unsupported operation: {what}"),
            AudioError::InvalidConfig(why) => write!(f, "invalid configuration: {why}"),
            AudioError::DeviceNotFound(id) => write!(f, "audio device not found: {id}"),
            AudioError::Backend(why) => write!(f, "audio backend error: {why}"),
            AudioError::Io(e) => write!(f, "I/O error: {e}"),
            AudioError::Decode(why) => write!(f, "decode error: {why}"),
            AudioError::BufferTooSmall { needed, available } => {
                write!(
                    f,
                    "buffer too small: needed {needed}, available {available}"
                )
            }
            AudioError::TrackNotFound(id) => write!(f, "track not found: {id}"),
            AudioError::ClipNotFound(id) => write!(f, "clip not found: {id}"),
            AudioError::AssetNotFound(id) => write!(f, "asset not found: {id}"),
            AudioError::InvalidEdit(why) => write!(f, "invalid edit: {why}"),
        }
    }
}

impl std::error::Error for AudioError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            AudioError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for AudioError {
    fn from(e: std::io::Error) -> Self {
        AudioError::Io(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_is_human_readable() {
        let e = AudioError::DeviceNotFound("dev_42".into());
        assert_eq!(e.to_string(), "audio device not found: dev_42");

        let e = AudioError::BufferTooSmall {
            needed: 8,
            available: 4,
        };
        assert_eq!(e.to_string(), "buffer too small: needed 8, available 4");
    }

    #[test]
    fn io_errors_convert() {
        let e: AudioError = std::io::Error::new(std::io::ErrorKind::NotFound, "nope").into();
        assert!(matches!(e, AudioError::Io(_)));
        assert!(std::error::Error::source(&e).is_some());
    }
}
