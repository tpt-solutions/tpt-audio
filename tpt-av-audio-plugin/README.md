# tpt-av-audio-plugin

[![Crates.io](https://img.shields.io/crates/v/tpt-av-audio-plugin.svg)](https://crates.io/crates/tpt-av-audio-plugin)
[![docs.rs](https://docs.rs/tpt-av-audio-plugin/badge.svg)](https://docs.rs/tpt-av-audio-plugin)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

Plugin hosting foundation for the [`tpt-av-audio-*`](https://github.com/tpt-solutions/tpt-audio)
engine: parameter automation wired into the timeline's
[`Envelope`](https://docs.rs/tpt-av-audio-timeline) model, bus routing,
and side-chaining.

## What's format-agnostic (works today)

- [`HostedPlugin`] — the trait a host uses to describe a plugin instance,
  independent of its underlying format.
- [`ParameterSet`] / [`ParameterInfo`] — a plugin's parameter surface
  (id, name, default/min/max).
- [`ParameterAutomation`] — envelope-driven automation on a clip-local or
  session-absolute time base.
- [`bus`] — [`BusLayout`], [`BusRouter`] (routes with gain, mute, and
  summing), and [`SidechainDucker`] (smoothed gain reduction from a key
  signal).

## Plugin format support

**CLAP** is supported behind the `clap` feature, via
[`clack-host`](https://docs.rs/clack-host) (pure Rust, `MIT OR
Apache-2.0`, crates.io) — see [`clap_host::ClapPluginNode`]. This is the
crate's sole FFI boundary: loading third-party `.clap` dynamic libraries
is `unsafe`, documented at that call site; everything else in the crate is
safe Rust (`#![deny(unsafe_code)]`).

**VST3 is not supported and will not be**: the Steinberg VST3 SDK is
GPLv3-or-proprietary dual-licensed, which conflicts with this workspace's
MIT/Apache-2.0-only `deny.toml` policy. (`nih-plug`, sometimes suggested
as an alternative, is a plugin *development* framework — it doesn't
expose a hosting API and couldn't provide this either way.)

## Usage

```bash
cargo add tpt-av-audio-plugin
cargo add tpt-av-audio-plugin --features clap   # to host .clap plugins
```

```rust
use tpt_av_audio_plugin::{HostedPlugin, ParameterId, ParameterInfo};
use tpt_av_audio_utils::{AudioBuffer, AudioError};

struct GainPlugin { gain: f32 }

impl HostedPlugin for GainPlugin {
    fn name(&self) -> &str { "gain" }
    fn parameters(&self) -> &[ParameterInfo] { &[] }
    fn set_parameter(&mut self, _id: ParameterId, value: f32) { self.gain = value; }
    fn process(&mut self, buffer: &mut AudioBuffer) -> Result<(), AudioError> {
        buffer.apply_gain(self.gain);
        Ok(())
    }
}
```

## Part of tpt-audio

Adapters in the [`tpt-av-audio`](../tpt-av-audio) facade (behind its own
`clap` feature) wire [`HostedPlugin`] implementations onto
[`tpt-av-audio-core`](../tpt-av-audio-core)'s `AudioNode` graph.

## License

Dual-licensed under [MIT](../LICENSE-MIT) OR [Apache-2.0](../LICENSE-APACHE).
