# Changelog

All notable changes to tpt-audio are documented here.

## Versioning scheme

tpt-audio follows [Semantic Versioning](https://semver.org/) (`MAJOR.MINOR.PATCH`):

- **MAJOR** — breaking changes to a crate's public API or the timeline JSON format.
- **MINOR** — new features in a backwards-compatible way (new backends, new DSP).
- **PATCH** — backwards-compatible bug fixes and hardening.

Pre-release builds use the `-alpha.N` / `-beta.N` / `-rc.N` suffixes (e.g. `0.1.0-rc.1`).
All crates in the workspace share a single version number defined in the root
`Cargo.toml` (`workspace.package.version`).

## [0.2.0] — Engine pivot (unreleased)

This release pivots the repository from the **router/mixer desktop app** into
the **`tpt-av-audio-*` non-destructive audio engine library workspace**
described in `spec2.txt`. The complete old app (crates, installer manifests,
packaging, signing/website docs, `spec.txt`) is preserved unmodified and
buildable under `legacy/` (excluded from the workspace).

### Added — engine workspace
- **`tpt-av-audio-utils`** — dependency-free foundation: `Sample` conversions
  (`f32`/`i16`/`u8`/`i32`), canonical interleaved `AudioBuffer`, frame/second/
  millisecond time math, shared `AudioError`.
- **`tpt-av-audio-timeline`** — the pure non-destructive data model:
  `Session`/`Track`/`Clip`/`AudioAsset`, `Envelope` with linear/cubic/step
  interpolation, undoable edit operations (insert/remove/move/split with
  fade continuation and envelope continuity across splits), bounded
  undo/redo `History`, and full serde support for JSON sessions.
- **`tpt-av-audio-core`** — the real-time engine:
  - `AudioGraph` + `AudioNode` (topologically ordered, real-time safe).
  - `TrackMixer` with balance-law pan, mute, and solo.
  - Built-in DSP: gain, constant-power pan, position-tracking fade, and
    resampling (`rubato` sinc offline + allocation-free linear inline).
  - `TimelineRenderer`: lock-free snapshot → PCM → clip envelopes/fades →
    track strip state → mix, with inline sample-rate conversion and playhead.
  - `TimelineState` (`arc-swap`): wait-free Main→Audio thread snapshot sync.
  - Asset management: `AssetStore` (pre-allocated PCM caches; writer-side
    mutex fixes a lost-update race between concurrent inserts), background
    `DecodePool`, lock-free `SpscRing` handoff.
  - **`tpt-cadence` integration** (behind the `cadence` feature): decodes
    WAV, AIFF, and FLAC through the cadence codec suite's real-time-safe
    `Decoder` contract, dispatched via the `DecodeRegistry`. cadence is
    wired through path dependencies on a sibling checkout until its codec
    crates are pushed to GitHub; without the feature, WAV decodes via the
    built-in hound fallback so CI and fresh clones build.
  - Real-time safety audit: `tests/rt_safety.rs` counts heap allocations
    through a global allocator and asserts the render path is
    allocation-free.
  - Examples: `headless_render` (timeline JSON → WAV), `simple_player`
    (WAV → live playback), `mixer_demo` (synthesized multi-track mix).
- **`tpt-av-audio-io`** — OS audio I/O with the spec §4.3 surface
  (`AudioDevice`, streams, `enumerate_devices()`):
  - WASAPI backend (Windows) ported from the old `platform-windows` crate:
    shared-mode render/capture, `AUTOCONVERTPCM` format adaptation, loopback
    capture of output devices, lazy per-thread COM init.
    *Verified live on Windows hardware.*
  - PipeWire backend (Linux) ported from `platform-linux`: device
    enumeration via `pw-dump` (streams pending the native `pipewire-rs`
    port).
  - CoreAudio backend (macOS): stub (new surface for this repo).
  - Archon: research-gated capability-broker stub (upstream API still
    unavailable).
  - `NullBackend`: dependency-free test/CI sink (`TPT_AUDIO_BACKEND=null`).
  - `VirtualRouter`: in-process mixing router (future virtual-device core).
- **`tpt-av-audio-plugin`** — hosting foundation: `HostedPlugin` trait,
  `ParameterSet`/`ParameterInfo`, envelope-driven `ParameterAutomation`
  (clip-local or session time base), `BusLayout`/`BusRouter`, and a
  smoothed `SidechainDucker`.

### Added — adoption & ergonomics
- **`tpt-av-audio` umbrella crate** — one dependency over the whole
  workspace: the `Engine` facade (timeline state + asset store + decoder
  registry + renderer pre-wired, with blocking and background asset
  loading), `play_file`/`play_file_blocking` one-call playback, and
  `offline::render_session_to_wav`.
- Ergonomics across the workspace: `Clip::new` + builder methods
  (`with_fades`, `with_volume_envelope`, …), `Session::save`/`load` JSON
  documents, `AudioBuffer::peak`/`rms`/`combined_peak` level meters.
- **PipeWire streams**: playback and capture through `pw-cat` raw f32
  pipes (`PwCatWriter`/`PwCatReader`), complementing `pw-dump`
  enumeration; the native `pipewire-rs` port remains the future upgrade.
- Security policy & supply-chain hardening: `SECURITY.md`, RustSec
  advisories and source restrictions in `cargo-deny` (new `security` CI
  job), `#![forbid(unsafe_code)]` on utils/timeline/plugin, explicit
  unsafe-block contracts in the WASAPI backend, MSRV (1.75) CI job,
  Dependabot for Actions, and `CONTRIBUTING.md`.

### Added — tooling & policy
- `deny.toml` (cargo-deny): permissive-only dependency policy; GPL-2.0,
  GPL-3.0, LGPL-2.1/3.0, AGPL-3.0, and MPL-2.0 are denied (plus
  `Unicode-3.0` allowed for `unicode-ident`, a transitive serde dependency).
- CI: new `licenses` job running cargo-deny; CI now also triggers on the
  `master` branch.
- Workspace: `[workspace.package]` (version 0.1.0, edition 2021, license
  `MIT OR Apache-2.0`, rust-version 1.75) and shared `[workspace.dependencies]`.

### Changed
- Licensing stays **dual MIT / Apache-2.0** across the pivot (overriding
  spec2's "pure MIT" wording); root `Cargo.toml` and all crates carry
  `license = "MIT OR Apache-2.0"`.
- README and QUICKSTART rewritten for the library-not-app model.

### Retired (preserved under `legacy/`)
- `core`, `gui`, `desktop` — the router app (egui UI, presets, i18n,
  diagnostics, update checks).
- `platform-windows`, `platform-linux`, `platform-archon` — superseded by
  the `tpt-av-audio-io` backends.
- `wix/`, `packaging/linux`, `docs/SIGNING.md`, `docs/WEBSITE.md`,
  `QUICKSTART.md` app sections — app-distribution artifacts for a library
  workspace.

### Known limitations
- `tpt-cadence` integration is blocked upstream (the repository is
  spec-only today); WAV decoding covers the gap via the decoder registry.
- PipeWire stream playback/capture is pending the native `pipewire-rs`
  port; enumeration works.
- CoreAudio and Archon backends are stubs.
- Plugin hosting (CLAP via `clack-host`, VST3) is future work; `nih-plug`
  cannot provide hosting (it is a plugin *development* framework).
- The Archon zero-copy IPC prototype remains blocked on the upstream API.

## [1.0.0] — Router app (archived)

The desktop router release line is archived with the app under `legacy/`.
Its feature set (visual routing matrix, per-route gain/mute, per-app
volume, presets, hotplug detection, diagnostics, crash reporting, WiX/MSI
and Flatpak packaging) is preserved there; see `legacy/` for the full app.
