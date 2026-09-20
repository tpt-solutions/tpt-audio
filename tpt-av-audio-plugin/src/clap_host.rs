//! CLAP plugin hosting, via [`clack-host`](https://docs.rs/clack-host).
//!
//! This is the only module in this crate permitted to use `unsafe`. Loading
//! a third-party `.clap` dynamic library is inherently unsafe (as with any
//! `dlopen`: the library runs arbitrary code as soon as it is loaded), and
//! `clack-host`'s own bundle-loading entry point (`PluginEntry::load`) is
//! itself an `unsafe fn` for exactly that reason. Everything past that
//! loading boundary is `clack-host`'s own safe API.
//!
//! ## Scope (v1)
//!
//! This is a minimal CLAP audio-effect host, not a full DAW-grade
//! integration:
//! - Audio-effect plugins only (fixed channel count, in ⇄ out through the
//!   same [`AudioBuffer`], one audio port each side). Instruments/note input
//!   are out of scope: [`HostedPlugin`] has no MIDI/note event surface yet.
//! - Parameters are applied per-block ([`set_parameter`](ClapPluginNode::set_parameter)
//!   queues an event flushed at the start of the next [`process`](ClapPluginNode::process)
//!   call), not sample-accurate.
//! - Plugin bundles are loaded from an explicit path; no per-OS CLAP plugin
//!   directory scanning.
//! - GUI hosting is not implemented.
//! - `PluginInstance` (the main-thread handle) is intentionally dropped
//!   once its [`StartedPluginAudioProcessor`] has been obtained: clack-host
//!   documents this as a safe, non-UB leak of the plugin's resources (see
//!   `PluginInstance`'s `Drop` impl), since the processor's `Arc` keeps the
//!   instance's state alive for as long as it is used. Proper deactivation
//!   requires shipping the processor back to the main thread, which this
//!   engine's node lifecycle does not yet support — tracked as a follow-up.

#![allow(unsafe_code)]

use std::path::Path;
use std::sync::OnceLock;

use clack_extensions::params::{ParamInfoBuffer, PluginParams};
use clack_host::events::event_types::ParamValueEvent;
use clack_host::prelude::*;
use tpt_av_audio_utils::{AudioBuffer, AudioError};

use crate::parameter::{ParameterId, ParameterInfo};
use crate::HostedPlugin;

/// Minimal CLAP host callback set: only fetches the plugin's `params`
/// extension during initialization, and otherwise does nothing.
///
/// v1 does not react to the plugin's restart/process/callback requests —
/// the host (this engine) drives processing directly every block instead
/// of waiting to be woken up.
struct ClapHostShared {
    params: OnceLock<Option<PluginParams>>,
}

impl<'a> SharedHandler<'a> for ClapHostShared {
    fn initializing(&self, instance: InitializingPluginHandle<'a>) {
        let _ = self.params.set(instance.get_extension());
    }

    fn request_restart(&self) {}
    fn request_process(&self) {}
    fn request_callback(&self) {}
}

enum ClapHost {}

impl HostHandlers for ClapHost {
    type Shared<'a> = ClapHostShared;
    type MainThread<'a> = ();
    type AudioProcessor<'a> = ();
}

/// A loaded, activated CLAP audio-effect plugin, adapted to [`HostedPlugin`].
pub struct ClapPluginNode {
    // Kept alive for as long as the processor needs the dynamic library
    // mapped; the `PluginInstance` itself is not retained (see module docs).
    _entry: PluginEntry,
    processor: StartedPluginAudioProcessor<ClapHost>,
    name: String,
    parameters: Vec<ParameterInfo>,
    channels: u16,
    max_block_frames: usize,
    input_ports: AudioPorts,
    output_ports: AudioPorts,
    scratch_in: Vec<Vec<f32>>,
    scratch_out: Vec<Vec<f32>>,
    pending_events: EventBuffer,
    output_events: EventBuffer,
    steady_time: u64,
}

