# Changelog — tpt-av-audio-plugin

All notable changes to this crate are documented here. See the
[workspace CHANGELOG](../CHANGELOG.md) for the full cross-crate history
and the versioning scheme (all crates in this workspace share one version
number).

## [0.2.0] — Unreleased

### Added
- Initial release as part of the `tpt-av-audio-*` engine workspace pivot:
  the hosting foundation — `HostedPlugin` trait, `ParameterSet`/
  `ParameterInfo`, envelope-driven `ParameterAutomation` (clip-local or
  session time base), `BusLayout`/`BusRouter`, and a smoothed
  `SidechainDucker`.
- CLAP plugin hosting behind the `clap` feature, via `clack-host` (pure
  Rust, `MIT OR Apache-2.0`, crates.io): `clap_host::ClapPluginNode`.
  VST3 is not supported and will not be — the Steinberg VST3 SDK's
  GPLv3-or-proprietary dual license conflicts with this workspace's
  MIT/Apache-2.0-only dependency policy.
