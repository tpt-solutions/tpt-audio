# Contributing to tpt-audio

This is a solo-maintained project. Pull requests are not accepted — please
open an issue instead (bug report, feature request, or question) and it
will be triaged from there.

The rest of this document covers the internal rules that keep the workspace
coherent.

## Ground rules

1. **Dual MIT OR Apache-2.0 licensing.** Your contributions ship under both.
2. **No copyleft dependencies.** `cargo deny check` (licenses, advisories,
   bans, sources) must pass — see `deny.toml` and `SECURITY.md`.
3. **Real-time safety is non-negotiable.** Code that runs on the audio
   thread (`AudioNode::process`, `TimelineRenderer::render`, SPSC ring
   reads/writes) must be allocation-free, lock-free, and panic-free. The
   counting-allocator test `tpt-av-audio-core/tests/rt_safety.rs` enforces
   the allocation rule; code review enforces the rest. Do blocking work on
   the Main Thread (`prepare`/`update` entry points) instead.

## Development

```bash
cargo build --all
cargo test  --all                    # built-in WAV fallback, no sibling needed
cargo test  --all --features tpt-av-audio-core/cadence   # real tpt-cadence decoders
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo deny check                     # licenses + advisories + bans + sources
```

The `cadence` feature uses path dependencies on a sibling
`../tpt-cadence` checkout; CI runs without it. Formatting is
`rustfmt.toml` (100 cols); clippy warnings deny the build.

## Layout notes

- Crates are `tpt-av-audio-*`; `tpt-av-audio` is the umbrella facade —
  user-facing conveniences go there, engine internals stay in the
  workspace crates.
- The old router app previously archived under `legacy/` has moved to its
  own repository, [`tpt-mixer`](https://github.com/tpt-solutions/tpt-mixer).
- Public APIs need doc comments with runnable `# Examples` where practical;
  `no_run` is fine for I/O.

## Commits

Imperative-mood subject lines that name the area, e.g.
`Implement PipeWire streams via pw-cat`, `Fix AssetStore insert race`.
Keep bodies to the *why*.
