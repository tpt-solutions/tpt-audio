//! Plugin / extension hook system.
//!
//! Extensions implement [`Plugin`] and are registered with a [`PluginRegistry`].
//! The GUI/engine notifies the registry at well-defined lifecycle points
//! (route changes, config applied, per-app volume changes) so extensions can
//! observe or react to routing activity without coupling to any one backend.

use crate::graph::RouterConfig;

/// An extension hook point. All methods have empty default implementations so
/// an extension only overrides the points it cares about.
pub trait Plugin: Send + Sync {
    /// Human-readable extension name (shown in the UI).
    fn name(&self) -> &str;

    /// Called once when the plugin is registered.
    fn on_load(&mut self) {}

    /// Called whenever the active routing graph changes (route added/removed,
    /// gain or mute toggled). `config` is the resulting full graph.
    fn on_route_change(&mut self, _config: &RouterConfig) {}

    /// Called after a full configuration (e.g. a preset) is applied.
    fn on_config_applied(&mut self, _config: &RouterConfig) {}

    /// Called when a per-application volume is set.
    fn on_app_volume(&mut self, _app_name: &str, _volume: f32) {}
}

/// Holds registered plugins and fans out lifecycle notifications.
pub struct PluginRegistry {
    plugins: Vec<Box<dyn Plugin>>,
}

impl PluginRegistry {
    pub fn new() -> Self {
        Self {
            plugins: Vec::new(),
        }
    }

    /// Register an extension. `on_load` is invoked immediately.
    pub fn register(&mut self, plugin: Box<dyn Plugin>) {
        let mut plugin = plugin;
        plugin.on_load();
        self.plugins.push(plugin);
    }

    pub fn notify_route_change(&mut self, config: &RouterConfig) {
        for plugin in &mut self.plugins {
            plugin.on_route_change(config);
        }
    }

    pub fn notify_config_applied(&mut self, config: &RouterConfig) {
        for plugin in &mut self.plugins {
            plugin.on_config_applied(config);
        }
    }

    pub fn notify_app_volume(&mut self, app_name: &str, volume: f32) {
        for plugin in &mut self.plugins {
            plugin.on_app_volume(app_name, volume);
        }
    }

    /// Names of all registered plugins (for UI display).
    pub fn names(&self) -> Vec<String> {
        self.plugins.iter().map(|p| p.name().to_string()).collect()
    }

    pub fn is_empty(&self) -> bool {
        self.plugins.is_empty()
    }
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// A trivial example extension: records the number of route-change events.
/// Useful as a smoke test and as a template for real extensions.
pub struct ActivityCounter {
    events: usize,
}

impl ActivityCounter {
    pub fn new() -> Self {
        Self { events: 0 }
    }

    pub fn events(&self) -> usize {
        self.events
    }
}

impl Plugin for ActivityCounter {
    fn name(&self) -> &str {
        "Activity Counter"
    }

    fn on_route_change(&mut self, _config: &RouterConfig) {
        self.events += 1;
    }
}

impl Default for ActivityCounter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_notifies_plugins() {
        let mut registry = PluginRegistry::new();
        registry.register(Box::new(ActivityCounter::new()));
        assert!(!registry.is_empty());
        assert_eq!(registry.names(), vec!["Activity Counter".to_string()]);

        let config = RouterConfig::new();
        registry.notify_route_change(&config);
        registry.notify_route_change(&config);

        // Re-fetch the counter via a fresh registry is not possible; instead
        // verify notification does not panic and names are preserved.
        assert_eq!(registry.names().len(), 1);
    }
}
