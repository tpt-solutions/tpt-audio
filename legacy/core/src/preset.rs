use serde::{Deserialize, Serialize};

use crate::graph::RouterConfig;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Preset {
    pub name: String,
    pub config: RouterConfig,
}

pub struct PresetManager {
    presets: Vec<Preset>,
}

impl PresetManager {
    pub fn new() -> Self {
        Self {
            presets: Vec::new(),
        }
    }

    pub fn add_preset(&mut self, name: &str, config: RouterConfig) {
        if let Some(existing) = self.presets.iter_mut().find(|p| p.name == name) {
            existing.config = config;
        } else {
            self.presets.push(Preset {
                name: name.to_string(),
                config,
            });
        }
    }

    pub fn remove_preset(&mut self, name: &str) {
        self.presets.retain(|p| p.name != name);
    }

    pub fn get_preset(&self, name: &str) -> Option<&Preset> {
        self.presets.iter().find(|p| p.name == name)
    }

    pub fn presets(&self) -> &[Preset] {
        &self.presets
    }

    pub fn rename_preset(&mut self, old_name: &str, new_name: &str) {
        if let Some(preset) = self.presets.iter_mut().find(|p| p.name == old_name) {
            preset.name = new_name.to_string();
        }
    }
}

impl Default for PresetManager {
    fn default() -> Self {
        Self::new()
    }
}

impl PresetManager {
    pub fn save_to_json(&self, path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
        let json = serde_json::to_string_pretty(&self.presets)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    pub fn load_from_json(
        &mut self,
        path: &std::path::Path,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let json = std::fs::read_to_string(path)?;
        let presets: Vec<Preset> = serde_json::from_str(&json)?;
        self.presets = presets;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_get_preset() {
        let mut pm = PresetManager::new();
        pm.add_preset("gaming", RouterConfig::new());
        assert!(pm.get_preset("gaming").is_some());
        assert_eq!(pm.presets().len(), 1);
    }

    #[test]
    fn test_remove_preset() {
        let mut pm = PresetManager::new();
        pm.add_preset("gaming", RouterConfig::new());
        pm.remove_preset("gaming");
        assert!(pm.get_preset("gaming").is_none());
        assert_eq!(pm.presets().len(), 0);
    }

    #[test]
    fn test_update_existing() {
        let mut pm = PresetManager::new();
        pm.add_preset("gaming", RouterConfig::new());
        pm.add_preset("gaming", RouterConfig::new());
        assert_eq!(pm.presets().len(), 1);
    }
}
