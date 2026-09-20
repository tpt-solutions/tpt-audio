# tpt-audio — Upgrade Checklist (Engine Pivot)

Dual-licensed MIT / Apache-2.0 — TPT Solutions

> Pivoting `tpt-audio` from the router/mixer app (`core`/`gui`/`desktop`/`platform-*`) into
> the `tpt-av-audio-*` non-destructive engine workspace described in `spec2.txt`. New crates
> are scaffolded incrementally; old crates keep building until each is fully migrated or
> retired. License stays dual MIT/Apache-2.0 (overrides spec2's "pure MIT" wording);
> `tpt-cadence` / `tpt-kinetix` are tracked only as external git dependencies, not built here.
>
> **Status note (2026-09-15):** Phases 0–3, 5, and the local parts of 4/6 are done.
> **tpt-cadence is now integrated** (WAV/AIFF/FLAC behind the `tpt-av-audio/cadence`
> feature; path deps on the sibling checkout until it is pushed to GitHub).
> **Review pass complete:** added the `tpt-av-audio` umbrella crate (`Engine`
> facade, one-call `play_file`/`render_session_to_wav`), implemented PipeWire
> streams via `pw-cat` (previously a stub), ergonomics (`Clip::new` builders,
> `Session::save`/`load`, `AudioBuffer::peak`/`rms`), and security hardening
> (`cargo audit` clean, RustSec advisories + source policy in cargo-deny,
> `#![forbid(unsafe_code)]` on the pure crates, SECURITY.md, MSRV CI job,
> Dependabot, CONTRIBUTING.md).
> Crossfades and musical-time helpers are in (Phase 7), with Engine-level
> undo/redo. Full workspace: `cargo test` passing (198 base / 201 with
> cadence), clippy `-D warnings` clean, fmt clean, full `cargo deny check`
> passing.
> Remaining items are external blockers (pushing cadence, crates.io publishing,
> CoreAudio/Archon upstreams, CLAP/VST3 hosting) or deferred.

---

## Phase 0 — Pivot Setup & Decisions
- [x] Confirm/record this pivot decision in README + CHANGELOG (router app → audio engine library)
- [x] Decide fate of `gui` + `desktop` (router UI): **decided — archive in this repo under `legacy/`, excluded from the workspace**
- [x] Update root `Cargo.toml` license field to `"MIT OR Apache-2.0"` (keep dual, do not follow spec2's MIT-only text)
- [x] Update README.md badges/description for new engine positioning; keep dual-license notice
- [x] Create `deny.toml` (cargo-deny) with allow list `MIT, Apache-2.0, BSD-2-Clause, BSD-3-Clause, ISC, Zlib`; deny `GPL-2.0, GPL-3.0, LGPL-2.1, LGPL-3.0, AGPL-3.0, MPL-2.0`
  (implemented as a v2 allow list: everything not allowed is denied; `Unicode-3.0` added for `unicode-ident` via serde)
- [x] Add cargo-deny check to CI workflow (`.github/workflows`) — also added `master` to CI branch triggers
- [x] Scaffold new workspace members in root `Cargo.toml`: `tpt-av-audio-utils`, `tpt-av-audio-timeline`, `tpt-av-audio-core`, `tpt-av-audio-io`, `tpt-av-audio-plugin`
- [x] Add `[workspace.package]` fields: version `0.1.0`, edition `2021`, license `"MIT OR Apache-2.0"`, rust-version `1.75`
- [x] Rename/retire `spec.txt` (old router spec) — **moved to `legacy/spec.txt`**
- [x] Decide whether existing `platform-archon` research/stub content is preserved for later I/O backend work — **decided: preserved as the research-gated `archon` module (capability broker) in `tpt-av-audio-io`**

---

## Phase 1 — Foundation & Timeline Model
- [x] Scaffold `tpt-av-audio-utils` crate (`lib.rs`, `sample.rs`, `buffer.rs`, `time.rs`, `error.rs`)
  - [x] `Sample` types (f32, i16, etc.)
  - [x] `AudioBuffer`-adjacent buffer abstractions (canonical interleaved `AudioBuffer`)
  - [x] Time/duration types (frames, seconds, samples) with conversions
  - [x] `AudioError` enum
- [x] Scaffold `tpt-av-audio-timeline` crate (`lib.rs`, `session.rs`, `track.rs`, `clip.rs`, `asset.rs`, `envelope.rs`, `edit.rs`, `history.rs`)
  - [x] `Session`, `Track`, `Clip`, `AudioAsset` structs per spec §4.1
  - [x] `Envelope` / `EnvelopePoint` + `InterpolationMethod` (linear, cubic, step)
  - [x] Edit operations: insert, delete, move, split
  - [x] Undo/redo history
  - [x] Unit tests: clip splitting, envelope interpolation math
- [x] Wire up cargo-deny CI pipeline enforcing license policy (verified locally with cargo-deny 0.20: `licenses ok`)

---

## Phase 2 — I/O Layer Integration
- [x] Scaffold `tpt-av-audio-io` crate (`lib.rs`, `device.rs`, `stream.rs`, `backend/`, `router.rs`)
- [x] Port WASAPI logic from `platform-windows` → `tpt-av-audio-io/src/backend/wasapi.rs`
  (shared-mode render + capture, loopback for output devices, `AUTOCONVERTPCM`; **verified live on Windows hardware**)
- [x] Port PipeWire logic from `platform-linux` → `tpt-av-audio-io/src/backend/pipewire.rs`
  (enumeration via `pw-dump`; streams deferred — see limitations)
- [x] Add/port CoreAudio backend stub → `tpt-av-audio-io/src/backend/coreaudio.rs` (macOS, new — not in old repo)
- [x] Decide fate of `platform-archon` (research-gated) — ported the capability model as `backend/archon.rs`; transport still blocked upstream
- [x] Define `AudioDevice`, `AudioStream`, `enumerate_devices()` per spec §4.3 (replaces old `AudioBackend` trait)
  (`OutputStream`/`InputStream` replace the old callback struct per spec §4.3)
- [x] PipeWire **stream playback/capture** via `pw-cat` raw-f32 pipes (`PwCatWriter`/`PwCatReader`);
  graceful `Unsupported` + hint when `pw-cat` is absent. Native `pipewire-rs` remains the future upgrade.
- [x] Migrate device enumeration + stream start/stop tests from old `platform-*` crates
  (old crates had no tests — new null-backend integration tests cover enumeration, stream start/stop, error surfacing)
- [x] Integration tests for device enumeration and stream management (`tpt-av-audio-io/tests/streams.rs`)
- [x] Retire old `platform-windows`, `platform-linux`, `platform-archon` crates once ported (moved to `legacy/`, removed from workspace members)

---

## Phase 3 — Real-Time Mixer
- [x] Scaffold `tpt-av-audio-core` crate (`lib.rs`, `graph.rs`, `mixer.rs`, `renderer.rs`, `dsp/`, `scheduler.rs`)
- [x] `AudioGraph` + `AudioNode` trait (real-time safe: allocation-free, lock-free, panic-free `process()`)
  (documented summing-bus semantics with topological processing; multi-bus routing deferred to the plugin crate's bus model)
- [x] `AudioBuffer` (interleaved f32) type — reconciled: the canonical `AudioBuffer` lives in `tpt-av-audio-utils` and `tpt-av-audio-core` re-exports it
- [x] Multi-track `mixer.rs` (balance-law pan, mute, solo)
- [x] `TimelineRenderer`: reads timeline snapshot, fetches PCM, applies envelopes, mixes, advances playhead
- [x] Built-in DSP: `gain.rs`, `pan.rs`, `fade.rs`
- [x] `resample.rs` wrapping `rubato` for sample rate conversion
  (+ allocation-free `linear_resample_into` for the real-time renderer path)
- [x] Lock-free state sync: `TimelineState` (snapshot swap)
  (implementation note: spec2's `AtomicPtr` double-buffer is unsound — a writer can overwrite a slot the audio thread is still reading; replaced with `arc-swap`, which is wait-free on the read side with safe reclamation)
- [x] Real-time safety audit: `tests/rt_safety.rs` — counting global allocator proves the render/graph paths are allocation-free
- [x] `examples/headless_render.rs`: renders a timeline JSON to WAV (smoke-tested end-to-end with `examples/podcast_demo.json`)
- [x] `examples/simple_player.rs`, `examples/mixer_demo.rs` (player verified live on Windows audio hardware)

---

## Phase 4 — Asset Management
- [x] Add `tpt-cadence` as external git dependency — pushed to
  `https://github.com/tpt-solutions/tpt-cadence` (origin `master` @ `ff0bd6b`); the four
  codec crates (`tpt-av-cadence-core`/`wav`/`aiff`/`flac`) are now `git` deps in the root
  `Cargo.toml` behind the `tpt-av-audio-core/cadence` feature (path deps on the sibling
  checkout retired). `deny.toml`'s `sources.allow-git` updated accordingly.
- [x] Integrate `tpt-cadence` decode calls into asset loading (Main Thread)
  (WAV/AIFF/FLAC decode through cadence's real-time-safe `Decoder` contract via
  `DecodeRegistry` under the `cadence` feature; hound WAV remains the no-feature fallback.
  Integration surfaced and fixed a real race: concurrent `AssetStore::insert` calls could
  lose updates — writes are now serialized, readers stay lock-free)
- [x] Pre-allocated PCM caches per `AudioAsset` (`core::asset::AssetStore`)
- [x] Background thread pool for decoding/caching (`core::pool::DecodePool`)
- [x] Ring buffer handoff between decoder thread and audio thread (lock-free, bounded) (`core::ring::SpscRing`)
- [x] Tests: asset cache correctness, decode-thread → audio-thread handoff under load
  (including a 200k-frame concurrent producer/consumer no-loss test)

---

## Phase 5 — Plugin Hosting (Future)
- [x] Scaffold `tpt-av-audio-plugin` crate (`HostedPlugin`, `ParameterSet`, `ParameterAutomation`, `BusLayout`, `BusRouter`, `SidechainDucker`)
- [ ] VST3 support — **blocked, will not implement:** the Steinberg VST3 SDK
  is GPLv3-or-proprietary dual-licensed, incompatible with this workspace's
  MIT/Apache-2.0-only `deny.toml` policy. Decision recorded 2026-09-21.
- [x] CLAP support via `clack-host` (crates.io, `MIT OR Apache-2.0`, behind
  the `clap` feature on `tpt-av-audio-plugin`/`tpt-av-audio`):
  `ClapPluginNode` in `tpt-av-audio-plugin/src/clap_host.rs` loads a `.clap`
  bundle (explicit path, first plugin in the entry), activates + starts it
  as a fixed-channel audio effect, snapshots its `params` extension into
  `ParameterInfo`, and bridges `AudioBuffer`'s interleaved layout to CLAP's
  per-channel buffers via pre-allocated scratch (allocation-free `process`).
  `set_parameter` queues a `ParamValueEvent`, flushed per block (not
  sample-accurate — a later enhancement). `#![forbid(unsafe_code)]` on the
  crate was narrowed to `#![deny(unsafe_code)]` + a scoped `#[allow]` on
  this one module, since loading a `.clap` dynamic library is inherently
  unsafe (documented in the module's doc comment). `HostedPluginAdapter` in
  the umbrella crate (`tpt-av-audio/src/plugin_bridge.rs`) wraps any
  `HostedPlugin` as an `AudioNode` for the core graph — lives in the
  umbrella crate, not `tpt-av-audio-core`, to avoid a core→plugin-crate
  dependency. No MIDI/note input, no plugin directory scanning, no GUI
  hosting, and no graceful deactivation-on-drop yet (the `PluginInstance`
  main-thread handle is intentionally dropped once the `Send`-able
  `StartedPluginAudioProcessor` is obtained — a documented, non-UB leak
  per clack-host's own `Drop` impl, not full lifecycle management) — all
  tracked as follow-ups. CI: `clap` feature job in `.github/workflows/ci.yml`
  (stable toolchain only — clack-host's MSRV 1.85 exceeds this workspace's
  1.75 floor, so it's excluded from the `msrv` job).
- [x] Plugin parameter automation (ties into `Envelope` model — clip-local or session time base)
- [x] Side-chaining and bus routing (`SidechainDucker`, `BusRouter`)

---

## Phase 6 — Legacy Cleanup & Release
- [x] Remove/archive retired `core`, `gui`, `desktop`, `platform-*` crates per Phase 0 decision (all under `legacy/`, excluded from workspace)
- [x] Update `wix/`, `packaging/linux`, `docs/SIGNING.md`, `docs/WEBSITE.md`, `QUICKSTART.md` — retired: installer/packaging/signing/website docs moved to `legacy/`; QUICKSTART rewritten for the library model
- [x] Update CHANGELOG.md with pivot notes and new crate versions
- [ ] Publish crates to crates.io under `tpt-av-audio-*` namespace (utils → timeline → core → io → plugin, dependency order)
  **Needs a human:** requires crates.io credentials; `cargo publish -p tpt-av-audio-utils` first, then the rest in dependency order.
- [x] Final README pass: vision, ecosystem diagram, crate table, quickstart for library consumers
- [ ] Tag `0.1.0` release across workspace
  **Needs a human:** tag after publishing (e.g. `git tag v0.1.0` on the release commit).

---

## Review Pass (2026-09-15) — adoption & hardening
- [x] `tpt-av-audio` umbrella crate: `Engine` facade (load_asset / queue_asset /
  spawn_decode_workers / insert_pcm / render / seek), `play_file` +
  `play_file_blocking`, `offline::render_session_to_wav` / `render_with_store`
- [x] Ergonomics: `Clip::new` + `with_fades`/`with_source_offset`/`with_volume_envelope`
  builders, `Session::save`/`load`, `AudioBuffer::peak`/`rms`/`combined_peak`,
  `DecodedAudio::frames`
- [x] Stub sweep: no TODO/FIXME markers; PipeWire streams implemented (above);
  CoreAudio + Archon stubs remain honestly documented (upstream-blocked)
- [x] Robustness: poison-tolerant mutex handling in `NullBackend`/`DecodePool`;
  fallible `DecodePool::new`; fixed `AssetStore` concurrent-insert lost-update race
  (writes serialized, readers stay lock-free)
- [x] Security audit: `cargo audit` clean (RustSec, 54 deps, `--deny warnings` exit 0);
  cargo-deny extended with advisories (yanked denied) + sources (crates.io only);
  `#![forbid(unsafe_code)]` on utils/timeline/plugin; unsafe confined to WASAPI COM
  interop + ring buffer with documented invariants; `unsafe_op_in_unsafe_fn` denied in io/core
- [x] Adoption tooling: SECURITY.md, CONTRIBUTING.md, MSRV 1.75 CI job, full
  `cargo-deny` security job, Dependabot (Actions), README badges + facade-first quickstart

## Phase 7 — Engine Polish & DAW Features (tracked 2026-09-15)
- [x] **Channel matrix in the renderer**: downmix/upmix between asset and bus
  channel counts — mono→stereo fill, constant-power stereo→mono fold-down,
  proportional-block N→M (`dsp::channels`, fused with resampling in a single
  allocation-free pass; the RT-safety audit caught a Vec-per-frame first draft
  and forced the plan-once rewrite)
- [x] **Real-time metering node**: `dsp::meter` `MeterNode` + `Meter` —
  pass-through peak (with release ballistics) and RMS published via atomics
- [x] **Waveform overview**: `overview::WaveformOverview` min/max peak buckets
  with zoom windows (`min_max_window`); cached per (asset, rate) via
  `Engine::overview`
- [x] **Clip loop regions**: `loop_start`/`loop_end` (serde-defaulted, builder
  + validated `set_loop_region`), wrap math in `Clip::source_position`,
  renderer support via the placed resampler, split keeps the region only on
  the half that fully contains it
- [x] **Crossfades**: `FadeCurve` (linear/equal-power) on clip fades,
  overlap-aware undoable `CrossfadeEdit` (pulls the right clip over the
  left tail and sets matching equal-power fades), and a `History`-backed
  `Engine::apply_edit`/`undo`/`redo`. Building this exposed and fixed a
  latent renderer flaw: overlapping clips on the same track used to
  overwrite each other instead of summing (clips now render into a
  clip-scratch and add, so fades shape the mix)
- [x] **WAV export options**: `WavExportFormat` (16/24-bit PCM, 32-bit float)
  in the offline renderer — also fixed tail over-write past session duration
- [x] **docs.rs metadata + doc-example coverage**: `[package.metadata.docs.rs]`
  all-features on every crate; facade crate documents the flagship flow
- [x] **CI job for the cadence feature**: added a `cadence` matrix job
  (windows-latest/ubuntu-latest) to `.github/workflows/ci.yml` running
  `cargo build`/`cargo test --workspace --features tpt-av-audio-core/cadence`,
  now that tpt-cadence is pushed
- [x] **Shared `tpt-av-test` real-time harness wired in**: `tpt-av-audio-core/tests/real_time_harness.rs`
  runs the `dsp::graph` gain→pan→fade chain through `tpt-av-test-benchmark`'s
  `TrackingAllocator`/`assert_real_time_safe!`, mirroring the hand-rolled
  `rt_safety.rs` audit through the cross-repo harness. Wired as a `git` dep
  (`https://github.com/tpt-solutions/tpt-av-test`, pushed, `tpt-av-test-benchmark`
  crate) via `[workspace.dependencies]`, same pattern as tpt-cadence; added to
  `deny.toml`'s `sources.allow-git`, plus `[bans] allow-wildcard-paths = true`
  since cargo-deny's wildcard lint flags git deps with no `version =` pin
- [x] **Musical time helpers**: `timeline::musical` — beat/bar lengths
  (quarter-note tempo convention, denominator-aware beat units),
  `beat_at_frame`/`frame_at_beat`, 1-based `bar_and_beat`,
  `frame_at_bar_beat`, and `snap_to_grid` (bar/beat/half/quarter);
  degrades gracefully when tempo is unset

## Open Questions / Risks
- ~~`gui`/`desktop` retirement destination not yet chosen (Phase 0)~~ — **resolved:** archived under `legacy/` (reversible: can be split to a new repo later if desired)
- ~~`tpt-cadence` repo availability is an external blocker for Phase 4~~ — **resolved:** pushed to GitHub, now a git dependency; opus/aac/mp3/vorbis codecs remain in progress upstream but are outside this repo's scope
- `platform-archon` was research-gated/blocked upstream even in the old repo — **still blocked**; the capability model survives in `tpt-av-audio-io/src/backend/archon.rs`
- PipeWire stream playback/capture needs the native `pipewire-rs` port (enumeration works via `pw-dump`)
- CoreAudio backend is a stub until a macOS implementation (AUHAL) lands
