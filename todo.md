# tpt-audio — Upgrade Checklist (Engine Pivot)

Dual-licensed MIT / Apache-2.0 — TPT Solutions

> Pivoting `tpt-audio` from the router/mixer app (`core`/`gui`/`desktop`/`platform-*`) into
> the `tpt-av-audio-*` non-destructive engine workspace described in `spec2.txt`. New crates
> are scaffolded incrementally; old crates keep building until each is fully migrated or
> retired. License stays dual MIT/Apache-2.0 (overrides spec2's "pure MIT" wording);
> `tpt-cadence` / `tpt-kinetix` are tracked only as external git dependencies, not built here.
>
> **Status note (2026-09-14):** Phases 0–3, 5, and the local parts of 4/6 are done.
> Full workspace: `cargo test` 147 passing, clippy `-D warnings` clean, fmt clean,
> `cargo deny check licenses` passing. Remaining items are external blockers
> (`tpt-cadence`, crates.io publishing) or marked deferred.

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
- [ ] Add `tpt-cadence` as external git dependency (once available) — *no in-repo build work on tpt-cadence itself*
  **BLOCKED upstream:** `github.com/tpt-solutions/tpt-cadence` is spec-only today (no code, no API).
  The decoder abstraction (`decode::DecodeRegistry`) is cadence-ready: register its decoder factory and everything downstream works unchanged.
- [x] Integrate `tpt-cadence` decode calls into asset loading (Main Thread)
  (replaced by the built-in WAV decoder through the same `Decoder` trait the cadence backend will implement)
- [x] Pre-allocated PCM caches per `AudioAsset` (`core::asset::AssetStore`)
- [x] Background thread pool for decoding/caching (`core::pool::DecodePool`)
- [x] Ring buffer handoff between decoder thread and audio thread (lock-free, bounded) (`core::ring::SpscRing`)
- [x] Tests: asset cache correctness, decode-thread → audio-thread handoff under load
  (including a 200k-frame concurrent producer/consumer no-loss test)

---

## Phase 5 — Plugin Hosting (Future)
- [x] Scaffold `tpt-av-audio-plugin` crate (`HostedPlugin`, `ParameterSet`, `ParameterAutomation`, `BusLayout`, `BusRouter`, `SidechainDucker`)
- [ ] VST3 support via `nih-plug` — **re-scoped:** `nih-plug` is a plugin *development* framework and provides no hosting API; VST3 hosting needs a `vst3-sys`-based host (future)
- [ ] CLAP support via `nih-plug` — **re-scoped:** CLAP hosting should use `clack-host` (future)
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

## Open Questions / Risks
- ~~`gui`/`desktop` retirement destination not yet chosen (Phase 0)~~ — **resolved:** archived under `legacy/` (reversible: can be split to a new repo later if desired)
- `tpt-cadence` repo availability/API stability is an external blocker for Phase 4 — **still blocked; decoder registry keeps the seam ready**
- `platform-archon` was research-gated/blocked upstream even in the old repo — **still blocked**; the capability model survives in `tpt-av-audio-io/src/backend/archon.rs`
- PipeWire stream playback/capture needs the native `pipewire-rs` port (enumeration works via `pw-dump`)
- CoreAudio backend is a stub until a macOS implementation (AUHAL) lands
