# Changelog — tpt-av-audio-io

All notable changes to this crate are documented here. See the
[workspace CHANGELOG](../CHANGELOG.md) for the full cross-crate history
and the versioning scheme (all crates in this workspace share one version
number).

## [0.2.0] — Unreleased

### Added
- Initial release as part of the `tpt-av-audio-*` engine workspace pivot,
  implementing the spec2 §4.3 surface (`AudioDevice`, streams,
  `enumerate_devices()`):
  - WASAPI backend (Windows), ported from the old `platform-windows`
    crate: shared-mode render/capture, `AUTOCONVERTPCM` format
    adaptation, loopback capture of output devices, lazy per-thread COM
    init. Verified live on Windows hardware.
  - PipeWire backend (Linux), ported from `platform-linux`: device
    enumeration via `pw-dump`, plus playback/capture through `pw-cat`
    raw f32 pipes (`PwCatWriter`/`PwCatReader`). A native `pipewire-rs`
    port remains the future upgrade.
  - CoreAudio backend (macOS): stub, new surface for this repo.
  - Archon: research-gated capability-broker stub (upstream API still
    unavailable).
  - `NullBackend`: dependency-free test/CI sink (`TPT_AUDIO_BACKEND=null`).
  - `VirtualRouter`: in-process mixing router (groundwork for a future
    virtual-device core).

### Changed
- Removed the unused `log` dependency (no call sites in this crate).
