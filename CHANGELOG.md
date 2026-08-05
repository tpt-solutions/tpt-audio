# Changelog

All notable changes to tpt-audio are documented here.

## Versioning scheme

tpt-audio follows [Semantic Versioning](https://semver.org/) (`MAJOR.MINOR.PATCH`):

- **MAJOR** — breaking changes to the routing model, config format, or platform support.
- **MINOR** — new features in a backwards-compatible way (new backends, UI panels, presets).
- **PATCH** — backwards-compatible bug fixes and hardening.

Pre-release builds use the `-alpha.N` / `-beta.N` / `-rc.N` suffixes (e.g. `1.0.0-rc.1`).
The Windows/MSI, Linux package, and Archon builds share a single version number defined in
the workspace `Cargo.toml` (`workspace.package.version`).

Update checks (Settings → Check for Updates) compare the local version against the latest
GitHub release `tag_name`.

## [1.0.0] — Unreleased

### Added
- Visual routing matrix: route any source (mic, app, input) to any sink (speakers, headset).
- Per-route gain and mute directly in the matrix.
- Per-app volume control (Windows session volume; Linux per-stream via PipeWire).
- Preset save/load **and** import/export to shareable JSON files.
- Live device/app list with auto-refresh and hotplug detection.
- Diagnostics panel: underrun/overrun counters, device churn, and an event log.
- Crash reporting: a panic hook writes a crash report and rolling log to the temp directory.
- Accessibility pass on the routing matrix (descriptive tooltips, labeled controls).
- In-app update check (Settings → Check for Updates).
- Windows installer (WiX/MSI) with a code-signing plan (see `docs/SIGNING.md`).
- Linux packaging definitions (Flatpak manifest, `.desktop`, metainfo).

### Platforms
- **Windows** — WASAPI capture/render in shared mode, per-app session volume.
- **Linux** — PipeWire backend via the standard CLI tools (`pw-link`, `wpctl`, `pw-dump`).
  Native `pipewire-rs` mixing proxy is a future option.
- **Archon** — backend stub behind the shared `AudioBackend` trait; `AUDIO_CAPTURE`
  capability request/grant model (prototype blocked on `tpt-archon-bridge` API).

### Known limitations
- Manual test passes on Windows/Linux/Archon hardware are pending.
- Linux native `pipewire-rs` zero-copy backend is not yet implemented.
- Archon zero-copy IPC prototype is blocked on the upstream API.
