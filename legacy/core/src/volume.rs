use std::collections::HashMap;

#[derive(Clone, Debug)]
pub struct VolumeController {
    app_volumes: HashMap<String, f32>,
    master_volume: f32,
}

impl VolumeController {
    pub fn new() -> Self {
        Self {
            app_volumes: HashMap::new(),
            master_volume: 1.0,
        }
    }

    pub fn set_master_volume(&mut self, volume: f32) {
        self.master_volume = volume.clamp(0.0, 1.0);
    }

    pub fn master_volume(&self) -> f32 {
        self.master_volume
    }

    pub fn set_app_volume(&mut self, app_name: &str, volume: f32) {
        self.app_volumes
            .insert(app_name.to_string(), volume.clamp(0.0, 1.0));
    }

    pub fn app_volume(&self, app_name: &str) -> f32 {
        self.app_volumes
            .get(app_name)
            .copied()
            .unwrap_or(self.master_volume)
    }

    pub fn remove_app(&mut self, app_name: &str) {
        self.app_volumes.remove(app_name);
    }

    pub fn app_volumes(&self) -> &HashMap<String, f32> {
        &self.app_volumes
    }

    pub fn effective_gain(&self, app_name: Option<&str>) -> f32 {
        let app_gain = app_name
            .and_then(|name| self.app_volumes.get(name))
            .copied()
            .unwrap_or(1.0);
        app_gain * self.master_volume
    }

    pub fn apply_route_gain(&self, sample: f32, route_gain: f32, app_name: Option<&str>) -> f32 {
        let effective = self.effective_gain(app_name);
        sample * route_gain * effective
    }
}

impl Default for VolumeController {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_master_volume() {
        let mut vc = VolumeController::new();
        assert_eq!(vc.master_volume(), 1.0);
        vc.set_master_volume(0.5);
        assert_eq!(vc.master_volume(), 0.5);
    }

    #[test]
    fn test_app_volume() {
        let mut vc = VolumeController::new();
        vc.set_app_volume("chrome", 0.3);
        assert_eq!(vc.app_volume("chrome"), 0.3);
        assert_eq!(vc.app_volume("unknown"), 1.0);
    }

    #[test]
    fn test_effective_gain() {
        let mut vc = VolumeController::new();
        vc.set_master_volume(0.5);
        vc.set_app_volume("discord", 0.8);
        let eff = vc.effective_gain(Some("discord"));
        assert!((eff - 0.4).abs() < 1e-6);
    }

    #[test]
    fn test_clamping() {
        let mut vc = VolumeController::new();
        vc.set_app_volume("test", 1.5);
        assert_eq!(vc.app_volume("test"), 1.0);
        vc.set_app_volume("test", -0.1);
        assert_eq!(vc.app_volume("test"), 0.0);
    }
}
