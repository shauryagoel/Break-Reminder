# Implementation Plan: `reminder-window`

## Overview

Implement the approved [window spec](../SPEC-reminder-window.md) on top of the completed [configuration](../SPEC-configuration.md) and [timing](../SPEC-reminder-timing.md) modules. Prove the macOS covering window and AeroSpace behavior early, then wire the windowless menu-bar parent to a short-lived overlay child. Finish image/layout work and packaging after the core break flow is live.

## Dependency order and architecture decisions

```text
bounded child snapshot and one-screen overlay
  -> multi-monitor native placement and packaged Mac check
  -> windowless tray parent and first end-to-end break
  -> child failure and event-order hardening
  -> menu reload/pause controls
  -> finished UI and image support
  -> documentation and platform verification
```

- Keep `--check-config` intact. Add a private `--overlay` child mode. Its stdin has a length-prefixed settings snapshot and a later `START` line; stdout contains `READY` and at most one terminal action. Parse these with small Rust functions and bound inputs; no general IPC framework.
- In the child, first build a simple egui overlay and test actual Mac window behavior. Then add one viewport per monitor and macOS-only AppKit frame/level adjustment. Use the native accessory policy and package `LSUIElement=true`; test the installed AeroSpace before relying on a float rule. The X11 branch uses absolute monitor geometry and is explicitly provisional until runtime-tested there.
- The parent owns `Config`, `Timer`, menu items, and one child invocation. I/O workers forward ready, outcome, EOF, and exit events through a `winit` proxy; the event loop handles timer/menu transitions without waiting on pipes or processes. Keep the launch-time config snapshot so Postpone indices remain stable across a reload.
- Use the Rust `image` crate only for bounded local PNG/JPEG/WebP decoding. Draw the rest with egui and existing YAML colors; no web UI, Swift UI, extra theme system, or persistent renderer window.
- Raise package MSRV to 1.95 for eframe 0.36.2. Disable default `wgpu` and tray GTK features; use `glow`, X11, AccessKit, and KSNI. Eframe still pulls transitive Wayland code into winit, so force X11 at runtime and reject Wayland-only sessions.

## Task list

### Phase 1: Overlay feasibility

1. [ ] Add the private overlay mode, bounded snapshot/control protocol, and a simple one-screen egui reminder that sends READY and one outcome.
2. [ ] Cover every monitor without native fullscreen, prove macOS frame/level/AeroSpace behavior, and package an early `LSUIElement` app for that check.

### Checkpoint: Screen behavior

- [ ] The Mac overlay is clickable, covers each attached display including menu bar/Dock space, and does not appear as an AeroSpace tile or native fullscreen Space. Record any necessary floating-rule fallback.
- [ ] Protocol parsing tests and the macOS release build pass.

### Phase 2: Running app

3. [ ] Add the windowless tray/menu process and connect timer due events to a successful overlay child through nonblocking event forwarding.
4. [ ] Harden child lifecycle, failure recovery, and buffered event ordering.
5. [ ] Complete Pause/Resume, Next Break, Reload/Open Config, and menu errors.

### Checkpoint: Core flow

- [ ] A short-interval Mac run proves due → all-screen overlay → elapsed/Skip/Postpone → correct next deadline, with no duplicate or overlapping child.
- [ ] Invalid reload retains settings; valid reload and manual Pause follow the approved timer spec. Sleep/wake preserves the remaining interval and display countdown.

### Phase 3: Finish

6. [ ] Finish the responsive overlay layout, keyboard actions, and bounded optional image with contain/cover and fallback.
7. [ ] Finalize the macOS app bundle, README, AeroSpace/X11 instructions, and available platform checks.

### Checkpoint: Window module complete

- [x] `cargo test --all-targets` (60 passed, 0 failed), `cargo fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo build --release` pass on macOS (review fixes verified 2026-10-03).
- [ ] The packaged Mac app passes the screen, tray, image, action, reload, and sleep/wake checks in `SPEC-reminder-window.md`.
- [ ] Linux/X11 build commands and runtime prerequisites are documented; an actual Linux build is run when a build host or CI runner is available, and X11 runtime is labelled unverified until tested in a session.
- [ ] Code review finds no unresolved timer/protocol/window correctness issue; user reviews the implemented module.

## Risks and mitigations

| Risk | Impact | Mitigation |
|---|---|---|
| AeroSpace tiles or moves the overlay, or eframe flashes a small root window | Break does not cover the active screen | Test the native frame and accessory/borderless window behavior in Tasks 1–2 before building the full UI; document a precise floating rule only if needed |
| eframe child viewport lacks a public winit handle | Other monitors remain incorrectly sized | Match unique titles in `NSApplication.windows()` before showing children; test every attached monitor; switch to one process per monitor only if that runtime probe fails |
| A child action, buffered stdout, and exit arrive in different orders | Postpone is lost or a break is scheduled twice | Task 4 latches one child action, orders stdout/EOF, waits for exit or forces close, and tests the event orders; a bounded drain grace handles inherited stdout after the direct child exits |
| No Linux/X11 host is available on the current Mac | X11 behavior cannot be proven locally | Keep X11-only code behind platform checks, document native build prerequisites and executable Linux commands; report build/runtime verification separately |
| Large or corrupt local image exhausts the child or hides controls | Break is missed | Bound file bytes and decoded dimensions; decode once, render the no-image layout on error |

## Open questions

None about behavior. Native window and Linux build feasibility are explicit verification checkpoints, not silent assumptions.
