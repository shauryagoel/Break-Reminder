# Tasks: `reminder-timing`

## Task 1: Due and break completion

**Description:** Add the pure timer state and countdown calculation. Handle one due event, overlay readiness, and a single terminal result that starts either a full interval or a selected postpone delay.

**Acceptance criteria:**

- [x] Startup is due after one configured interval; before the deadline it does nothing, and a late wake starts only one launch.
- [x] Display countdown starts at overlay visibility, reaches zero after the configured duration, and never becomes negative.
- [x] The child's elapsed outcome after the display deadline, Skip, failed launch, and an overlay that closes without an action start a full interval; premature elapsed is ignored. Postpone uses its relative delay; a later duplicate completion cannot replace the chosen deadline. The parent never expires a visible break independently.

**Verification:**

- [x] Write a failing `src/timing.rs` test before implementation, then run `cargo test timing::tests` to green.
- [x] Run `cargo test --all-targets`, `cargo fmt --all -- --check`, and `cargo build --release`.

**Dependencies:** Completed `configuration` module.

**Files likely touched:** `src/timing.rs`, `src/main.rs` (2).

## Task 2: Pause, Resume, and reload

**Description:** Preserve the remainder across manual Pause/Resume and apply a new interval on successful config reload according to the timer state.

**Acceptance criteria:**

- [x] Pause freezes the remaining interval and Resume continues it; both are unavailable during overlay launch/display.
- [x] Reload preserves a waiting deadline or paused remainder and updates future interval/display settings without changing an overlay's original display duration. Restart Timer uses the latest full interval, preserves a paused state, and is unavailable during overlay launch/display.

**Verification:**

- [x] Write failing state-transition tests before implementation; run `cargo test timing::tests` and `cargo test --all-targets`.
- [x] Run `cargo fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo build --release`.

**Dependencies:** Task 1.

**Files likely touched:** `src/timing.rs` (1).

## Checkpoint: Timing complete

- [x] Both tasks meet their local acceptance criteria and the pure module builds on macOS.
- [x] `reminder-window` carries the remaining overlay-ready, failure reporting, child-outcome ordering/closure, sleep/wake, invalid-reload, and X11 runtime checks.
- [x] The module is ready for review before `reminder-window` begins.
