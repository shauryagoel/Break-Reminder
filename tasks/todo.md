# Tasks: `reminder-timing`

## Task 1: Due and break completion

**Description:** Add the pure timer state and countdown calculation. Handle one due event, overlay readiness, and a single terminal result that starts either a full interval or a selected postpone delay.

**Acceptance criteria:**

- [ ] Startup is due after one configured interval; before the deadline it does nothing, and a late wake starts only one launch.
- [ ] Display countdown starts at overlay visibility, reaches zero after the configured duration, and never becomes negative.
- [ ] Timeout, Skip, failed launch, and an overlay that closes without an action start a full interval; Postpone uses its relative delay; a later duplicate completion cannot replace the chosen deadline. The app shell reports failures during integration.

**Verification:**

- [ ] Write a failing `src/timing.rs` test before implementation, then run `cargo test timing::tests` to green.
- [ ] Run `cargo test --all-targets`, `cargo fmt --all -- --check`, and `cargo build --release`.

**Dependencies:** Completed `configuration` module.

**Files likely touched:** `src/timing.rs`, `src/main.rs` (2).

## Task 2: Pause, Resume, and reload

**Description:** Preserve the remainder across manual Pause/Resume and apply a new interval on successful config reload according to the timer state.

**Acceptance criteria:**

- [ ] Pause freezes the remaining interval and Resume continues it; both are unavailable during overlay launch/display.
- [ ] Reload resets a waiting timer, updates a paused remainder while staying paused, and applies after an overlay ends in both launching and showing states without changing that overlay's original display duration.
- [ ] Invalid reload does not call the timer and leaves its state unchanged; this caller behavior is tracked in `reminder-window` integration.

**Verification:**

- [ ] Write failing state-transition tests before implementation; run `cargo test timing::tests` and `cargo test --all-targets`.
- [ ] Run `cargo fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo build --release`.

**Dependencies:** Task 1.

**Files likely touched:** `src/timing.rs` (1).

## Checkpoint: Timing complete

- [ ] Both tasks meet their local acceptance criteria and the pure module builds on macOS.
- [ ] `reminder-window` carries the remaining overlay-ready, sleep/wake, invalid-reload, and X11 runtime checks.
- [ ] The module is ready for review before `reminder-window` begins.
