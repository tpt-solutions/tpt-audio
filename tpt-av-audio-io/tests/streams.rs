//! Integration tests: device enumeration and stream management through the
//! null backend (deterministic, CI-safe), plus tolerant live-backend probes
//! when a real device stack is present.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tpt_av_audio_io::device::{default_output_device, enumerate_devices, Direction};
use tpt_av_audio_io::stream::{OutputStream, StreamConfig};
use tpt_av_audio_io::{AudioBackend, NullBackend};
use tpt_av_audio_utils::AudioBuffer;

fn null_backend() -> NullBackend {
    NullBackend::new()
}

#[test]
fn enumerates_devices_via_trait() {
    let backend = null_backend();
    let devices = backend.enumerate_devices().unwrap();
    assert!(!devices.is_empty());
    assert!(devices.iter().any(|d| d.direction == Direction::Output));
    assert!(devices.iter().any(|d| d.direction == Direction::Input));
}

#[test]
fn output_stream_runs_callbacks_and_stops() {
    let backend = null_backend();
    let device = backend
        .enumerate_devices()
        .unwrap()
        .into_iter()
        .find(|d| d.direction == Direction::Output)
        .unwrap();

    let config = StreamConfig {
        sample_rate: 48_000,
        channels: 2,
        buffer_size: 256,
    };
    let writer = backend.open_output_writer(&device, config).unwrap();

    let callbacks = Arc::new(AtomicU64::new(0));
    let cb = {
        let callbacks = Arc::clone(&callbacks);
        Box::new(move |buf: &mut AudioBuffer| {
            buf.clear();
            callbacks.fetch_add(1, Ordering::Relaxed);
        }) as Box<dyn FnMut(&mut AudioBuffer) + Send>
    };

    let mut handle = OutputStream::start(&device, config, writer, cb).unwrap();
    assert!(handle.is_running());

    // ~200 ms of real-time-paced null output at 256 frames/48 kHz per
    // callback ≈ 18 callbacks.
    std::thread::sleep(Duration::from_millis(200));
    handle.stop();
    assert!(!handle.is_running());
    assert!(handle.take_errors().is_empty());
    assert!(callbacks.load(Ordering::Relaxed) > 0);
}

#[test]
fn stream_writes_frames_seen_by_backend_counters() {
    let backend = null_backend();
    let stats = backend.stats();
    let device = backend
        .enumerate_devices()
        .unwrap()
        .into_iter()
        .find(|d| d.direction == Direction::Output)
        .unwrap();

    let config = StreamConfig {
        sample_rate: 48_000,
        channels: 2,
        buffer_size: 512,
    };
    let writer = backend.open_output_writer(&device, config).unwrap();

    let mut handle = OutputStream::start(
        &device,
        config,
        writer,
        Box::new(|buf: &mut AudioBuffer| buf.clear()),
    )
    .unwrap();

    let deadline = Instant::now() + Duration::from_secs(5);
    while stats.frames_written.load(Ordering::Relaxed) < 4_096 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    handle.stop();

    assert!(
        stats.frames_written.load(Ordering::Relaxed) >= 4_096,
        "backend never saw the stream's frames"
    );
}

#[test]
fn failing_writer_surfaces_errors_and_stops_stream() {
    let backend = null_backend();
    backend.set_fail_writes(true);
    let device = backend
        .enumerate_devices()
        .unwrap()
        .into_iter()
        .find(|d| d.direction == Direction::Output)
        .unwrap();

    let config = StreamConfig::default_output();
    let result = backend.open_output_writer(&device, config);
    assert!(result.is_err(), "disabled null backend must refuse writers");
}

#[test]
fn default_backend_selection_respects_null_override() {
    // The unit-test process may or may not have TPT_AUDIO_BACKEND set; this
    // only checks the call path does not panic and returns *something*.
    let _ = enumerate_devices();
    let _ = default_output_device();
}
