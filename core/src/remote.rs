//! Remote / headless control.
//!
//! Provides a tiny, dependency-free JSON command protocol over a backend so the
//! app can be driven without the GUI (CLI, scripts, or a future web API). Each
//! command is a JSON object with an `action` field; the response is a JSON
//! object printed back. [`dispatch`] handles a single command; [`run_stdin_server`]
//! reads newline-delimited commands from a reader until EOF.

use std::io::{BufRead, Write};

use crate::backend::AudioBackend;
use crate::graph::{AudioSink, AudioSource, RouterConfig};

/// Execute one JSON command against `backend`, returning a JSON response string.
pub fn dispatch(backend: &mut dyn AudioBackend, command: &str) -> String {
    let value: serde_json::Value = match serde_json::from_str(command) {
        Ok(v) => v,
        Err(e) => {
            return serde_json::json!({ "ok": false, "error": format!("invalid JSON: {e}") })
                .to_string()
        }
    };

    let action = value
        .get("action")
        .and_then(|a| a.as_str())
        .unwrap_or("");

    match action {
        "enumerate_devices" => {
            serde_json::json!({ "ok": true, "devices": backend.enumerate_devices() }).to_string()
        }
        "enumerate_applications" => serde_json::json!({
            "ok": true,
            "applications": backend.enumerate_applications()
        })
        .to_string(),
        "current_config" => {
            serde_json::json!({ "ok": true, "config": backend.current_config() }).to_string()
        }
        "set_app_volume" => match parse_app_volume(&value) {
            Ok((app, volume)) => {
                backend.set_app_volume(&app, volume);
                serde_json::json!({ "ok": true, "app": app, "volume": volume }).to_string()
            }
            Err(e) => err(e),
        },
        "create_route" => match parse_endpoint(&value) {
            Ok((source, sink)) => {
                let route = backend.create_route(source, sink);
                serde_json::json!({ "ok": true, "route": route }).to_string()
            }
            Err(e) => err(e),
        },
        "remove_route" => match value.get("id").and_then(|v| v.as_u64()) {
            Some(id) => {
                backend.remove_route(id);
                serde_json::json!({ "ok": true, "id": id }).to_string()
            }
            None => err("missing or invalid 'id' (u64)"),
        },
        "set_route_gain" => match parse_route_param(&value, "gain") {
            Ok((id, gain)) => {
                backend.set_route_gain(id, gain);
                serde_json::json!({ "ok": true, "id": id, "gain": gain }).to_string()
            }
            Err(e) => err(e),
        },
        "set_route_mute" => match parse_route_mute(&value) {
            Ok((id, muted)) => {
                backend.set_route_mute(id, muted);
                serde_json::json!({ "ok": true, "id": id, "muted": muted }).to_string()
            }
            Err(e) => err(e),
        },
        "apply_config" => match parse_config(&value) {
            Ok(config) => {
                backend.apply_config(&config);
                serde_json::json!({ "ok": true }).to_string()
            }
            Err(e) => err(e),
        },
        other => err(format!("unknown action: '{other}'")),
    }
}

/// Read newline-delimited JSON commands from `reader`, dispatching each to
/// `backend` and writing the JSON response to `writer`. Stops at EOF.
pub fn run_stdin_server<R, W>(
    backend: &mut dyn AudioBackend,
    reader: R,
    writer: &mut W,
) where
    R: BufRead,
    W: Write,
{
    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let response = dispatch(backend, trimmed);
        let _ = writeln!(writer, "{response}");
        let _ = writer.flush();
    }
}

fn err(msg: impl AsRef<str>) -> String {
    serde_json::json!({ "ok": false, "error": msg.as_ref() }).to_string()
}

fn parse_app_volume(value: &serde_json::Value) -> Result<(String, f32), String> {
    let app = value
        .get("app")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or("missing or invalid 'app' (string)")?;
    let volume = value
        .get("volume")
        .and_then(|v| v.as_f64())
        .map(|v| v as f32)
        .ok_or("missing or invalid 'volume' (number 0..1)")?;
    Ok((app, volume))
}

