# Security Policy

## Supported versions

The workspace is pre-1.0; only the latest `master` receives security fixes.
The archived router app under `legacy/` is **not supported** — it is kept
for history only.

## Reporting a vulnerability

Please report privately via [GitHub Security Advisories](https://github.com/tpt-solutions/tpt-audio/security/advisories)
("Report a vulnerability") rather than a public issue. Include:

- the crate (`tpt-av-audio-*`) and a minimal reproduction,
- whether the issue affects the **audio thread** (real-time path) — see below,
- your assessment of severity.

We aim to acknowledge reports within 7 days and will publish a fix and a
RustSec advisory (via the `cargo-audit` database) for confirmed issues.

## What matters most in this codebase

1. **Real-time safety is a security property here.** The audio-thread path
   (`AudioNode::process`, `TimelineRenderer::render`, `SpscRing`) must never
   allocate, lock, or panic — a stall is an audible failure and can cascade
   into device loss. Violations are treated as defects and guarded by
   `tpt-av-audio-core/tests/rt_safety.rs`.
2. **Unsafe code is confined** to the WASAPI backend (COM interop) and the
   SPSC ring buffer's `UnsafeCell` slot; each site documents its invariants.
   The `utils`, `timeline`, and `plugin` crates are `#![forbid(unsafe_code)]`.
3. **Untrusted input:** timeline JSON documents (`Session::load`) and audio
   files (through `tpt-cadence`/hound) are untrusted input. Fuzzing-friendly
   boundaries; treat malformed-input crashes as security bugs.

## Supply-chain policy

Enforced in CI (`cargo deny check`, see `deny.toml`):

- **licenses** — permissive-only allow list; GPL/LGPL/AGPL/MPL are denied.
- **advisories** — RustSec vulnerabilities and yanked crates are denied.
- **sources** — dependencies may only come from crates.io (unknown
  registries/git sources denied).
- **bans** — duplicate versions warned, wildcard versions denied.

GitHub Actions toolchains are kept current via Dependabot.

## Automated scanning

- `cargo-deny` runs on every push/PR (all four check groups).
- `cargo audit` can be run locally; the advisory DB is the same one the
  CI job uses.
