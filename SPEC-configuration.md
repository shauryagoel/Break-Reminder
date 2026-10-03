# Spec: `configuration`

## Objective

Provide one human-editable YAML file for all first-release break timing, media-pause, and appearance settings. A user can change the reminder interval, display duration, visible postpone choices, optional media pause, title, message, colors, and optional image without recompiling. The module returns validated values to `reminder-timing` and `reminder-window`; it does not own either UI or scheduling.

Default path on macOS and X11: `~/.config/break-reminder/config.yaml`. `--config PATH` overrides this path for development and testing; a missing file at either path gets the sample. The approved [capability map](CAPABILITY-MAP.md) defines this module as a dependency of the other two.

## YAML contract

The generated sample has these effective defaults:

```yaml
interval_minutes: 60
duration_seconds: 30
postpone_minutes: [10, 15]
pause_media: false
appearance:
  title: "Time for a break"
  message: "Step away from your screen and rest your eyes."
  background_color: "#101827"
  text_color: "#F8FAFC"
  accent_color: "#69D5B2"
  # image:
  #   path: "~/Pictures/break.png"
  #   fit: contain # contain or cover
```

Any omitted field uses its sample default. Unknown or duplicate keys and unsupported YAML tags are errors, so misspelled settings do not silently disappear. A YAML document is limited to 64 KiB and may not load other files through YAML tags. All durations are integer values. `interval_minutes` and each `postpone_minutes` value must be 1–1440; `duration_seconds` must be 1–3600. The postpone list must contain 1–12 distinct values; order is preserved for button order. Colors use `#RRGGBB`. Title and message must be nonempty after trimming. The optional image path can be absolute, relative to the config directory, or start with `~/`; it is resolved to an absolute path. If an `image` mapping is present, `path` is required and nonempty; `fit` defaults to `contain` and may be `contain` or `cover`. A missing or unreadable image does not invalidate the configuration: `reminder-window` reports the failure and renders its no-image fallback so reminders continue.

On first launch, create the directory and sample file only if the file is absent. Never overwrite an existing file. Remove a newly created sample if writing it fails. Parsing or validation errors report the file path and offending field or YAML location. `load` returns a complete validated config or an error. At app startup, a load error falls back to the sample defaults and reports the error in the menu and on stderr; the configuration diagnostic remains until a successful reload. On reload, the caller keeps its previous config if loading fails. `--check-config` remains strict and exits nonzero on a load error.

`pause_media` is a boolean, defaulting to `false` even in existing files that omit it. The setting is captured when each break launches; reload affects future breaks. See [README Settings](README.md#settings) for supported platforms, media behavior, permissions, and diagnostics.

## Tech stack

- Rust 2024 edition. This module's dependency floor is Rust 1.89 through [`serde-saphyr` 1.3](https://docs.rs/serde-saphyr/latest/serde_saphyr/); the complete app raises the package minimum to 1.95 for `reminder-window`.
- `serde` 1.x with `derive` for typed fields.
- `serde-saphyr` 1.3.x with its `deserialize` feature for this module; `reminder-window` may also enable its `serialize` feature for a validated parent-to-child settings snapshot. Use its [reader API and input budget](https://docs.rs/serde-saphyr/latest/serde_saphyr/fn.from_reader_with_options.html) to enforce the 64 KiB limit, its default duplicate-key rejection, and `reject_unsupported_tags: true`. Apply `#[serde(deny_unknown_fields)]` to every YAML mapping type, including nested appearance and image mappings.
- Rust standard library for paths, file creation, and post-parse validation. No config framework, YAML value tree, or file watcher.

## Commands

These commands become executable once this module is implemented:

```sh
cargo run -- --check-config
cargo run -- --check-config --config ./assets/default-config.yaml
cargo test --all-targets
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo build --release
```

`--check-config` validates and prints the resolved config path and a success or actionable error without opening the tray or an overlay.

## Project structure

```text
Cargo.toml                  Rust package and dependency versions
src/main.rs                 CLI path selection and --check-config entry point
src/config.rs               Typed YAML loading, defaults, validation, path resolution, tests
assets/default-config.yaml  Commented sample copied on first launch
```

The later `reminder-timing` and `reminder-window` specs will name their own source files. Tests for this module live beside its parsing and validation code in `src/config.rs`.

## Code style

Use `rustfmt`, `snake_case` for functions and YAML fields, explicit units in names, and typed errors rather than panics in the load path. Keep parsing separate from semantic validation. For example:

```rust
fn interval(value: u32) -> Result<Duration, &'static str> {
    if !(1..=1_440).contains(&value) {
        return Err("interval_minutes must be between 1 and 1440");
    }
    Ok(Duration::from_secs(u64::from(value) * 60))
}
```

## Testing strategy

Use Rust's built-in test runner. Focused tests cover sample defaults, partial overrides, image path resolution, blank image paths, acceptance of missing image files for window fallback, malformed YAML, duplicate/unknown nested keys, unsupported tags, every numeric boundary, empty and repeated postpone values, bad colors, and the 64 KiB input limit. File-system tests use temporary paths to confirm the first-run sample is created without overwriting an existing config; a separate test checks default path resolution. The CLI check verifies that validation can run without creating a window. No coverage percentage target; every validation branch must have a check.

## Boundaries

- **Always:** Validate before exposing settings; preserve the existing file; return actionable errors; run `cargo test --all-targets`, `cargo fmt --all -- --check`, and `cargo clippy --all-targets -- -D warnings` before marking this module done.
- **Ask first:** Change the YAML location or a field's meaning; add dependencies outside this approved stack; introduce a breaking YAML schema change.
- **Never:** Enable file includes or command execution through YAML tags; silently accept invalid or unknown values; overwrite an existing user config; write settings outside the selected config path.

## Success criteria

1. First launch creates the sample at the default path; later launches preserve edits byte-for-byte.
2. A partial YAML file produces complete validated settings with documented defaults, and image paths resolve as specified.
3. Invalid settings, duplicate keys, and oversized input fail with actionable errors; an invalid reload does not replace the caller's last valid config.
4. `--check-config` succeeds or reports failure without opening a window.
5. Tests, formatting, linting, and release build commands pass on macOS. The configuration code compiles for X11/Linux in the later platform verification step.

## Open questions

None for this module. The visual rendering of colors and images, and error display in the tray, belong to `reminder-window`.
