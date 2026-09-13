//! Dependency-free test sink: a device that swallows audio and paces itself
//! in real time.
//!
//! `NullBackend` always exposes one stereo output device and one stereo
//! input device. Writers/readers pace callbacks by buffer duration so tests
//! that start a real [`crate::stream::OutputStream`] can measure callback
//! progress with plain sleeps. A shared frame counter makes assertions
//! possible without any OS audio stack.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use super::AudioBackend;
use crate::device::{AudioDevice, Direction};
use crate::stream::{DeviceReader, DeviceWriter, StreamConfig};
use tpt_av_audio_utils::{AudioBuffer, AudioError};

/// Counters shared between a [`NullBackend`] and its open streams.
#[derive(Debug, Default)]
pub struct NullStats {
    pub frames_written: AtomicU64,
    pub frames_read: AtomicU64,
}

/// A no-OS backend for tests and CI.
pub struct NullBackend {
    stats: Arc<NullStats>,
    /// Optional simulated device failure for writer open calls.
    fail_writes: Mutex<bool>,
}

impl NullBackend {
    /// Creates a null backend with fresh counters.
    pub fn new() -> Self {
        Self {
            stats: Arc::new(NullStats::default()),
            fail_writes: Mutex::new(false),
        }
    }

    /// Snapshot of the shared counters.
    pub fn stats(&self) -> Arc<NullStats> {
        Arc::clone(&self.stats)
    }

    /// Makes subsequent output-stream writes fail (simulates a dead device).
    pub fn set_fail_writes(&self, fail: bool) {
        *self.fail_writes.lock().unwrap() = fail;
    }
}

impl Default for NullBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioBackend for NullBackend {
    fn name(&self) -> &'static str {
        "null"
    }

    fn enumerate_devices(&self) -> Result<Vec<AudioDevice>, AudioError> {
        Ok(vec![
            AudioDevice::simple(
                "null-output",
                "Null Output (test sink)",
                Direction::Output,
                2,
                true,
            ),
            AudioDevice::simple(
                "null-input",
                "Null Input (test source)",
                Direction::Input,
                2,
                true,
            ),
        ])
    }

    fn open_output_writer(
        &self,
        _device: &AudioDevice,
        config: StreamConfig,
    ) -> Result<Box<dyn DeviceWriter>, AudioError> {
        if *self.fail_writes.lock().unwrap() {
            return Err(AudioError::Backend("null backend: writes disabled".into()));
        }
        Ok(Box::new(NullWriter {
            config,
            stats: Arc::clone(&self.stats),
        }))
    }

    fn open_input_reader(
        &self,
        _device: &AudioDevice,
        config: StreamConfig,
    ) -> Result<Box<dyn DeviceReader>, AudioError> {
        Ok(Box::new(NullReader {
            config,
            stats: Arc::clone(&self.stats),
        }))
    }
}

struct NullWriter {
    config: StreamConfig,
    stats: Arc<NullStats>,
}

impl DeviceWriter for NullWriter {
    fn write(&mut self, buffer: &AudioBuffer) -> Result<(), AudioError> {
        self.stats
            .frames_written
            .fetch_add(buffer.frames as u64, Ordering::Relaxed);
        crate::stream::pace_by_buffer_duration(self.config);
        Ok(())
    }
}

struct NullReader {
    config: StreamConfig,
    stats: Arc<NullStats>,
}

impl DeviceReader for NullReader {
    fn read(&mut self, buffer: &mut AudioBuffer) -> Result<(), AudioError> {
        // Digital silence; count frames so tests can observe progress.
        buffer.clear();
        self.stats
            .frames_read
            .fetch_add(buffer.frames as u64, Ordering::Relaxed);
        crate::stream::pace_by_buffer_duration(self.config);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enumerates_null_devices() {
        let b = NullBackend::new();
        let devices = b.enumerate_devices().unwrap();
        assert_eq!(devices.len(), 2);
        assert!(devices
            .iter()
            .any(|d| d.direction == Direction::Output && d.is_default));
    }
}
