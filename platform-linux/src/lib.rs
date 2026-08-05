//! Linux audio backend backed by [PipeWire](https://pipewire.org).
//!
//! This implementation talks to a running PipeWire session through the standard
//! command-line tools (`pw-dump`, `pw-link`, `wpctl`) rather than linking the
//! native `libpipewire` library. This keeps the crate pure-Rust and buildable
//! (and verifiable in CI) on any platform, while still fulfilling the Phase 2
//! goal: a working routing matrix and per-app volume control over PipeWire.
//!
//! Routing is performed natively by PipeWire via graph links between node ids,
//! so audio stays inside the server (no extra copy through this process).
//!
//! At runtime the host must have PipeWire running with `pw-link`/`wpctl`
//! available (typically provided by the `pipewire` and `wireplumber` packages).
//!
//! Note: per-route *gain* is recorded but not applied by this backend, because a
//! PipeWire link is unity-gain. Per-app volume (via `wpctl`) is applied. A future
//! native `pipewire-rs` backend can apply per-route gain through a mixing proxy.

use std::collections::HashMap;
use std::process::Command;
use std::sync::Arc;

use tpt_audio_core::backend::AudioBackend;
use tpt_audio_core::diagnostics::Diagnostics;
use tpt_audio_core::graph::{
    AudioDevice, AudioDirection, AudioSink, AudioSource, Route, RouterConfig,
};

pub struct PipewireBackend {
    devices: Vec<AudioDevice>,
    app_sources: Vec<AudioSource>,
    routes: Vec<Route>,
    app_volumes: HashMap<String, f32>,
    node_ids: HashMap<String, u32>,
    app_node_ids: HashMap<String, u32>,
    diagnostics: Arc<Diagnostics>,
    initialized: bool,
}

impl PipewireBackend {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let mut backend = Self {
            devices: Vec::new(),
            app_sources: Vec::new(),
            routes: Vec::new(),
            app_volumes: HashMap::new(),
            node_ids: HashMap::new(),
            app_node_ids: HashMap::new(),
            diagnostics: Arc::new(Diagnostics::new(1000)),
            initialized: false,
        };

        backend.refresh_devices()?;
        backend.refresh_app_sources()?;
        backend.initialized = true;

