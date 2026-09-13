//! Plugin parameter descriptions and value sets.

use serde::{Deserialize, Serialize};

/// Unique identifier for a plugin parameter (backend-assigned).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ParameterId(pub u32);

/// Static description of one parameter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParameterInfo {
    /// Backend parameter id.
    pub id: ParameterId,
    /// Display name (e.g. "Cutoff").
    pub name: String,
    /// Default value.
    pub default: f32,
    /// Minimum value.
    pub min: f32,
    /// Maximum value.
    pub max: f32,
}

impl ParameterInfo {
    /// Creates a parameter description.
    pub fn new(id: ParameterId, name: impl Into<String>, default: f32, min: f32, max: f32) -> Self {
        Self {
            id,
            name: name.into(),
            default,
            min,
            max,
        }
    }

    /// Clamps `value` into this parameter's range.
    pub fn clamp(&self, value: f32) -> f32 {
        value.clamp(self.min, self.max)
    }
}

/// Current parameter values for one plugin instance, keyed by
/// [`ParameterId`]. Main-thread owned; pushed to the plugin via
/// [`super::HostedPlugin::set_parameter`].
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ParameterSet {
    values: Vec<(ParameterId, f32)>,
}

impl ParameterSet {
    /// Creates an empty set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Initializes a set from parameter defaults.
    pub fn from_infos(infos: &[ParameterInfo]) -> Self {
        Self {
            values: infos.iter().map(|p| (p.id, p.default)).collect(),
        }
    }

    /// Sets a parameter value.
    pub fn set(&mut self, id: ParameterId, value: f32) {
        match self.values.iter_mut().find(|(i, _)| *i == id) {
            Some(slot) => slot.1 = value,
            None => self.values.push((id, value)),
        }
    }

    /// Gets a parameter value, if present.
    pub fn get(&self, id: ParameterId) -> Option<f32> {
        self.values.iter().find(|(i, _)| *i == id).map(|(_, v)| *v)
    }

    /// Iterates all set values.
    pub fn iter(&self) -> impl Iterator<Item = (ParameterId, f32)> + '_ {
        self.values.iter().copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_get_round_trip() {
        let mut set = ParameterSet::new();
        set.set(ParameterId(1), 0.5);
        set.set(ParameterId(2), 0.9);
        assert_eq!(set.get(ParameterId(1)), Some(0.5));
        set.set(ParameterId(1), 0.25); // overwrite, not duplicate
        assert_eq!(set.get(ParameterId(1)), Some(0.25));
        assert_eq!(set.iter().count(), 2);
    }

    #[test]
    fn from_infos_uses_defaults() {
        let infos = [ParameterInfo::new(ParameterId(7), "mix", 0.3, 0.0, 1.0)];
        let set = ParameterSet::from_infos(&infos);
        assert_eq!(set.get(ParameterId(7)), Some(0.3));
    }

    #[test]
    fn info_clamps() {
        let info = ParameterInfo::new(ParameterId(1), "gain", 1.0, 0.0, 2.0);
        assert_eq!(info.clamp(5.0), 2.0);
        assert_eq!(info.clamp(-1.0), 0.0);
    }
}
