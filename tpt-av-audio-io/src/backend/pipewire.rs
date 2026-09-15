//! PipeWire backend (Linux).
//!
//! Ported from the old `platform-linux` router crate. As there, this
//! implementation talks to a running PipeWire session through the standard
//! CLI tools rather than linking `libpipewire`, keeping the crate pure-Rust
//! and buildable (and CI-verifiable) anywhere:
//!
//! - Device enumeration via `pw-dump` (sinks, sources, streams).
//! - Stream playback/capture via `pw-cat --playback`/`--record` with raw
//!   interleaved f32 LE samples on stdio ([`PwCatWriter`]/[`PwCatReader`]).
//!   The stream driver thread writes/reads the child's pipe and is paced by
//!   PipeWire's own buffering. If `pw-cat` is missing, stream creation
//!   returns [`AudioError::Unsupported`] with a hint.
//! - Graph routing stays available through `pw-link`/`wpctl`.
//!
//! The `pw-cat` path adds one userspace hop (engine → pipe → PipeWire); a
//! native `pipewire-rs` port with zero-copy stream control remains the
//! long-term upgrade.
//!
//! Requires PipeWire running with `pw-dump` and `pw-cat` installed
//! (typically the `pipewire` and `pipewire-audio` packages).

use std::io::{Read, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::Value;

use crate::device::{AudioDevice, Direction};
use crate::stream::{DeviceReader, DeviceWriter, StreamConfig};
use tpt_av_audio_utils::AudioError;

/// Linux PipeWire backend.
pub struct PipewireBackend;

impl PipewireBackend {
    /// Creates the backend.
    pub fn new() -> Result<Self, AudioError> {
        Ok(Self)
    }
}

impl crate::backend::AudioBackend for PipewireBackend {
    fn name(&self) -> &'static str {
        "pipewire"
    }

    fn enumerate_devices(&self) -> Result<Vec<AudioDevice>, AudioError> {
        Ok(collect_devices(&pw_dump_nodes()))
    }

    fn open_output_writer(
        &self,
        device: &AudioDevice,
        config: StreamConfig,
    ) -> Result<Box<dyn DeviceWriter>, AudioError> {
        PwCatWriter::spawn(Some(&device.id.0), config)
    }

    fn open_input_reader(
        &self,
        device: &AudioDevice,
        config: StreamConfig,
    ) -> Result<Box<dyn DeviceReader>, AudioError> {
        let target = if device.direction == Direction::Output {
            // Capturing an output device = monitor of the sink.
            Some(device.id.0.as_str())
        } else {
            None
        };
        PwCatReader::spawn(target, config)
    }
}