        Ok(backend)
    }

    pub fn diagnostics(&self) -> Arc<Diagnostics> {
        self.diagnostics.clone()
    }

    pub fn refresh_devices(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let old = self.devices.len();
        self.load_state();
        self.diagnostics.set_last_refresh();
        if self.devices.len() > old {
            self.diagnostics.record_device_found();
        } else if self.devices.len() < old {
            self.diagnostics.record_device_lost();
        }
        Ok(())
    }

    pub fn refresh_app_sources(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.load_state();
        Ok(())
    }

    fn load_state(&mut self) {
        let nodes = pw_dump_nodes();
        self.devices.clear();
        self.app_sources.clear();
        self.node_ids.clear();
        self.app_node_ids.clear();

        for node in nodes {
            let id = match node.get("id").and_then(|v| v.as_u64()) {
                Some(v) if v > 0 => v as u32,
                _ => continue,
            };
            let props = node.get("props");
            let media_class = props
                .and_then(|p| p.get("media.class"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if media_class.is_empty() {
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

            let device_id = id.to_string();
            self.node_ids.insert(device_id.clone(), id);

            if media_class == "Audio/Sink" {
                self.devices.push(AudioDevice {
                    id: device_id,
                    name,
                    direction: AudioDirection::Output,
                    channels: 2,
                    is_default: false,
                });
            } else if media_class == "Audio/Source" {
                self.devices.push(AudioDevice {
                    id: device_id,
                    name,
                    direction: AudioDirection::Input,
                    channels: 2,
                    is_default: false,
                });
            } else if media_class.starts_with("Stream/Output/Audio")
                || media_class.starts_with("Stream/Input/Audio")
            {
                let app_name = props
                    .and_then(|p| p.get("application.name"))
                    .and_then(|v| v.as_str())
                    .or_else(|| {
                        props
                            .and_then(|p| p.get("media.name"))
                            .and_then(|v| v.as_str())
                    })
                    .unwrap_or(&name)
                    .to_string();
                self.app_sources.push(AudioSource {
                    device_id: device_id.clone(),
                    app_name: Some(app_name.clone()),
                    app_pid: Some(id),
                });
                self.app_node_ids.insert(app_name, id);
            }
        }
    }

    fn node_pair(&self, route: &Route) -> Option<(u32, u32)> {
        let out = *self.node_ids.get(&route.source.device_id)?;
        let in_ = *self.node_ids.get(&route.sink.device_id)?;
        Some((out, in_))
    }

    fn create_link(&self, route: &Route) {
        if let Some((out, in_)) = self.node_pair(route) {
            let _ = Command::new("pw-link")
                .arg(out.to_string())
                .arg(in_.to_string())
                .status();
            self.diagnostics
                .info(format!("PipeWire link created {} -> {}", out, in_));
        }
    }

    fn remove_link(&self, route: &Route) {
        if let Some((out, in_)) = self.node_pair(route) {
            let _ = Command::new("pw-link")
                .arg("-d")
                .arg(out.to_string())
                .arg(in_.to_string())
                .status();
            self.diagnostics
                .info(format!("PipeWire link removed {} -> {}", out, in_));
        }
    }

    fn set_node_volume(&self, node_id: u32, volume: f32) {
        let _ = Command::new("wpctl")
            .arg("set-volume")
            .arg(node_id.to_string())
            .arg(format!("{:.3}", volume.clamp(0.0, 1.0)))
            .status();
    }
}

impl Default for PipewireBackend {
    fn default() -> Self {
        Self {
            devices: Vec::new(),
            app_sources: Vec::new(),
            routes: Vec::new(),
            app_volumes: HashMap::new(),
            node_ids: HashMap::new(),
            app_node_ids: HashMap::new(),
            diagnostics: Arc::new(Diagnostics::new(1000)),
            initialized: false,
        }
    }
}

impl AudioBackend for PipewireBackend {
    fn enumerate_devices(&self) -> Vec<AudioDevice> {
        self.devices.clone()
    }

    fn enumerate_applications(&self) -> Vec<AudioSource> {
        self.app_sources.clone()
    }

    fn create_route(&mut self, source: AudioSource, sink: AudioSink) -> Route {
        let connected = self.node_ids.contains_key(&source.device_id)
            && self.node_ids.contains_key(&sink.device_id);
        let id = self.routes.len() as u64 + 1;
        let route = Route {
            id,
            source,
            sink,
            gain: 1.0,
            muted: false,
            connected,
        };

        if connected && !route.muted {
            self.create_link(&route);
        }

        self.diagnostics.record_route_created();
        self.diagnostics
            .info(format!("Route {} created (connected: {})", id, connected));
        self.routes.push(route.clone());
        route
    }

    fn remove_route(&mut self, route_id: u64) {
        if let Some(pos) = self.routes.iter().position(|r| r.id == route_id) {
            let route = self.routes.remove(pos);
            if route.connected {
                self.remove_link(&route);
            }
            self.diagnostics.record_route_removed();
            self.diagnostics.info(format!("Route {} removed", route_id));
        }
    }

    fn set_route_gain(&mut self, route_id: u64, gain: f32) {
        if let Some(route) = self.routes.iter_mut().find(|r| r.id == route_id) {
            route.gain = gain.clamp(0.0, 1.0);
            self.diagnostics.info(format!(
                "Route {} gain set to {:.2} (per-route gain is recorded but not applied by the PipeWire link backend)",
                route_id, route.gain
            ));
        }
    }

    fn set_route_mute(&mut self, route_id: u64, muted: bool) {
        let was_muted = self.routes.iter_mut().find(|r| r.id == route_id).map(|r| {
            let prev = r.muted;
            r.muted = muted;
            prev
        });
        let Some(was_muted) = was_muted else {
            return;
        };
        if was_muted == muted {
            return;
        }
        if let Some(route) = self.routes.iter().find(|r| r.id == route_id) {
            if route.connected {
                if muted {
                    self.remove_link(route);
                } else {
                    self.create_link(route);
                }
            }
        }
        self.diagnostics
            .info(format!("Route {} mute set to {}", route_id, muted));
    }

    fn set_app_volume(&mut self, app_name: &str, volume: f32) {
        let clamped = volume.clamp(0.0, 1.0);
        self.app_volumes.insert(app_name.to_string(), clamped);

        if let Some(&node_id) = self.app_node_ids.get(app_name) {
            self.set_node_volume(node_id, clamped);
        } else if let Some(node_id) = app_name
            .strip_prefix("app::")
            .and_then(|s| s.parse::<u32>().ok())
        {
            self.set_node_volume(node_id, clamped);
        } else {
            self.diagnostics.warn(format!(
                "No PipeWire node found for app '{}'; volume not applied",
                app_name
            ));
        }
    }

    fn apply_config(&mut self, config: &RouterConfig) {
        for route in &self.routes {
            if route.connected {
                self.remove_link(route);
            }
        }
        self.routes = config.routes.clone();
        self.app_volumes = config.app_volumes.clone();
        self.load_state();

        for route in &self.routes {
            let connected = self.node_ids.contains_key(&route.source.device_id)
                && self.node_ids.contains_key(&route.sink.device_id);
            let mut route = route.clone();
            route.connected = connected;
            if connected && !route.muted {
                self.create_link(&route);
            }
        }
        self.diagnostics.info("Config applied".to_string());
    }

    fn current_config(&self) -> RouterConfig {
        let mut config = RouterConfig::new();
        config.routes = self.routes.clone();
        config.app_volumes = self.app_volumes.clone();
        config
    }

    fn start_streams(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // Routing is performed natively by PipeWire via graph links created in
        // `create_route`; there is no mixing stream to start on this backend.
        self.diagnostics
            .info("Streams started (links already live)".to_string());
        Ok(())
    }

    fn stop_streams(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.diagnostics
            .info("Streams stopped (links persist until route removal)".to_string());
        Ok(())
    }
}

impl Drop for PipewireBackend {
    fn drop(&mut self) {
        for route in &self.routes {
            if route.connected {
                self.remove_link(route);
            }
        }
    }
}

fn pw_dump_nodes() -> Vec<serde_json::Value> {
    let try_dump = |args: &[&str]| -> Option<Vec<serde_json::Value>> {
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
                    .map(|t| t.ends_with(":Node"))
                    .unwrap_or(false)
            })
            .collect(),
        None => Vec::new(),
    }
}