fn parse_endpoint(
    value: &serde_json::Value,
) -> Result<(AudioSource, AudioSink), String> {
    let source: AudioSource = serde_json::from_value(
        value
            .get("source")
            .cloned()
            .ok_or("missing 'source' object")?,
    )
    .map_err(|e| format!("invalid 'source': {e}"))?;
    let sink: AudioSink = serde_json::from_value(
        value.get("sink").cloned().ok_or("missing 'sink' object")?,
    )
    .map_err(|e| format!("invalid 'sink': {e}"))?;
    Ok((source, sink))
}

fn parse_route_param(value: &serde_json::Value, field: &str) -> Result<(u64, f32), String> {
    let id = value
        .get("id")
        .and_then(|v| v.as_u64())
        .ok_or("missing or invalid 'id' (u64)")?;
    let param = value
        .get(field)
        .and_then(|v| v.as_f64())
        .map(|v| v as f32)
        .ok_or_else(|| format!("missing or invalid '{field}' (number)"))?;
    Ok((id, param))
}

fn parse_route_mute(value: &serde_json::Value) -> Result<(u64, bool), String> {
    let id = value
        .get("id")
        .and_then(|v| v.as_u64())
        .ok_or("missing or invalid 'id' (u64)")?;
    let muted = value
        .get("muted")
        .and_then(|v| v.as_bool())
        .ok_or("missing or invalid 'muted' (bool)")?;
    Ok((id, muted))
}

fn parse_config(value: &serde_json::Value) -> Result<RouterConfig, String> {
    let config: RouterConfig = serde_json::from_value(
        value.get("config").cloned().ok_or("missing 'config' object")?,
    )
    .map_err(|e| format!("invalid 'config': {e}"))?;
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::AudioDevice;

    /// Minimal in-memory backend used to exercise command dispatch.
    struct StubBackend {
        applied: bool,
    }

    impl AudioBackend for StubBackend {
        fn enumerate_devices(&self) -> Vec<AudioDevice> {
            vec![]
        }
        fn enumerate_applications(&self) -> Vec<crate::graph::AudioSource> {
            vec![]
        }
        fn create_route(
            &mut self,
            _source: AudioSource,
            _sink: AudioSink,
        ) -> crate::graph::Route {
            crate::graph::Route {
                id: 1,
                source: AudioSource {
                    device_id: "s".into(),
                    app_name: None,
                    app_pid: None,
                },
                sink: AudioSink {
                    device_id: "k".into(),
                },
                gain: 1.0,
                muted: false,
                connected: true,
            }
        }
        fn remove_route(&mut self, _route_id: u64) {}
        fn set_route_gain(&mut self, _route_id: u64, _gain: f32) {}
        fn set_route_mute(&mut self, _route_id: u64, _muted: bool) {}
        fn set_app_volume(&mut self, _app_name: &str, _volume: f32) {}
        fn apply_config(&mut self, _config: &RouterConfig) {
            self.applied = true;
        }
        fn current_config(&self) -> RouterConfig {
            RouterConfig::new()
        }
        fn start_streams(&mut self) -> Result<(), Box<dyn std::error::Error>> {
            Ok(())
        }
        fn stop_streams(&mut self) -> Result<(), Box<dyn std::error::Error>> {
            Ok(())
        }
    }

    #[test]
    fn unknown_action_errors() {
        let mut b = StubBackend { applied: false };
        let r = dispatch(&mut b, r#"{"action":"bogus"}"#);
        assert!(r.contains("\"ok\":false"));
    }

    #[test]
    fn malformed_json_errors() {
        let mut b = StubBackend { applied: false };
        let r = dispatch(&mut b, "not json");
        assert!(r.contains("\"ok\":false"));
    }

    #[test]
    fn enumerate_devices_ok() {
        let mut b = StubBackend { applied: false };
        let r = dispatch(&mut b, r#"{"action":"enumerate_devices"}"#);
        assert!(r.contains("\"ok\":true"));
    }

    #[test]
    fn create_route_returns_route() {
        let mut b = StubBackend { applied: false };
        let r = dispatch(
            &mut b,
            r#"{"action":"create_route","source":{"device_id":"mic"},"sink":{"device_id":"spk"}}"#,
        );
        assert!(r.contains("\"ok\":true"));
        assert!(r.contains("\"id\":1"));
    }

    #[test]
    fn apply_config_ok() {
        let mut b = StubBackend { applied: false };
        let r = dispatch(&mut b, r#"{"action":"apply_config","config":{"routes":[],"app_volumes":{},"default_gain":1.0}}"#);
        assert!(r.contains("\"ok\":true"));
    }
}