impl ClapPluginNode {
    /// Loads the first plugin exposed by the `.clap` bundle at `path`,
    /// activates it as a `channels`-channel audio effect, and starts
    /// processing.
    ///
    /// # Safety-adjacent note
    ///
    /// Loading `path` executes the plugin's own initialization code. Only
    /// load plugins from a source you trust.
    pub fn load(
        path: &Path,
        sample_rate: f64,
        channels: u16,
        max_block_frames: usize,
    ) -> Result<Self, AudioError> {
        // SAFETY: loading a dynamic library is inherently unsafe (it may
        // run arbitrary code); the caller is trusted to only load plugins
        // from a known-good location, per this function's doc note.
        let entry = unsafe { PluginEntry::load(path) }
            .map_err(|e| AudioError::Backend(format!("clap: failed to load {path:?}: {e}")))?;

        let plugin_factory = entry
            .get_plugin_factory()
            .ok_or_else(|| AudioError::Backend(format!("clap: no plugin factory in {path:?}")))?;
        let descriptor = plugin_factory
            .plugin_descriptors()
            .next()
            .ok_or_else(|| AudioError::Backend(format!("clap: no plugins exposed by {path:?}")))?;
        let plugin_id = descriptor
            .id()
            .ok_or_else(|| AudioError::Backend(format!("clap: plugin in {path:?} has no id")))?;
        let name = descriptor
            .name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "clap plugin".to_string());

        let host_info = HostInfo::new(
            "TPT Audio",
            "TPT Solutions",
            "https://github.com/tpt-solutions/tpt-audio",
            env!("CARGO_PKG_VERSION"),
        )
        .map_err(|e| AudioError::Backend(format!("clap: invalid host info: {e}")))?;

        let mut instance = PluginInstance::<ClapHost>::new(
            |_| ClapHostShared {
                params: OnceLock::new(),
            },
            |_shared| (),
            &entry,
            plugin_id,
            &host_info,
        )
        .map_err(|e| AudioError::Backend(format!("clap: failed to instantiate: {e}")))?;

        let config = PluginAudioConfiguration {
            sample_rate,
            min_frames_count: 1,
            max_frames_count: max_block_frames as u32,
        };
        let stopped = instance
            .activate(|_, _| (), config)
            .map_err(|e| AudioError::Backend(format!("clap: failed to activate: {e}")))?;

        let parameters = Self::snapshot_parameters(&mut instance);

        let processor = stopped
            .start_processing()
            .map_err(|_| AudioError::Backend("clap: failed to start processing".to_string()))?;

        Ok(Self {
            _entry: entry,
            processor,
            name,
            parameters,
            channels,
            max_block_frames,
            input_ports: AudioPorts::with_capacity(channels as usize, 1),
            output_ports: AudioPorts::with_capacity(channels as usize, 1),
            scratch_in: vec![vec![0.0; max_block_frames]; channels as usize],
            scratch_out: vec![vec![0.0; max_block_frames]; channels as usize],
            pending_events: EventBuffer::new(),
            output_events: EventBuffer::new(),
            steady_time: 0,
        })
    }

    /// Queries the plugin's `params` extension (if any) and returns its
    /// parameter surface. Must be called on the main thread before the
    /// audio processor is started.
    fn snapshot_parameters(instance: &mut PluginInstance<ClapHost>) -> Vec<ParameterInfo> {
        let params_ext = instance.access_shared_handler(|h| h.params.get().copied().flatten());
        let Some(params_ext) = params_ext else {
            return Vec::new();
        };

        let plugin_handle = instance.plugin_handle();
        let count = params_ext.count(&plugin_handle);
        let mut buffer = ParamInfoBuffer::new();
        let mut parameters = Vec::with_capacity(count as usize);
        for index in 0..count {
            if let Some(info) = params_ext.get_info(&plugin_handle, index, &mut buffer) {
                parameters.push(ParameterInfo::new(
                    ParameterId(info.id.get()),
                    String::from_utf8_lossy(info.name).into_owned(),
                    info.default_value as f32,
                    info.min_value as f32,
                    info.max_value as f32,
                ));
            }
        }
        parameters
    }

    /// Copies `buffer`'s interleaved channels into the pre-allocated
    /// per-channel scratch buffers. Allocation-free: `scratch` is sized to
    /// `max_block_frames` once at construction.
    fn deinterleave(buffer: &AudioBuffer, scratch: &mut [Vec<f32>], frames: usize) {
        let channels = buffer.channels as usize;
        for (c, channel_buf) in scratch.iter_mut().enumerate() {
            for (frame, sample) in channel_buf.iter_mut().take(frames).enumerate() {
                *sample = buffer.data[frame * channels + c];
            }
        }
    }

    /// Writes the per-channel scratch buffers back into `buffer`'s
    /// interleaved layout.
    fn interleave(buffer: &mut AudioBuffer, scratch: &[Vec<f32>], frames: usize) {
        let channels = buffer.channels as usize;
        for (c, channel_buf) in scratch.iter().enumerate() {
            for (frame, sample) in channel_buf.iter().take(frames).enumerate() {
                buffer.data[frame * channels + c] = *sample;
            }
        }
    }
}

