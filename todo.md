# tpt-audio — Upgrade Checklist (Engine Pivot)

Dual-licensed MIT / Apache-2.0 — TPT Solutions

> Pivoting `tpt-audio` from the router/mixer app (`core`/`gui`/`desktop`/`platform-*`) into
> the `tpt-av-audio-*` non-destructive engine workspace described in `spec2.txt`. New crates
> are scaffolded incrementally; old crates keep building until each is fully migrated or
> retired. License stays dual MIT/Apache-2.0 (overrides spec2's "pure MIT" wording);
> `tpt-cadence` / `tpt-kinetix` are tracked only as external git dependencies, not built here.

---

## Phase 0 — Pivot Setup & Decisions
- [ ] Confirm/record this pivot decision in README + CHANGELOG (router app → audio engine library)
- [ ] Decide fate of `gui` + `desktop` (router UI): archive in this repo under `legacy/`, split to a new repo, or delete outright
- [ ] Update root `Cargo.toml` license field to `"MIT OR Apache-2.0"` (keep dual, do not follow spec2's MIT-only text)
- [ ] Update README.md badges/description for new engine positioning; keep dual-license notice
- [ ] Create `deny.toml` (cargo-deny) with allow list `MIT, Apache-2.0, BSD-2-Clause, BSD-3-Clause, ISC, Zlib`; deny `GPL-2.0, GPL-3.0, LGPL-2.1, LGPL-3.0, AGPL-3.0, MPL-2.0`
- [ ] Add cargo-deny check to CI workflow (`.github/workflows`)
- [ ] Scaffold new workspace members in root `Cargo.toml`: `tpt-av-audio-utils`, `tpt-av-audio-timeline`, `tpt-av-audio-core`, `tpt-av-audio-io`, `tpt-av-audio-plugin`
- [ ] Add `[workspace.package]` fields: version `0.1.0`, edition `2021`, license `"MIT OR Apache-2.0"`, rust-version `1.75`
- [ ] Rename/retire `spec.txt` (old router spec) — keep for history or move to `legacy/`
- [ ] Decide whether existing `platform-archon` research/stub content is preserved for later I/O backend work

---

## Phase 1 — Foundation & Timeline Model
- [ ] Scaffold `tpt-av-audio-utils` crate (`lib.rs`, `sample.rs`, `buffer.rs`, `time.rs`, `error.rs`)
  - [ ] `Sample` types (f32, i16, etc.)
  - [ ] `AudioBuffer`-adjacent buffer abstractions
  - [ ] Time/duration types (frames, seconds, samples) with conversions
  - [ ] `AudioError` enum
- [ ] Scaffold `tpt-av-audio-timeline` crate (`lib.rs`, `session.rs`, `track.rs`, `clip.rs`, `asset.rs`, `envelope.rs`, `edit.rs`, `history.rs`)
  - [ ] `Session`, `Track`, `Clip`, `AudioAsset` structs per spec §4.1
  - [ ] `Envelope` / `EnvelopePoint` + `InterpolationMethod` (linear, cubic, step)
  - [ ] Edit operations: insert, delete, move, split
  - [ ] Undo/redo history
  - [ ] Unit tests: clip splitting, envelope interpolation math
- [ ] Wire up cargo-deny CI pipeline enforcing license policy (verify it actually runs on these new crates)

---

## Phase 2 — I/O Layer Integration
- [ ] Scaffold `tpt-av-audio-io` crate (`lib.rs`, `device.rs`, `stream.rs`, `backend/`, `router.rs`)
- [ ] Port WASAPI logic from `platform-windows` → `tpt-av-audio-io/src/backend/wasapi.rs`
- [ ] Port PipeWire logic from `platform-linux` → `tpt-av-audio-io/src/backend/pipewire.rs`
- [ ] Add/port CoreAudio backend stub → `tpt-av-audio-io/src/backend/coreaudio.rs` (macOS, new — not in old repo)
- [ ] Decide fate of `platform-archon` (research-gated) — port as future backend or shelve per Phase 0 decision
- [ ] Define `AudioDevice`, `AudioStream`, `enumerate_devices()` per spec §4.3 (replaces old `AudioBackend` trait)
- [ ] Migrate device enumeration + stream start/stop tests from old `platform-*` crates
- [ ] Integration tests for device enumeration and stream management
- [ ] Retire old `platform-windows`, `platform-linux`, `platform-archon` crates once ported (remove from workspace members)

---

## Phase 3 — Real-Time Mixer
- [ ] Scaffold `tpt-av-audio-core` crate (`lib.rs`, `graph.rs`, `mixer.rs`, `renderer.rs`, `dsp/`, `scheduler.rs`)
- [ ] `AudioGraph` + `AudioNode` trait (real-time safe: allocation-free, lock-free, panic-free `process()`)
- [ ] `AudioBuffer` (interleaved f32) type — reconcile with `tpt-av-audio-utils` buffer types
- [ ] Multi-track `mixer.rs`
- [ ] `TimelineRenderer`: reads timeline snapshot, fetches PCM, applies envelopes, mixes, advances playhead
- [ ] Built-in DSP: `gain.rs`, `pan.rs`, `fade.rs`
- [ ] `resample.rs` wrapping `rubato` for sample rate conversion
- [ ] Lock-free state sync: `TimelineState` (double-buffered snapshot, `AtomicPtr`/`AtomicUsize`)
- [ ] Real-time safety audit: confirm no heap allocation/locking/panics on the audio-thread path (document + add a lint/test guard, e.g. `#[no_alloc]`-style check or allocation-counting test)
- [ ] `examples/headless_render.rs`: renders a timeline JSON to WAV
- [ ] `examples/simple_player.rs`, `examples/mixer_demo.rs`

---

## Phase 4 — Asset Management
- [ ] Add `tpt-cadence` as external git dependency (once available) — *no in-repo build work on tpt-cadence itself*
- [ ] Integrate `tpt-cadence` decode calls into asset loading (Main Thread)
- [ ] Pre-allocated PCM caches per `AudioAsset` (`AudioAssetCache`)
- [ ] Background thread pool for decoding/caching
- [ ] Ring buffer handoff between decoder thread and audio thread (lock-free, bounded)
- [ ] Tests: asset cache correctness, decode-thread → audio-thread handoff under load

---

## Phase 5 — Plugin Hosting (Future)
- [ ] Scaffold `tpt-av-audio-plugin` crate
- [ ] VST3 support via `nih-plug`
- [ ] CLAP support via `nih-plug`
- [ ] Plugin parameter automation (ties into `Envelope` model)
- [ ] Side-chaining and bus routing

---

## Phase 6 — Legacy Cleanup & Release
- [ ] Remove/archive retired `core`, `gui`, `desktop`, `platform-*` crates per Phase 0 decision
- [ ] Update `wix/`, `packaging/linux`, `docs/SIGNING.md`, `docs/WEBSITE.md`, `QUICKSTART.md` — retire or rewrite for library-not-app distribution model
- [ ] Update CHANGELOG.md with pivot notes and new crate versions
- [ ] Publish crates to crates.io under `tpt-av-audio-*` namespace (utils → timeline → core → io → plugin, dependency order)
- [ ] Final README pass: vision, ecosystem diagram, crate table, quickstart for library consumers
- [ ] Tag `0.1.0` release across workspace

---

## Open Questions / Risks
- `gui`/`desktop` retirement destination not yet chosen (Phase 0)
- `tpt-cadence` repo availability/API stability is an external blocker for Phase 4
- `platform-archon` was research-gated/blocked upstream even in the old repo — likely stays blocked for the new `tpt-av-audio-io` Archon backend too
