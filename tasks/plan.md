# Implementation Plan: `reminder-timing`

## Overview

Implement the approved [timing spec](../SPEC-reminder-timing.md) as a pure Rust module. It accepts validated durations from `configuration`, tracks one reminder, and exposes deadlines and state transitions for the later tray/overlay app shell. This plan covers only `reminder-timing` from the [capability map](../CAPABILITY-MAP.md). The completed configuration plan is archived in `tasks/configuration-plan.md`.

## Architecture decisions

- Keep one timer state: waiting for a deadline, manually paused with a remaining duration, launching an overlay, or showing a break. A due check moves waiting to launching once; only the first terminal outcome schedules the next deadline.
- Pass `Instant` into transitions and calculate remaining time with saturation. Tests advance synthetic instants rather than sleeping. The display countdown begins when all overlay windows are visible, not when the child process starts.
- Use only Rust `Duration` and `Instant` in this module. The later `reminder-window` module owns `winit` waiting, tray events, overlay process execution, and error presentation.
- A successful reload changes the interval and resets the waiting or paused remainder; if an overlay is launching or showing, the new interval applies after it ends.

## Task list

### Phase 1: Due and break completion

1. [ ] Add a pure timer with due/launch/visible states, countdown calculation, and one-shot completion for timeout, Skip, Postpone, and failed launch.

### Phase 2: User control and reload

2. [ ] Add Pause/Resume with frozen remainder and the approved reload behavior in every state.

### Checkpoint: Timing complete

- [ ] `cargo test --all-targets` passes with deterministic state-transition tests.
- [ ] `cargo fmt --all -- --check` passes.
- [ ] `cargo clippy --all-targets -- -D warnings` passes.
- [ ] `cargo build --release` succeeds on macOS.
- [ ] Every local timing success criterion in `SPEC-reminder-timing.md` passes; app-shell, sleep/wake, and X11 runtime checks remain tracked for `reminder-window`.

## Risks and mitigations

| Risk | Impact | Mitigation |
|---|---|---|
| A late timer wake queues several overdue breaks | Multiple overlays could launch | One state transition from waiting to launching; test a long late gap |
| A button action and child exit both complete one break | Postpone could be overwritten | Ignore terminal outcomes after the first; test duplicate completion |
| Launch time consumes the display duration | Break is shorter than configured | Start countdown only after all windows are visible; test the timing calculation |
| Rust changes `Instant` suspend behavior | Sleep could consume the remaining time | Verify a real macOS sleep/wake cycle during window integration; pin or replace the clock if that check fails |

## Open questions

None for the pure timer. The real overlay-ready signal is part of the later window integration.
