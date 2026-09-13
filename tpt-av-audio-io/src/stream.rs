//! Audio stream management: callback-driven input/output streams.
//!
//! A stream pairs a caller-supplied callback ([`OutputCallback`]) with a
//! backend-provided device sink/source ([`DeviceWriter`] / [`DeviceReader`]).
//! The stream layer owns the driver thread and pacing; the backend owns the
//! platform API calls. The callback runs on the stream's driver thread — the
//! same real-time rules as the engine's audio thread apply (no allocation,
//! no locking, no blocking).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tpt_av_audio_utils::{AudioBuffer, AudioError};

use crate::device::AudioDevice;

/// Format and buffer parameters for a stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamConfig {
    /// Sample rate in Hz (e.g. 48_000).
    pub sample_rate: u32,
    /// Channels interleaved in the buffer.
    pub channels: u16,
    /// Frames per callback.
    pub buffer_size: usize,
}

impl StreamConfig {
    /// The engine-default configuration: 48 kHz stereo, 256-frame buffers.
    pub fn default_output() -> Self {
        Self {
            sample_rate: tpt_av_audio_utils::time::DEFAULT_SAMPLE_RATE,
            channels: 2,
            buffer_size: 256,
        }
    }
}

/// Callback invoked by an output stream each period to fill the output
/// buffer. Implementations must be real-time safe.
pub type OutputCallback = Box<dyn FnMut(&mut AudioBuffer) + Send>;

/// Callback invoked by an input stream each period with freshly captured
/// audio. Implementations must be real-time safe.
pub type InputCallback = Box<dyn FnMut(&AudioBuffer) + Send>;

/// Backend-side sink for rendered audio (one stream, one device).
pub trait DeviceWriter: Send {
    /// Writes (blocking until the device accepts) one buffer of audio.
    fn write(&mut self, buffer: &AudioBuffer) -> Result<(), AudioError>;
}

/// Backend-side source for captured audio.
pub trait DeviceReader: Send {
    /// Reads (blocking until audio is available) one buffer of audio.
    fn read(&mut self, buffer: &mut AudioBuffer) -> Result<(), AudioError>;
}

/// Shared control block between the stream handle and its driver thread.
struct StreamControl {
    running: AtomicBool,
    errors: Mutex<Vec<AudioError>>,
}

impl StreamControl {
    fn new() -> Self {
        Self {
            running: AtomicBool::new(false),
            errors: Mutex::new(Vec::new()),
        }
    }

    fn push_error(&self, e: AudioError) {
        if let Ok(mut errors) = self.errors.lock() {
            // Bound the log so a wedged device can't grow it forever.
            if errors.len() < 64 {
                errors.push(e);
            }
        }
    }
}

/// Handle to a started stream: poll health, take errors, and stop.
pub struct StreamHandle {
    control: Arc<StreamControl>,
    thread: Option<JoinHandle<()>>,
}

impl StreamHandle {
    /// Whether the driver thread believes it is still running.
    pub fn is_running(&self) -> bool {
        self.control.running.load(Ordering::SeqCst)
    }

    /// Drains errors recorded by the driver thread (device failures etc.).
    pub fn take_errors(&self) -> Vec<AudioError> {
        self.control
            .errors
            .lock()
            .map(|mut e| std::mem::take(&mut *e))
            .unwrap_or_default()
    }

    /// Signals the driver thread to stop and joins it.
    pub fn stop(&mut self) {
        self.control.running.store(false, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for StreamHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

/// An active output (playback) stream.
pub struct OutputStream;

impl OutputStream {
    /// Opens `device` for playback and starts the driver thread.
    ///
    /// `callback` is invoked once per `config.buffer_size` frames; its output
    /// is handed to the backend writer.
    pub fn start(
        device: &AudioDevice,
        config: StreamConfig,
        writer: Box<dyn DeviceWriter>,
        mut callback: OutputCallback,
    ) -> Result<StreamHandle, AudioError> {
        let control = Arc::new(StreamControl::new());
        control.running.store(true, Ordering::SeqCst);

        let thread_control = Arc::clone(&control);
        let thread = thread::Builder::new()
            .name(format!("tpt-audio-out:{}", device.id.0))
            .spawn(move || {
                let mut buffer = AudioBuffer::new(config.buffer_size, config.channels);
                let mut writer = writer;
                while thread_control.running.load(Ordering::SeqCst) {
                    callback(&mut buffer);
                    if let Err(e) = writer.write(&buffer) {
                        thread_control.push_error(e);
                        break;
                    }
                }
                thread_control.running.store(false, Ordering::SeqCst);
            })
            .map_err(|e| AudioError::Backend(format!("failed to spawn output driver: {e}")))?;

        Ok(StreamHandle {
            control,
            thread: Some(thread),
        })
    }
}

/// An active input (capture) stream.
pub struct InputStream;

impl InputStream {
    /// Opens `device` for capture and starts the driver thread.
    ///
    /// `callback` is invoked once per captured buffer. Implementations must
    /// not block; copy the data out and return.
    pub fn start(
        device: &AudioDevice,
        config: StreamConfig,
        reader: Box<dyn DeviceReader>,
        mut callback: InputCallback,
    ) -> Result<StreamHandle, AudioError> {
        let control = Arc::new(StreamControl::new());
        control.running.store(true, Ordering::SeqCst);

        let thread_control = Arc::clone(&control);
        let thread = thread::Builder::new()
            .name(format!("tpt-audio-in:{}", device.id.0))
            .spawn(move || {
                let mut buffer = AudioBuffer::new(config.buffer_size, config.channels);
                let mut reader = reader;
                while thread_control.running.load(Ordering::SeqCst) {
                    match reader.read(&mut buffer) {
                        Ok(()) => callback(&buffer),
                        Err(e) => {
                            thread_control.push_error(e);
                            break;
                        }
                    }
                }
                thread_control.running.store(false, Ordering::SeqCst);
            })
            .map_err(|e| AudioError::Backend(format!("failed to spawn input driver: {e}")))?;

        Ok(StreamHandle {
            control,
            thread: Some(thread),
        })
    }
}

/// Pacing helper for backends without a blocking device API (the null
/// backend): sleeps one buffer's worth of wall time.
pub(crate) fn pace_by_buffer_duration(config: StreamConfig) {
    let micros = (config.buffer_size as f64 / config.sample_rate as f64 * 1_000_000.0) as u64;
    thread::sleep(Duration::from_micros(micros.max(1_000)));
}
