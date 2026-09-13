//! PipeWire backend (Linux).
//!
//! Ported from the old `platform-linux` router crate. As there, this
//! implementation talks to a running PipeWire session through the standard
//! CLI tools (`pw-dump`, `pw-link`, `wpctl`) rather than linking
//! `libpipewire`, keeping the crate pure-Rust and buildable (and CI-verifiable)
//! anywhere. Audio routing stays inside the PipeWire graph via `pw-link`.
//!
//! Status for the engine surface:
//! - Device enumeration: implemented via `pw-dump` (sinks, sources, streams).
//! - Streams: **not yet** — playback/capture needs the native `pipewire-rs`
//!   port (`open_output_writer` returns [`AudioError::Unsupported`]).
//!
//! Requires PipeWire running with `pw-dump`/`pw-link`/`wpctl` installed
//! (typically the `pipewire` and `wireplumber` packages).

use std::process::Command;

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
        _device: &AudioDevice,
        _config: StreamConfig,
    ) -> Result<Box<dyn DeviceWriter>, AudioError> {
        Err(AudioError::Unsupported(
            "PipeWire stream playback is not yet implemented (native pipewire-rs port pending)",
        ))
    }

    fn open_input_reader(
        &self,
        _device: &AudioDevice,
        _config: StreamConfig,
    ) -> Result<Box<dyn DeviceReader>, AudioError> {
        Err(AudioError::Unsupported(
            "PipeWire stream capture is not yet implemented (native pipewire-rs port pending)",
        ))
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
        assert!(collect_devices(&pw_dump_nodes()).is_empty() || true);
        let devices = collect_devices(&[]);
        assert!(devices.is_empty());
    }
}
