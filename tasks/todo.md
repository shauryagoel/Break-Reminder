# Tasks: `configuration`

## Task 1: Runnable configuration path

**Description:** Create the Rust package and a usable config check command. On first use, create the sample YAML without overwriting an existing file. Parse sample and partial YAML into defaults.

**Acceptance criteria:**

- [ ] `cargo run -- --check-config` creates `~/.config/break-reminder/config.yaml` when absent and validates it without opening a window.
- [ ] `cargo run -- --check-config --config ./assets/default-config.yaml` reads that path. A missing custom path in a temporary directory gets the sample, and repeated calls never overwrite its contents.
- [ ] Omitted YAML fields inherit the documented defaults.

**Verification:**

- [ ] `cargo test --all-targets` checks sample parsing, partial overrides, and create-new behavior using temporary paths.
- [ ] `cargo run -- --check-config --config ./assets/default-config.yaml` reports the resolved path and success.
- [ ] `cargo fmt --all -- --check` and `cargo build --release` pass.

**Dependencies:** None.

**Files likely touched:** `Cargo.toml`, `src/main.rs`, `src/config.rs`, `assets/default-config.yaml` (4).

## Task 2: Validate the full YAML contract

**Description:** Add semantic validation, bounded parsing, image path resolution, and clear errors. Keep loading atomic for future menu reloads.

**Acceptance criteria:**

- [ ] Invalid syntax, unknown/duplicate nested keys, unsupported tags, out-of-range durations, empty/duplicate postpone choices, bad colors, empty text, nonexistent or non-file image paths, and files over 64 KiB return actionable errors.
- [ ] Relative and `~/` image paths resolve as documented; postpone order is preserved.
- [ ] A failed second load returns an error without altering a previously returned valid config.

**Verification:**

- [ ] `cargo test --all-targets` exercises each validation branch and path form.
- [ ] `cargo run -- --check-config --config ./assets/default-config.yaml` succeeds; a temporary invalid YAML file exits unsuccessfully with its path and reason and opens no window.
- [ ] `cargo fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo build --release` pass.

**Dependencies:** Task 1.

**Files likely touched:** `src/config.rs`, `src/main.rs` (2).

## Checkpoint: Configuration complete

- [ ] Tasks 1 and 2 meet their acceptance criteria.
- [ ] All success criteria in `SPEC-configuration.md` pass on macOS, with X11 verification tracked for later.
- [ ] The module is ready for review before `reminder-timing` begins.
