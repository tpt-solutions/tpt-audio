use crate::graph::{AudioDevice, AudioSink, AudioSource, Route, RouterConfig};

pub trait AudioBackend: Send {
    fn enumerate_devices(&self) -> Vec<AudioDevice>;

    fn enumerate_applications(&self) -> Vec<AudioSource>;

    fn create_route(&mut self, source: AudioSource, sink: AudioSink) -> Route;

    fn remove_route(&mut self, route_id: u64);

    fn set_route_gain(&mut self, route_id: u64, gain: f32);

    fn set_route_mute(&mut self, route_id: u64, muted: bool);

    fn set_app_volume(&mut self, app_name: &str, volume: f32);

    fn apply_config(&mut self, config: &RouterConfig);

    fn current_config(&self) -> RouterConfig;

    fn start_streams(&mut self) -> Result<(), Box<dyn std::error::Error>>;

    fn stop_streams(&mut self) -> Result<(), Box<dyn std::error::Error>>;
}