/// Maps `pw-dump` Node objects into engine devices.
fn collect_devices(nodes: &[Value]) -> Vec<AudioDevice> {
    let mut devices = Vec::new();
    for node in nodes {
        let id = node.get("id").and_then(|v| v.as_u64());
        let props = node.get("props");
        let media_class = props
            .and_then(|p| p.get("media.class"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let Some(id) = id else { continue };
        if id == 0 || media_class.is_empty() {
            continue;
        }

        let name = props
            .and_then(|p| p.get("node.description"))
            .and_then(|v| v.as_str())
            .or_else(|| {
                props
                    .and_then(|p| p.get("node.name"))
                    .and_then(|v| v.as_str())
            })
            .unwrap_or("unknown")
            .to_string();

        let direction = match media_class {
            "Audio/Sink" => Direction::Output,
            "Audio/Source" => Direction::Input,
            _ => continue,
        };
        devices.push(AudioDevice::simple(
            id.to_string(),
            name,
            direction,
            2,
            false,
        ));
    }
    devices
}

/// Runs `pw-dump` and returns the Node entries, tolerating missing tools.
fn pw_dump_nodes() -> Vec<Value> {
    let try_dump = |args: &[&str]| -> Option<Vec<Value>> {
        let output = Command::new("pw-dump").args(args).output().ok()?;
        if !output.status.success() {
            return None;
        }
        serde_json::from_slice(&output.stdout).ok()
    };

    let all = try_dump(&["Node"]).or_else(|| try_dump(&[]));
    match all {
        Some(value) => value
            .into_iter()
            .filter(|v| {
                v.get("type")
                    .and_then(|t| t.as_str())
                    .is_some_and(|t| t.ends_with(":Node"))
            })
            .collect(),
        None => Vec::new(),
    }
}

/// Formats the `pw-cat` raw-mode arguments for `config`.
fn pw_cat_args(config: StreamConfig, record: bool) -> Vec<String> {
    vec![
        (if record { "--record" } else { "--playback" }).into(),
        "--raw".into(),
        "--format=f32".into(),
        "--rate".into(),
        config.sample_rate.to_string(),
        "--channels".into(),
        config.channels.to_string(),
        "--latency".into(),
        format!(
            "{}ms",
            config.buffer_size as u64 * 1_000 / config.sample_rate.max(1) as u64
        ),
    ]
}

/// Resolves the `pw-cat` binary, tolerating a missing install.
fn pw_cat_available() -> bool {
    Command::new("pw-cat")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Playback sink feeding interleaved f32 LE samples to `pw-cat --playback`
/// over stdin. The child's buffering paces the writer, so [`DeviceWriter::write`]
/// blocks when PipeWire has not consumed audio yet.
pub struct PwCatWriter {
    child: Child,
    stdin: Option<ChildStdin>,
}

impl PwCatWriter {
    /// Spawns `pw-cat --playback` targeting `target` (a PipeWire node id),
    /// or the default sink when `None`.
    pub fn spawn(target: Option<&str>, config: StreamConfig) -> Result<Self, AudioError> {
        if !pw_cat_available() {
            return Err(AudioError::Unsupported(
                "pw-cat not found on PATH; install the pipewire-audio tools".into(),
            ));
        }

        let mut args = pw_cat_args(config, false);
        if let Some(t) = target {
            args.push("--target".into());
            args.push(t.to_string());
        }

        let mut child = Command::new("pw-cat")
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| AudioError::Backend(format!("pw-cat spawn failed: {e}")))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| AudioError::Backend("pw-cat stdin unavailable".into()))?;
        Ok(Self {
            child,
            stdin: Some(stdin),
        })
    }
}

impl DeviceWriter for PwCatWriter {
    fn write(&mut self, buffer: &AudioBuffer) -> Result<(), AudioError> {
        // Interleaved f32 little-endian is our native layout already; the
        // copy into bytes happens on the (worker) driver thread.
        let bytes: Vec<u8> = buffer.data.iter().flat_map(|s| s.to_le_bytes()).collect();
        let Some(stdin) = self.stdin.as_mut() else {
            return Err(AudioError::Backend("pw-cat stdin closed".into()));
        };
        stdin
            .write_all(&bytes)
            .map_err(|e| AudioError::Backend(format!("pw-cat write failed: {e}")))?;
        stdin
            .flush()
            .map_err(|e| AudioError::Backend(format!("pw-cat flush failed: {e}")))?;
        Ok(())
    }
}

impl Drop for PwCatWriter {
    fn drop(&mut self) {
        // Dropping stdin signals EOF; give pw-cat a moment to drain, then
        // make sure it is gone.
        self.stdin.take();
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(200);
        while std::time::Instant::now() < deadline {
            match self.child.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) => std::thread::sleep(std::time::Duration::from_millis(10)),
                Err(_) => break,
            }
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Capture source reading interleaved f32 LE samples from
/// `pw-cat --record` stdout. Returns silence for any gap the producer has
/// not filled yet (device underruns never wedge the stream).
pub struct PwCatReader {
    child: Child,
    stdout: Option<ChildStdout>,
    bytes_per_frame: usize,
}

impl PwCatReader {
    /// Spawns `pw-cat --record`; `target` selects a specific source node id
    /// (or a sink id to capture its monitor), or `None` for the default
    /// microphone.
    pub fn spawn(target: Option<&str>, config: StreamConfig) -> Result<Self, AudioError> {
        if !pw_cat_available() {
            return Err(AudioError::Unsupported(
                "pw-cat not found on PATH; install the pipewire-audio tools".into(),
            ));
        }

        let mut args = pw_cat_args(config, true);
        if let Some(t) = target {
            args.push("--target".into());
            args.push(t.to_string());
        }

        let mut child = Command::new("pw-cat")
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| AudioError::Backend(format!("pw-cat spawn failed: {e}")))?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| AudioError::Backend("pw-cat stdout unavailable".into()))?;
        Ok(Self {
            child,
            stdout: Some(stdout),
            bytes_per_frame: config.channels as usize * 4,
        })
    }
}

