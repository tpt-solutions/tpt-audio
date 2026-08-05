//! Archon audio backend — **research-gated**.
//!
//! This crate is a scaffold for the `tpt-archon` integration described in
//! `todo.md` (Phase 3). The `tpt-archon` audio server API (and the
//! `tpt-archon-bridge` zero-copy IPC) is not yet published/available, so this
//! backend cannot be fully implemented. The pieces that *can* be designed today
//! — the `AUDIO_CAPTURE` capability request/grant model and the `AudioBackend`
//! surface — are provided here so the GUI and core engine can be wired up ahead
//! of the real transport.
//!
//! Everything that would require the live server returns
//! [`ArchonError::NotImplemented`] and is recorded in diagnostics.

use std::collections::HashMap;
use std::sync::Arc;

use tpt_audio_core::backend::AudioBackend;
use tpt_audio_core::diagnostics::Diagnostics;
use tpt_audio_core::graph::{AudioDevice, AudioSink, AudioSource, Route, RouterConfig};

/// Capability tokens understood by the Archon microkernel audio server.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Capability {
    /// Permission for an app to *capture* system audio (microphone or loopback).
    AudioCapture,
    /// Permission to render audio to a system output.
    AudioRender,
}

/// Outcome of a capability request made to the Archon security broker.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CapabilityGrant {
    /// The user granted the capability.
    Granted(Capability),
    /// The user denied the capability.
    Denied(Capability),
    /// The broker has not yet responded (awaiting user decision).
    Pending(Capability),
}

/// Errors returned by the Archon backend while the transport is unavailable.
#[derive(Debug)]
pub enum ArchonError {
    NotImplemented(&'static str),
    TransportUnavailable,
}

impl std::fmt::Display for ArchonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ArchonError::NotImplemented(what) => {
                write!(
                    f,
                    "Archon backend: '{}' is not implemented (research-gated)",
                    what
                )
            }
            ArchonError::TransportUnavailable => {
                write!(f, "Archon backend: audio server transport unavailable")
            }
        }
    }
}

impl std::error::Error for ArchonError {}

pub struct ArchonBackend {
    routes: Vec<Route>,
    app_volumes: HashMap<String, f32>,
    grants: HashMap<Capability, CapabilityGrant>,
    diagnostics: Arc<Diagnostics>,
}

impl ArchonBackend {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let backend = Self {
            routes: Vec::new(),
            app_volumes: HashMap::new(),
            grants: HashMap::new(),
            diagnostics: Arc::new(Diagnostics::new(1000)),
        };
        backend
            .diagnostics
            .warn("Archon backend initialised in research-gated stub mode".to_string());
        Ok(backend)
    }

    pub fn diagnostics(&self) -> Arc<Diagnostics> {
        self.diagnostics.clone()
    }

    /// Request the `AUDIO_CAPTURE` capability from the Archon security broker.
    ///
    /// In the real backend this opens a request over `tpt-archon-bridge` and
    /// surfaces a user prompt; here it records the pending request and returns
    /// [`CapabilityGrant::Pending`] until the transport exists.
    pub fn request_audio_capture(&mut self) -> CapabilityGrant {
        let cap = Capability::AudioCapture;
        let grant = CapabilityGrant::Pending(cap);
        self.grants.insert(cap, grant.clone());
        self.diagnostics
            .warn("AUDIO_CAPTURE requested (research-gated: no broker available)".to_string());
        grant
    }

    /// Whether the given capability has been granted by the user.
    pub fn is_granted(&self, cap: Capability) -> bool {
        matches!(self.grants.get(&cap), Some(CapabilityGrant::Granted(_)))
    }
}

impl Default for ArchonBackend {
    fn default() -> Self {
        Self {
            routes: Vec::new(),
            app_volumes: HashMap::new(),
            grants: HashMap::new(),
            diagnostics: Arc::new(Diagnostics::new(1000)),
        }
    }
}

impl AudioBackend for ArchonBackend {
    fn enumerate_devices(&self) -> Vec<AudioDevice> {
        Vec::new()
    }

    fn enumerate_applications(&self) -> Vec<AudioSource> {
        Vec::new()
    }

    fn create_route(&mut self, source: AudioSource, sink: AudioSink) -> Route {
        let id = self.routes.len() as u64 + 1;
        let route = Route {
            id,
            source,
            sink,
            gain: 1.0,
            muted: false,
            connected: false,
        };
        self.diagnostics.warn(format!(
            "Route {} creation is a no-op in the research-gated stub",
            id
        ));
        self.routes.push(route.clone());
        route
    }

    fn remove_route(&mut self, route_id: u64) {
        self.routes.retain(|r| r.id != route_id);
    }

    fn set_route_gain(&mut self, _route_id: u64, _gain: f32) {}

    fn set_route_mute(&mut self, _route_id: u64, _muted: bool) {}

    fn set_app_volume(&mut self, app_name: &str, volume: f32) {
        self.app_volumes
            .insert(app_name.to_string(), volume.clamp(0.0, 1.0));
    }

    fn apply_config(&mut self, config: &RouterConfig) {
        self.routes = config.routes.clone();
        self.app_volumes = config.app_volumes.clone();
    }

    fn current_config(&self) -> RouterConfig {
        let mut config = RouterConfig::new();
        config.routes = self.routes.clone();
        config.app_volumes = self.app_volumes.clone();
        config
    }

    fn start_streams(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        Err(Box::new(ArchonError::NotImplemented("start_streams")))
    }

    fn stop_streams(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        Err(Box::new(ArchonError::NotImplemented("stop_streams")))
    }
}
