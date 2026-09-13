pub mod device;
pub mod pipeline;
pub mod session;
pub mod stream;

use std::collections::HashMap;
use std::sync::Arc;

use tpt_audio_core::backend::AudioBackend;
use tpt_audio_core::diagnostics::Diagnostics;
use tpt_audio_core::graph::{AudioDevice, AudioSink, AudioSource, Route, RouterConfig};
use windows::Win32::System::Com::*;

pub struct WasapiBackend {
    devices: Vec<AudioDevice>,
    app_sources: Vec<AudioSource>,
    routes: Vec<Route>,
    app_volumes: HashMap<String, f32>,
    pipeline: pipeline::AudioPipeline,
    diagnostics: Arc<Diagnostics>,
    initialized: bool,
}

impl WasapiBackend {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }

        let diagnostics = Arc::new(Diagnostics::new(1000));

        let mut backend = Self {
            devices: Vec::new(),
            app_sources: Vec::new(),
            routes: Vec::new(),
            app_volumes: HashMap::new(),
            pipeline: pipeline::AudioPipeline::new(diagnostics.clone()),
            diagnostics,
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
        let old_count = self.devices.len();
        self.devices = device::enumerate_all()?;
        self.diagnostics.set_last_refresh();

        if self.devices.len() > old_count {
            self.diagnostics.record_device_found();
            self.diagnostics.info(format!(
                "Devices added: {} → {}",
                old_count,
                self.devices.len()
            ));
        } else if self.devices.len() < old_count {
            self.diagnostics.record_device_lost();
            self.diagnostics.warn(format!(
                "Devices removed: {} → {}",
                old_count,
                self.devices.len()
            ));
        }
        Ok(())
    }

    pub fn refresh_app_sources(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.app_sources = session::enumerate_audio_sessions()?;
        Ok(())
    }

    fn validate_routes(&mut self) {
        let active_ids: Vec<String> = self.devices.iter().map(|d| d.id.clone()).collect();

        for route in self.routes.iter_mut() {
            let source_ok = active_ids.contains(&route.source.device_id)
                || route.source.device_id.starts_with("app::");
            let sink_ok = active_ids.contains(&route.sink.device_id);
            route.connected = source_ok && sink_ok;
        }

        let disconnected: Vec<Route> = self
            .routes
            .iter()
            .filter(|r| !r.connected)
            .cloned()
            .collect();
        for d in &disconnected {
            self.diagnostics
                .warn(format!("Route {} disconnected (device missing)", d.id));
        }
    }
}

impl AudioBackend for WasapiBackend {
    fn enumerate_devices(&self) -> Vec<AudioDevice> {
        self.devices.clone()
    }

    fn enumerate_applications(&self) -> Vec<AudioSource> {
        self.app_sources.clone()
    }

    fn create_route(&mut self, source: AudioSource, sink: AudioSink) -> Route {
        let id = self.routes.len() as u64 + 1;
        let connected = {
            let active_ids: Vec<String> = self.devices.iter().map(|d| d.id.clone()).collect();
            (active_ids.contains(&source.device_id) || source.device_id.starts_with("app::"))
                && active_ids.contains(&sink.device_id)
        };
        let route = Route {
            id,
            source,
            sink,
            gain: 1.0,
            muted: false,
            connected,
        };
        self.diagnostics.record_route_created();
        self.diagnostics
            .info(format!("Route {} created (connected: {})", id, connected));
        self.routes.push(route.clone());
        self.pipeline.update_routes(self.routes.clone());
        route
    }

    fn remove_route(&mut self, route_id: u64) {
        self.routes.retain(|r| r.id != route_id);
        self.diagnostics.record_route_removed();
        self.diagnostics.info(format!("Route {} removed", route_id));
        self.pipeline.update_routes(self.routes.clone());
    }

    fn set_route_gain(&mut self, route_id: u64, gain: f32) {
        if let Some(route) = self.routes.iter_mut().find(|r| r.id == route_id) {
            route.gain = gain.clamp(0.0, 1.0);
            self.pipeline.update_routes(self.routes.clone());
        }
    }

    fn set_route_mute(&mut self, route_id: u64, muted: bool) {
        if let Some(route) = self.routes.iter_mut().find(|r| r.id == route_id) {
            route.muted = muted;
            self.pipeline.update_routes(self.routes.clone());
        }
    }

    fn set_app_volume(&mut self, app_name: &str, volume: f32) {
        let clamped = volume.clamp(0.0, 1.0);
        self.app_volumes.insert(app_name.to_string(), clamped);

        if let Some(pid_str) = app_name.strip_prefix("app::") {
            if let Ok(pid) = pid_str.parse::<u32>() {
                if let Err(e) = session::set_session_volume(pid, clamped) {
                    self.diagnostics
                        .warn(format!("Failed to set volume for PID {}: {}", pid, e));
                }
            }
        }
    }

    fn apply_config(&mut self, config: &RouterConfig) {
        self.routes = config.routes.clone();
        self.app_volumes = config.app_volumes.clone();
        self.validate_routes();
        self.pipeline.update_routes(self.routes.clone());
        self.diagnostics.info("Config applied".to_string());
    }

    fn current_config(&self) -> RouterConfig {
        let mut config = RouterConfig::new();
        config.routes = self.routes.clone();
        config.app_volumes = self.app_volumes.clone();
        config
    }

    fn start_streams(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let source_ids: Vec<String> = self
            .routes
            .iter()
            .filter(|r| r.connected && !r.source.device_id.starts_with("app::"))
            .map(|r| r.source.device_id.clone())
            .collect();
        let sink_ids: Vec<String> = self
            .routes
            .iter()
            .filter(|r| r.connected)
            .map(|r| r.sink.device_id.clone())
            .collect();

        self.pipeline
            .start(&source_ids, &sink_ids, self.routes.clone())?;
        self.diagnostics.info("Streams started".to_string());
        Ok(())
    }

    fn stop_streams(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.pipeline.stop();
        self.diagnostics.info("Streams stopped".to_string());
        Ok(())
    }
}

impl Drop for WasapiBackend {
    fn drop(&mut self) {
        self.pipeline.stop();
        if self.initialized {
            unsafe {
                CoUninitialize();
            }
        }
    }
}
