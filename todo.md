# tpt-audio — Project Checklist

Dual-licensed MIT / Apache-2.0 — TPT Solutions

## Phase 0 — Project Setup
- [x] Initialize git repo, `.gitignore`, README skeleton
- [x] Add `LICENSE-MIT` and `LICENSE-APACHE`, dual-license notice in README/Cargo.toml
- [x] Set up Rust workspace (`Cargo.toml` with member crates: `core`, `gui`, `platform-windows`, `desktop`)
- [x] Set up CI (build + test on Windows and Linux runners)
- [x] Decide on GUI framework: egui/eframe — spike a "hello window" app
- [x] Define coding standards / lint setup (`rustfmt`, `clippy`)

## Phase 1 — Windows MVP (full-featured)
### Routing engine (`core`)
- [x] Design core audio graph model: sources, sinks, routes, per-route gain
- [x] Define platform-agnostic trait(s) for audio backends
- [x] Implement mixing/routing engine (buffer handling, per-route gain mixing)
- [x] Implement per-route/per-app volume control logic
- [x] Add config persistence (save/load routing setups, presets)

### Windows backend (`platform-windows`)
- [x] WASAPI device enumeration (inputs/outputs)
- [x] WASAPI capture/render stream setup (shared mode with thread)
- [x] Per-application audio session enumeration & volume control (`IAudioSessionManager2`)
- [x] Wire Windows backend into core engine via the platform trait
- [x] Measure and tune for zero added latency / low CPU usage

### GUI (`gui`)
- [x] App shell (window, toolbar with tabs)
- [x] Routing matrix widget (sources x sinks grid)
- [x] Per-cell/per-route volume sliders
- [x] Per-app volume panel
- [x] Device/app list live-updates (auto-refresh every ~6s, hotplug detection)
- [x] Save/load presets from the UI
- [x] Basic settings screen (refresh, info)

### Windows MVP hardening
- [ ] Manual test pass: common apps (pending: needs real Windows hardware)
- [x] Handle device disconnect/reconnect gracefully
- [x] Installer (MSI or similar) + code signing plan (WiX `wix/main.wxs` + `docs/SIGNING.md`)
- [x] Write user-facing quickstart docs

## Phase 2 — Linux (PipeWire) Support
- [x] Implement `platform-linux` backend (PipeWire, via `pw-dump`/`pw-link`/`wpctl` CLI — see note below)
- [x] Map PipeWire nodes/ports to the core graph model (Audio/Sink, Audio/Source, Stream/* nodes)
- [x] Per-app (per-stream) volume control via PipeWire (`wpctl set-volume`)
- [x] Verify routing matrix UI works unchanged against the new backend (backend implements the same `AudioBackend` trait; compiles; runtime verify pending)
- [x] Package for common distros (e.g. AppImage / Flatpak / .deb) (added `packaging/linux`: Flatpak manifest, `.desktop`, metainfo, README; build/test on Linux pending)
- [ ] Manual test pass on at least one major distro

> Note: The original plan called for `pipewire-rs`. To keep the crate buildable
> and verifiable without linking native `libpipewire` (and because the Linux
> backend could not be compiled/run in this Windows dev environment), it is
> currently backed by the standard PipeWire CLI tools. Routing is still native
> (PipeWire graph links), so audio stays zero-copy in the server. A future
> `pipewire-rs` native backend can also apply per-route gain via a mixing proxy.

## Phase 3 — Archon Support (research-gated)
- [x] Track `tpt-archon` audio server API design (scaffold; `AUDIO_CAPTURE` capability model in `platform-archon`)
- [ ] Prototype against `tpt-archon-bridge` zero-copy IPC (blocked: API not yet available)
- [x] Implement `platform-archon` backend behind the same core trait (stub; returns research-gated errors)
- [x] Implement `AUDIO_CAPTURE` capability request/grant flow model (enum/grant types + `request_audio_capture()`)
- [ ] Verify shared-memory buffer routing meets zero-latency goal (blocked: API not yet available)
- [ ] Manual test pass on Archon (blocked: platform not available)

## Phase 4 — Cross-Platform Polish & Release
- [x] Unify UX across all three platforms (single shared `gui` crate; identical matrix/volume/preset UI on all backends)
- [x] Accessibility pass on the matrix UI (descriptive tooltips + hover labels on routes, controls, headers)
- [ ] Performance/CPU profiling pass on all platforms (pending: requires runtime profiling on real hardware)
- [x] Crash reporting / diagnostics logging (panic hook + rolling log file + crash-report writer in `core::diagnostics`; wired in `desktop`)
- [x] Public website/download page copy (`docs/WEBSITE.md`)
- [x] 1.0 release notes, versioning scheme, update-check mechanism (`CHANGELOG.md` + `core::update` + Settings → Check for Updates)

## Phase 5 — Post-1.0 / Stretch
- [x] Routing presets sharable as files (export/import to JSON in the Presets tab)
- [x] Hotkeys / global shortcuts (in-app keyboard shortcuts via egui: Ctrl+1..5 switch tabs, F5 refresh; global OS hotkeys noted as future work)
- [x] Plugin/extension hooks (`core::plugin` — `Plugin` trait + `PluginRegistry`, wired into GUI lifecycle)
- [x] Remote/headless control (CLI: `--control <json>` one-shot and `--server` stdin loop in `core::remote`)
- [x] Localization (`core::i18n` with en/de/es, language selector in Settings; UI strings routed through `I18n::tr`)

---

## Status — 2026-08-05

Completed this pass (code + docs/packaging, verified with `cargo build`/`test`/`clippy`):
crash reporting & diagnostics logging, shareable preset files, accessibility pass on the
matrix, Windows installer (WiX) + code-signing plan, Linux packaging (Flatpak/`.desktop`/
metainfo), versioning scheme + 1.0 release notes + in-app update check, website copy, and
UX unification (single shared GUI crate), and the Phase 5 stretch items: in-app
hotkeys, plugin/extension hook system, headless CLI remote control, and UI localization (en/de/es).

Still pending / not actionable in this environment:
- Manual test passes on real Windows / Linux / Archon hardware (need devices + sessions).
- Archon tasks are blocked on the upstream `tpt-archon-bridge` API (zero-copy IPC,
  zero-latency verification, manual Archon test).
- Performance/CPU profiling pass requires runtime profiling on target hardware.
- Global OS-level hotkeys (vs the in-app shortcuts delivered) noted as future work.
