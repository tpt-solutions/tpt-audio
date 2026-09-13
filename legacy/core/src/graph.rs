use std::collections::HashMap;

use serde::{Deserialize, Serialize};

pub type DeviceId = String;
pub type RouteId = u64;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum AudioDirection {
    Input,
    Output,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AudioDevice {
    pub id: DeviceId,
    pub name: String,
    pub direction: AudioDirection,
    pub channels: u16,
    pub is_default: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AudioSource {
    pub device_id: DeviceId,
    pub app_name: Option<String>,
    pub app_pid: Option<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AudioSink {
    pub device_id: DeviceId,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Route {
    pub id: RouteId,
    pub source: AudioSource,
    pub sink: AudioSink,
    pub gain: f32,
    pub muted: bool,
    #[serde(default = "default_connected")]
    pub connected: bool,
}

fn default_connected() -> bool {
    true
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RouterConfig {
    pub routes: Vec<Route>,
    pub app_volumes: HashMap<String, f32>,
    pub default_gain: f32,
}

impl RouterConfig {
    pub fn new() -> Self {
        Self {
            routes: Vec::new(),
            app_volumes: HashMap::new(),
            default_gain: 1.0,
        }
    }
}

impl Default for RouterConfig {
    fn default() -> Self {
        Self::new()
    }
}
