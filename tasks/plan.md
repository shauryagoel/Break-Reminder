# Implementation Plan: `configuration`

## Overview

Implement the approved [configuration spec](../SPEC-configuration.md) first. The result is a Rust package that creates and validates `~/.config/break-reminder/config.yaml` and exposes a validated settings value to the later timer and window modules. This plan covers only `configuration` from the [capability map](../CAPABILITY-MAP.md).

## Architecture decisions

- Use `serde-saphyr`'s typed reader API with a 64 KiB input budget. Private deserialization structs use defaults and reject unknown fields at every nesting level; parser options reject unsupported tags. A validation step converts them into settings with explicit `Duration` values and a resolved optional image path.
- Keep the public config types free of GUI dependencies. The later modules receive interval, display duration, ordered postpone durations, appearance values, and image fit through the validated config.
- Embed `assets/default-config.yaml` with `include_str!` and write it with `create_new` so first run cannot replace a user's file.
- Parse the two CLI flags with the standard library. `--check-config` is the only runnable mode during this module; normal runtime and tray integration belong to later modules.
- Keep loading pure from the caller's perspective: return a complete config or an error. The future reload caller swaps its active value only on success.

## Task list

### Phase 1: Runnable configuration path

1. [x] Bootstrap the Rust package, sample YAML, typed loading, and `--check-config` command. Verify the sample is created once and partial YAML uses defaults.

### Phase 2: Validation and diagnostics

2. [x] Enforce the full YAML contract, image path rules, size limit, and actionable errors. Verify invalid reload leaves a previously loaded config usable.

### Checkpoint: Configuration complete

- [x] `cargo test --all-targets` passes.
- [x] `cargo fmt --all -- --check` passes.
- [x] `cargo clippy --all-targets -- -D warnings` passes.
- [x] `cargo build --release` succeeds on macOS.
- [x] Manual `cargo run -- --check-config --config ./assets/default-config.yaml` succeeds; an invalid temporary config fails clearly without opening a window.
- [x] Every success criterion in `SPEC-configuration.md` is satisfied or its later X11 verification is explicitly tracked.

## Risks and mitigations

| Risk | Impact | Mitigation |
|---|---|---|
| YAML parser defaults differ from the intended strict schema | Misspelled or duplicate values could be accepted | Cover duplicate and unknown keys with executable tests before accepting the parser integration |
| Two simultaneous first launches read the new file while it is still being written | One instance may temporarily report an invalid sample | `create_new` prevents overwrite; use atomic publication if simultaneous startup becomes a requirement |
| Bad image path or invalid reload interrupts scheduling | App could lose working settings | Validate the path before returning a config; future caller retains the previous value on error. Image decode and no-image fallback belong to the window module |
| X11 compilation differs from macOS | Cross-platform claim is incomplete | Keep config platform-neutral and perform Linux/X11 build and runtime checks in the later window module |

## Open questions

None. The written YAML contract is approved.
