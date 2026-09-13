use std::sync::Arc;

use crate::graph::{Route, RouteId, RouterConfig};

pub const SAMPLE_RATE: u32 = 48000;
pub const CHANNELS: u16 = 2;
pub const FRAMES_PER_BUFFER: usize = 256;

pub struct MixingEngine {
    routes: Vec<Route>,
    next_route_id: RouteId,
}

impl MixingEngine {
    pub fn new() -> Self {
        Self {
            routes: Vec::new(),
            next_route_id: 1,
        }
    }

    pub fn add_route(&mut self, route: Route) {
        self.routes.push(route);
    }

    pub fn remove_route(&mut self, id: RouteId) {
        self.routes.retain(|r| r.id != id);
    }

    pub fn set_gain(&mut self, id: RouteId, gain: f32) {
        if let Some(route) = self.routes.iter_mut().find(|r| r.id == id) {
            route.gain = gain.clamp(0.0, 1.0);
        }
    }

    pub fn set_mute(&mut self, id: RouteId, muted: bool) {
        if let Some(route) = self.routes.iter_mut().find(|r| r.id == id) {
            route.muted = muted;
        }
    }

    pub fn routes(&self) -> &[Route] {
        &self.routes
    }

    pub fn next_id(&mut self) -> RouteId {
        let id = self.next_route_id;
        self.next_route_id += 1;
        id
    }

    pub fn apply_gains(
        &self,
        input_buffer: Arc<[f32]>,
        routes: &[Route],
    ) -> Vec<(RouteId, Vec<f32>)> {
        let mut outputs: Vec<(RouteId, Vec<f32>)> = Vec::with_capacity(routes.len());

        for route in routes {
            if route.muted {
                outputs.push((route.id, vec![0.0; input_buffer.len()]));
                continue;
            }
            let scaled: Vec<f32> = input_buffer.iter().map(|&s| s * route.gain).collect();
            outputs.push((route.id, scaled));
        }

        outputs
    }

    pub fn mix_outputs(
        &self,
        route_buffers: &[(RouteId, Vec<f32>)],
        sink_route_map: &[(String, Vec<RouteId>)],
    ) -> Vec<(String, Vec<f32>)> {
        let mut mixed: Vec<(String, Vec<f32>)> = Vec::new();

        for (sink_id, route_ids) in sink_route_map {
            let mut accumulated = vec![0.0f32; FRAMES_PER_BUFFER * CHANNELS as usize];
            for (rid, buf) in route_buffers {
                if route_ids.contains(rid) {
                    for (i, sample) in accumulated.iter_mut().enumerate() {
                        *sample += buf[i];
                    }
                }
            }
            mixed.push((sink_id.clone(), accumulated));
        }

        mixed
    }

    pub fn config(&self) -> RouterConfig {
        let mut config = RouterConfig::new();
        config.routes = self.routes.clone();
        config
    }
}

impl Default for MixingEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::AudioSource;

    fn dummy_source() -> AudioSource {
        AudioSource {
            device_id: "mic1".into(),
            app_name: None,
            app_pid: None,
        }
    }

    fn dummy_sink() -> crate::graph::AudioSink {
        crate::graph::AudioSink {
            device_id: "speakers".into(),
        }
    }

    #[test]
    fn test_add_remove_route() {
        let mut engine = MixingEngine::new();
        let id = engine.next_id();
        let route = Route {
            id,
            source: dummy_source(),
            sink: dummy_sink(),
            gain: 0.5,
            muted: false,
            connected: true,
        };
        engine.add_route(route);
        assert_eq!(engine.routes().len(), 1);
        engine.remove_route(id);
        assert_eq!(engine.routes().len(), 0);
    }

    #[test]
    fn test_set_gain_and_mute() {
        let mut engine = MixingEngine::new();
        let id = engine.next_id();
        let route = Route {
            id,
            source: dummy_source(),
            sink: dummy_sink(),
            gain: 0.5,
            muted: false,
            connected: true,
        };
        engine.add_route(route);

        engine.set_gain(id, 0.75);
        assert_eq!(engine.routes()[0].gain, 0.75);

        engine.set_mute(id, true);
        assert!(engine.routes()[0].muted);
    }

    #[test]
    fn test_gain_clamping() {
        let mut engine = MixingEngine::new();
        let id = engine.next_id();
        let route = Route {
            id,
            source: dummy_source(),
            sink: dummy_sink(),
            gain: 0.5,
            muted: false,
            connected: true,
        };
        engine.add_route(route);

        engine.set_gain(id, -0.1);
        assert_eq!(engine.routes()[0].gain, 0.0);

        engine.set_gain(id, 1.5);
        assert_eq!(engine.routes()[0].gain, 1.0);
    }

    #[test]
    fn test_apply_gains_muted() {
        let engine = MixingEngine::new();
        let buf: Arc<[f32]> = vec![1.0f32; 256].into();

        let routes = vec![Route {
            id: 1,
            source: dummy_source(),
            sink: dummy_sink(),
            gain: 1.0,
            muted: true,
            connected: true,
        }];

        let outputs = engine.apply_gains(buf, &routes);
        assert!(outputs[0].1.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn test_mix_outputs() {
        let engine = MixingEngine::new();
        let buf_len = FRAMES_PER_BUFFER * CHANNELS as usize;
        let route_bufs = vec![(1u64, vec![0.5f32; buf_len]), (2u64, vec![0.3f32; buf_len])];
        let sink_map = vec![("speakers".into(), vec![1u64, 2u64])];

        let mixed = engine.mix_outputs(&route_bufs, &sink_map);
        assert_eq!(mixed.len(), 1);
        assert_eq!(mixed[0].0, "speakers");
        assert!((mixed[0].1[0] - 0.8).abs() < 1e-6);
    }
}
