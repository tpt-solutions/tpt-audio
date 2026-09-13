//! Timeline-driven parameter automation.
//!
//! Automation reuses the timeline's [`Envelope`] model: each automated
//! parameter points at an envelope evaluated in **clip-local** or
//! **session-global** frames (host's choice), producing sample-accurate
//! parameter streams without any per-sample allocation.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use tpt_av_audio_timeline::{Envelope, InterpolationMethod};

use crate::parameter::ParameterId;

/// Which time base an automation envelope is evaluated in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimeBase {
    /// Frames relative to the plugin's host clip start.
    ClipLocal,
    /// Frames relative to the session start (absolute timeline position).
    Session,
}

/// One parameter's automation lane.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AutomationLane {
    pub parameter: ParameterId,
    pub time_base: TimeBase,
    /// Envelope points in the chosen time base.
    pub envelope: Envelope,
}

/// All automation for a plugin instance.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ParameterAutomation {
    lanes: BTreeMap<ParameterId, AutomationLane>,
}

impl ParameterAutomation {
    /// Creates empty automation.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds (or replaces) an automation lane.
    pub fn add_lane(&mut self, lane: AutomationLane) {
        self.lanes.insert(lane.parameter, lane);
    }

    /// Removes the lane for a parameter. Returns whether one existed.
    pub fn remove_lane(&mut self, parameter: ParameterId) -> bool {
        self.lanes.remove(&parameter).is_some()
    }

    /// The lane for a parameter, if automated.
    pub fn lane(&self, parameter: ParameterId) -> Option<&AutomationLane> {
        self.lanes.get(&parameter)
    }

    /// All automated parameters, in id order.
    pub fn automated_parameters(&self) -> Vec<ParameterId> {
        self.lanes.keys().copied().collect()
    }

    /// Evaluates a parameter at `frame`.
    ///
    /// Returns `None` when the parameter is not automated — the caller then
    /// falls back to the manual [`crate::ParameterSet`] value.
    pub fn value_at(
        &self,
        parameter: ParameterId,
        clip_start: u64,
        session_frame: u64,
    ) -> Option<f32> {
        let lane = self.lanes.get(&parameter)?;
        let local = match lane.time_base {
            TimeBase::ClipLocal => session_frame.saturating_sub(clip_start),
            TimeBase::Session => session_frame,
        };
        Some(lane.envelope.value_at(local))
    }

    /// Convenience constructor: a linear ramp lane for one parameter.
    pub fn linear_lane(
        parameter: ParameterId,
        time_base: TimeBase,
        points: Vec<(u64, f32)>,
    ) -> Self {
        let mut automation = Self::new();
        automation.add_lane(AutomationLane {
            parameter,
            time_base,
            envelope: Envelope::with_points(
                points
                    .into_iter()
                    .map(|(frame, value)| tpt_av_audio_timeline::EnvelopePoint { frame, value })
                    .collect(),
                InterpolationMethod::Linear,
            ),
        });
        automation
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GAIN: ParameterId = ParameterId(1);

    #[test]
    fn unautomated_parameters_return_none() {
        let automation = ParameterAutomation::new();
        assert_eq!(automation.value_at(GAIN, 0, 100), None);
    }

    #[test]
    fn session_timebase_uses_absolute_frames() {
        let automation =
            ParameterAutomation::linear_lane(GAIN, TimeBase::Session, vec![(0, 0.0), (100, 1.0)]);
        assert_eq!(automation.value_at(GAIN, 1_000, 50), Some(0.5));
        assert_eq!(automation.value_at(GAIN, 1_000, 25), Some(0.25));
        // Clip start is irrelevant in the session time base.
        assert_eq!(automation.value_at(GAIN, 999_999, 50), Some(0.5));
    }

    #[test]
    fn clip_local_timebase_offsets_by_clip_start() {
        let automation =
            ParameterAutomation::linear_lane(GAIN, TimeBase::ClipLocal, vec![(0, 0.0), (100, 1.0)]);
        // Session frame 1_050 is 50 frames into the clip starting at 1_000.
        assert_eq!(automation.value_at(GAIN, 1_000, 1_050), Some(0.5));
        // Before the clip start saturates to local frame 0.
        assert_eq!(automation.value_at(GAIN, 1_000, 500), Some(0.0));
    }

    #[test]
    fn step_automation_holds_values() {
        let mut automation = ParameterAutomation::new();
        automation.add_lane(AutomationLane {
            parameter: GAIN,
            time_base: TimeBase::Session,
            envelope: Envelope::with_points(
                vec![
                    tpt_av_audio_timeline::EnvelopePoint {
                        frame: 0,
                        value: 1.0,
                    },
                    tpt_av_audio_timeline::EnvelopePoint {
                        frame: 50,
                        value: 0.0,
                    },
                ],
                InterpolationMethod::Step,
            ),
        });
        assert_eq!(automation.value_at(GAIN, 0, 49), Some(1.0));
        assert_eq!(automation.value_at(GAIN, 0, 50), Some(0.0));
    }

    #[test]
    fn lane_management() {
        let mut automation =
            ParameterAutomation::linear_lane(GAIN, TimeBase::Session, vec![(0, 1.0)]);
        assert_eq!(automation.automated_parameters(), vec![GAIN]);
        assert!(automation.remove_lane(GAIN));
        assert!(!automation.remove_lane(GAIN));
        assert!(automation.automated_parameters().is_empty());
    }
}