impl DeviceReader for PwCatReader {
    fn read(&mut self, buffer: &mut AudioBuffer) -> Result<(), AudioError> {
        let want = buffer.frames * self.bytes_per_frame;
        let mut bytes = vec![0u8; want];
        let mut filled = 0usize;
        if let Some(stdout) = self.stdout.as_mut() {
            while filled < want {
                match stdout.read(&mut bytes[filled..]) {
                    Ok(0) => break, // pw-cat closed: pad the rest with silence
                    Ok(n) => filled += n,
                    Err(ref e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(e) => return Err(AudioError::Backend(format!("pw-cat read failed: {e}"))),
                }
            }
        }

        for (chunk, sample) in bytes[..filled].chunks_exact(4).zip(buffer.data.iter_mut()) {
            *sample = f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        }
        for sample in &mut buffer.data[filled / 4..] {
            *sample = 0.0;
        }
        Ok(())
    }
}

impl Drop for PwCatReader {
    fn drop(&mut self) {
        self.stdout.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn collects_sinks_and_sources() {
        let nodes = vec![
            json!({
                "id": 42,
                "type": "PipeWire:Interface:Node",
                "props": { "media.class": "Audio/Sink", "node.description": "Speakers" }
            }),
            json!({
                "id": 43,
                "type": "PipeWire:Interface:Node",
                "props": { "media.class": "Audio/Source", "node.name": "Mic" }
            }),
            json!({
                "id": 44,
                "type": "PipeWire:Interface:Node",
                "props": { "media.class": "Stream/Output/Audio", "application.name": "Player" }
            }),
            json!({
                "id": 45,
                "type": "PipeWire:Interface:Node",
                "props": {}
            }),
        ];
        let devices = collect_devices(&nodes);
        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].id.0, "42");
        assert_eq!(devices[0].direction, Direction::Output);
        assert_eq!(devices[1].direction, Direction::Input);
    }

    #[test]
    fn empty_or_missing_dump_is_empty_list() {
        let devices = collect_devices(&[]);
        assert!(devices.is_empty());
    }

    #[test]
    fn pw_cat_args_cover_format_rate_channels_latency() {
        let args = pw_cat_args(
            StreamConfig {
                sample_rate: 48_000,
                channels: 2,
                buffer_size: 480,
            },
            false,
        );
        assert!(args.contains(&"--playback".to_string()));
        assert!(args.contains(&"--raw".to_string()));
        assert!(args.contains(&"--format=f32".to_string()));
        assert!(args.iter().any(|a| a == "48000"));
        assert!(args.iter().any(|a| a == "2"));
        assert!(args.contains(&"10ms".to_string()));

        let record_args = pw_cat_args(
            StreamConfig {
                sample_rate: 44_100,
                channels: 1,
                buffer_size: 441,
            },
            true,
        );
        assert!(record_args.contains(&"--record".to_string()));
        assert!(record_args.contains(&"10ms".to_string()));
    }
}
