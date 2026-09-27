# Spec: `reminder-timing`

## Objective

Schedule one recurring break using the validated durations from [`configuration`](SPEC-configuration.md). The user can let a break end, Skip it, Postpone it by one of the configured delays, or Pause/Resume between breaks. Timing logic is independent of the menu, overlay rendering, and platform window APIs. The approved [capability map](CAPABILITY-MAP.md) makes this module the provider of timer state for `reminder-window`.

## Timing contract

| Event | Result |
|---|---|
| App starts | First reminder is due after one full `interval`. |
| Reminder becomes due | Enter a launching state and request one overlay; do not start another reminder while it launches or shows. |
| Overlay is visible on every monitor | Start the display countdown from that moment, so launch and first-paint time do not reduce the configured duration. |
| Display duration expires | The overlay reports an elapsed outcome; end the break and start a full interval from that moment. |
| Skip is clicked | End the break immediately and start a full interval from the click. |
| Postpone is clicked | End the break immediately; the next reminder is due after that choice's relative delay from the click. |
| Pause is clicked between breaks | Freeze the remaining interval. No reminder appears while paused. |
| Resume is clicked | Continue with exactly the frozen remaining interval. |
| Valid config is reloaded | While waiting, start a full new interval from reload. While manually paused, stay paused with the new full interval stored. An overlay already launching or showing keeps its original display duration, then starts the new interval when it ends. |
| Invalid config reload | Keep the current timer and settings unchanged. |
| Overlay launch fails or closes without an action | Start a full interval and report the error through the app shell; do not immediately relaunch. |
| App restarts | Start a new full interval; no schedule is persisted. |

The visible countdown reaches zero and never displays a negative value. If a timer wake is late, show only one reminder; there is no queue of missed breaks. The overlay child is the sole authority for ordering its timeout, Skip, and Postpone actions: it emits one terminal outcome. The parent applies that outcome once by invocation ID and ignores a later child exit. It accepts an elapsed outcome only after the overlay is visible and its display deadline has passed. The parent does not independently expire a visible break, because an earlier click could still be in transit. Pause/Resume is unavailable while the overlay is launching or showing.

On macOS and X11/Linux, system sleep does not consume the remaining interval or an active display countdown. Awake idle time and a locked screen do count. The current [`std::time::Instant` underlying clocks](https://doc.rust-lang.org/std/time/struct.Instant.html) use Darwin uptime and Linux monotonic time; [Apple](https://developer.apple.com/documentation/driverkit/mach_absolute_time) and the [Linux manual](https://man7.org/linux/man-pages/man3/clock_getres.3.html) describe both as excluding suspend. Rust does not guarantee that behavior for all future versions or platforms. A macOS sleep/wake runtime check is required before release.

## Tech stack

- Rust 2024 edition and standard-library timing. The complete app raises the package minimum from 1.89 to 1.95 for `reminder-window`.
- Rust standard library `Duration` and `Instant` for interval deadlines and countdown calculations. No scheduler crate or wall-clock arithmetic.
- The future windowless app shell uses [`winit` `ControlFlow::WaitUntil`](https://docs.rs/winit/latest/winit/event_loop/enum.ControlFlow.html) for an efficient wait between breaks, checks the deadline on event-loop wakes, and receives the overlay child's single terminal outcome through an event-loop proxy. It confirms or forces child closure before another launch. This integration belongs to `reminder-window`, not this module.

## Commands

```sh
cargo test timing::tests
cargo test --all-targets
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo build --release
```

## Project structure

```text
src/timing.rs    Pure timer state and countdown calculation, with unit tests
src/main.rs      Declares the module; app-shell wiring follows in reminder-window
```

## Code style

Use explicit state names, `Duration` for lengths of time, and `Instant` only for deadlines. Pass `now` into each transition so tests need no sleeps or clock mocks. Follow `rustfmt` and `snake_case`. For example:

```rust
fn remaining(deadline: Instant, now: Instant) -> Duration {
    deadline.saturating_duration_since(now)
}
```

## Testing strategy

Use Rust's built-in tests in `src/timing.rs` with supplied `Instant` values. Cover due boundaries, one launch per due event, full interval after the child's elapsed outcome and Skip, relative Postpone, Pause/Resume preserving the remainder, reload in waiting/paused/launching/active states, a countdown that begins only after visibility and clamps to zero, and first-terminal-result-only behavior. Test long elapsed gaps to ensure they never queue multiple reminders. Integration checks in `reminder-window` must verify that invalid config reload leaves the timer untouched, the child emits only one outcome, and child closure is confirmed before another overlay launches. Manually sleep and wake the macOS machine partway through a short interval and during a visible countdown; both must retain their pre-sleep remainder.

## Boundaries

- **Always:** Derive schedules from validated `Duration` values; use monotonic deadlines; keep tests deterministic; run tests, formatting, Clippy, and release build before marking the module done.
- **Ask first:** Change Skip/Postpone/Pause/reload semantics, add quiet hours or multiple break types, or persist schedule state across restarts.
- **Never:** Use `SystemTime` for intervals; spin or poll continuously; queue missed reminders; block the UI event loop while waiting for an overlay process.

## Success criteria

1. Timer transitions match every row in the timing contract, including one launch or active break at a time and one terminal outcome per overlay invocation.
2. Pause/Resume preserves the remaining interval; a successful reload resets it as specified; invalid reload leaves it unchanged.
3. Display countdown begins only after the overlay is visible, lasts the configured duration, and clamps at zero; late wakes cause no duplicate reminders.
4. `cargo test --all-targets`, `cargo fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo build --release` pass on macOS.
5. The later app-shell integration and macOS sleep/wake checks prove that real reminders follow the same behavior. X11 runtime verification remains required on an X11 machine.

## Open questions

None for this module. Platform event-loop and window details belong to `reminder-window`.