impl HostedPlugin for ClapPluginNode {
    fn name(&self) -> &str {
        &self.name
    }

    fn parameters(&self) -> &[ParameterInfo] {
        &self.parameters
    }

    fn set_parameter(&mut self, id: ParameterId, value: f32) {
        let Some(clap_id) = ClapId::from_raw(id.0) else {
            return;
        };
        let event = ParamValueEvent::new(0, clap_id, Pckn::match_all(), f64::from(value));
        self.pending_events.push(&event);
    }

    fn process(&mut self, buffer: &mut AudioBuffer) -> Result<(), AudioError> {
        if buffer.channels != self.channels {
            return Err(AudioError::InvalidConfig(format!(
                "clap plugin '{}' expects {} channels, buffer has {}",
                self.name, self.channels, buffer.channels
            )));
        }
        let frames = buffer.frames.min(self.max_block_frames);

        Self::deinterleave(buffer, &mut self.scratch_in, frames);

        let input_audio = self.input_ports.with_input_buffers([AudioPortBuffer {
            latency: 0,
            channels: AudioPortBufferType::f32_input_only(
                self.scratch_in
                    .iter_mut()
                    .map(|ch| InputChannel::variable(&mut ch[..frames])),
            ),
        }]);
        let mut output_audio = self.output_ports.with_output_buffers([AudioPortBuffer {
            latency: 0,
            channels: AudioPortBufferType::f32_output_only(
                self.scratch_out.iter_mut().map(|ch| &mut ch[..frames]),
            ),
        }]);

        let input_events = self.pending_events.as_input();
        let mut output_events = self.output_events.as_output();

        self.processor
            .process(
                &input_audio,
                &mut output_audio,
                &input_events,
                &mut output_events,
                Some(self.steady_time),
                None,
            )
            .map_err(|e| AudioError::Backend(format!("clap: process failed: {e}")))?;

        self.pending_events.clear();
        self.output_events.clear();
        self.steady_time = self.steady_time.wrapping_add(frames as u64);

        Self::interleave(buffer, &self.scratch_out, frames);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deinterleave_interleave_round_trip() {
        // 2 channels, 3 frames: L0 R0 L1 R1 L2 R2.
        let mut buffer = AudioBuffer::new(3, 2);
        buffer
            .data
            .copy_from_slice(&[1.0, -1.0, 2.0, -2.0, 3.0, -3.0]);

        let mut scratch = vec![vec![0.0; 3]; 2];
        ClapPluginNode::deinterleave(&buffer, &mut scratch, 3);
        assert_eq!(scratch[0], [1.0, 2.0, 3.0]);
        assert_eq!(scratch[1], [-1.0, -2.0, -3.0]);

        // Simulate an effect that doubles the signal, then write it back.
        for channel in &mut scratch {
            for sample in channel.iter_mut() {
                *sample *= 2.0;
            }
        }

        let mut out = AudioBuffer::new(3, 2);
        ClapPluginNode::interleave(&mut out, &scratch, 3);
        assert_eq!(out.data, vec![2.0, -2.0, 4.0, -4.0, 6.0, -6.0]);
    }

    #[test]
    fn deinterleave_only_touches_requested_frames() {
        let mut buffer = AudioBuffer::new(4, 1);
        buffer.data.copy_from_slice(&[1.0, 2.0, 3.0, 4.0]);

        let mut scratch = vec![vec![9.0; 4]; 1];
        ClapPluginNode::deinterleave(&buffer, &mut scratch, 2);
        assert_eq!(scratch[0], [1.0, 2.0, 9.0, 9.0]);
    }
}
