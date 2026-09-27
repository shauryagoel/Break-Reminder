# Tasks: `reminder-window`

## Task 1: Single-screen overlay and protocol

**Description:** Add GUI dependencies and a private `--overlay` path. Read a bounded length-prefixed settings snapshot from stdin, wait for START after emitting READY, and emit one flushed elapsed, Skip, or Postpone action from a simple egui root viewport. Keep `--check-config` working.

**Acceptance criteria:**

- [ ] The snapshot parser rejects an oversized, truncated, or malformed frame; terminal lines reject unknown actions and out-of-range Postpone indices. Tests cover framing and one-action latching.
- [ ] A direct Mac overlay run shows a clickable ordinary borderless window, starts the countdown only after READY/START, and exits after one outcome. `--check-config` still opens no UI.
- [ ] The dependency set uses Rust 1.95, eframe `glow`/X11/AccessKit without default `wgpu`/Wayland, and a private mode that never creates the parent tray.

**Verification:**

- [ ] Write failing protocol/state tests before implementation; run `cargo test --all-targets`, formatting, Clippy, and release build.
- [ ] Drive the private child through a local pipe and inspect its window/action on the Mac.

**Dependencies:** Completed `configuration` and `reminder-timing` modules.

**Files likely touched:** `Cargo.toml`, `Cargo.lock`, `src/main.rs`, `src/protocol.rs`, `src/overlay.rs` (5).

**Estimated scope:** Medium.

## Task 2: Full monitor coverage and AeroSpace

**Description:** Extend the overlay to one viewport per connected monitor. Apply exact macOS screen frames, level, and accessory policy without native fullscreen; add a small Mac bundle script early so AeroSpace is tested with `LSUIElement=true`. Add X11 physical-to-logical placement with a testable geometry function.

**Acceptance criteria:**

- [ ] Every monitor connected at reminder start gets a borderless covering window and all windows close on one action; READY follows creation, sizing, showing, and paint for all targets.
- [ ] On the Mac, the overlay covers menu bar/Dock space without a native fullscreen Space, remains clickable, and does not tile under AeroSpace. A specific floating-rule fallback is documented if native settings fail.
- [ ] Disconnecting a monitor during a break does not strand the remaining windows or timer; a newly connected monitor is included at the next break.
- [ ] X11 geometry accounts for monitor origins and scale factors; the code avoids eframe `with_monitor` and keeps X11 runtime status explicitly provisional.

**Verification:**

- [ ] Run focused geometry tests, full tests, formatting, Clippy, and release build.
- [ ] Inspect the packaged Mac overlay on every attached monitor, including focus, first click, window flash, and AeroSpace classification; unplug an external monitor mid-break when available.

**Dependencies:** Task 1.

**Files likely touched:** `src/overlay.rs`, `src/macos_window.rs`, `Cargo.toml`, `Cargo.lock`, `scripts/package-macos.sh` (5).

**Estimated scope:** Medium.

## Checkpoint: Screen behavior

- [ ] Task 2 passes its Mac visual checks or records a concrete fallback before the parent UI is built.
- [ ] The private overlay protocol and release binary remain green.

## Task 3: Tray parent and first end-to-end break

**Description:** Create the windowless `winit`/`tray-icon` parent. It loads config, starts `Timer`, waits efficiently for due time, launches the same binary as its overlay child, and forwards successful child READY/action events through an event-loop proxy. Start with a minimal Quit menu.

**Acceptance criteria:**

- [ ] Normal launch shows one macOS menu-bar icon and no ordinary window; a one-minute temporary config produces the overlay. The parent remains responsive while the child runs.
- [ ] READY triggers `Timer::visible` and START; Skip, configured Postpone, and elapsed schedule the approved delays in the normal child exit path.
- [ ] Child pipe reads/writes run off the event-loop thread; due/menu events continue while a reminder shows. Error-order hardening follows in Task 4.

**Verification:**

- [ ] Write failing happy-path protocol/event tests before wiring the process; run full tests, formatting, Clippy, and release build.
- [ ] Manually run a temporary one-minute config on the Mac through due, Skip, Postpone, and elapsed paths.

**Dependencies:** Tasks 1–2.

**Files likely touched:** `src/app.rs`, `src/main.rs`, `src/protocol.rs`, `Cargo.toml`, `Cargo.lock` (5).

**Estimated scope:** Medium.

## Task 4: Child lifecycle and event-order hardening

**Description:** Make the parent robust to process and pipe failure. Apply only one terminal result by invocation ID, let buffered stdout win over process exit, and confirm or force child closure before another launch.

**Acceptance criteria:**

- [ ] Spawn failure, missing READY, malformed output, child close without action, and early Elapsed before any accepted action yield one full-interval fallback and a diagnostic; no failure causes an immediate relaunch loop.
- [ ] Duplicate or stale terminal lines after an accepted action are ignored without changing that action's deadline.
- [ ] A valid action buffered before process exit wins even if exit notification arrives first; the parent waits for stdout EOF before using Closed/Failed.
- [ ] An unresponsive child is terminated without blocking the event loop, and a future reminder cannot overlap an unreaped child.

**Verification:**

- [ ] Add deterministic tests for event/EOF/exit orders and a fake-child failure path; run full tests, formatting, Clippy, and release build.
- [ ] On the Mac, close or kill a child and confirm the menu stays responsive and the next break is scheduled once.

**Dependencies:** Task 3.

**Files likely touched:** `src/app.rs`, `src/protocol.rs`, `src/main.rs` (3).

**Estimated scope:** Medium.

## Task 5: Menu controls and reload

**Description:** Add Next Break status, Pause/Resume, Reload Config, Open Config, and Quit behavior. Refresh status at most once per minute. Preserve active settings on invalid reload and the launch snapshot on reload during an overlay.

**Acceptance criteria:**

- [ ] Pause freezes the remainder, Resume restores it, and both are disabled during a break; Next Break/Paused/Break in progress labels are accurate.
- [ ] Valid reload calls `Timer::reload` once; invalid reload leaves config and timer unchanged and displays a short menu error with full stderr detail. An active overlay retains its old Postpone index mapping and appearance.
- [ ] Open Config uses `open` on macOS or `xdg-open` on X11 with an absolute argument and reports a failure; Quit closes an active child before the parent exits.

**Verification:**

- [ ] Add focused tests for reload and action routing, run full tests, formatting, Clippy, and release build.
- [ ] Check every menu action on the Mac, then perform a short interval and visible-break sleep/wake check.

**Dependencies:** Task 4.

**Files likely touched:** `src/app.rs`, `src/main.rs` (2).

**Estimated scope:** Medium.

## Checkpoint: Core flow

- [ ] The Mac parent/child flow and all menu controls work without duplicate reminders or timer drift.
- [ ] Config reload and sleep/wake behavior matches the approved timing spec.

## Task 6: Finished overlay UI and image

**Description:** Replace the simple overlay content with the approved responsive visual layout and keyboard actions. Decode the optional local image once with byte/dimension limits and render contain/cover or a no-image fallback.

**Acceptance criteria:**

- [ ] Configured title, message, colors, image fit, countdown, Skip, and 1–12 ordered Postpone buttons render without clipping at 800×600 and laptop/external sizes.
- [ ] Tab/Enter/Space and Escape operate the same actions as clicks; hover/focus states and the default palette are legible.
- [ ] PNG/JPEG/WebP load within 20 MiB and 4096×4096 limits. Missing, corrupt, unsupported, or oversized images report an error and still show a usable reminder.

**Verification:**

- [ ] Add focused image and contain/cover geometry tests, then run full tests, formatting, Clippy, and release build.
- [ ] Inspect the actual Mac overlay with no image, a valid image, and a bad image; test all 12 choices and keyboard navigation.

**Dependencies:** Tasks 2–5.

**Files likely touched:** `src/overlay.rs`, `src/image.rs`, `Cargo.toml`, `Cargo.lock` (4).

**Estimated scope:** Medium.

## Task 7: Package and document the first release

**Description:** Finalize the macOS `.app` command and user instructions. Document the config sample, menu, AeroSpace fallback if needed, X11 StatusNotifier/native library requirements, and exact verification status. Run the available platform checks.

**Acceptance criteria:**

- [ ] The packaged Mac app has a stable bundle ID, `LSUIElement=true`, one menu-bar icon, and the same overlay behavior as a cargo run.
- [ ] README gives concise macOS build/run, config, menu, image, and X11 prerequisites; it does not claim an unrun X11 check passed.
- [ ] A Wayland-only Linux session exits with a clear unsupported-session error instead of running without a visible tray.
- [ ] The documented Linux `cargo test --all-targets` and `cargo build --release` are run when a Linux host/CI is available; actual X11 session behavior is separately labelled until tested.

**Verification:**

- [ ] Run full Mac tests, format, Clippy, release build, bundle script, final menu/overlay visual check, and inspect idle CPU/memory with no persistent hidden window.
- [ ] Run available Linux build checks, test the Wayland-only startup guard where possible, and record any host limitation accurately.

**Dependencies:** Tasks 1–6.

**Files likely touched:** `scripts/package-macos.sh`, `README.md`, `Cargo.toml`, `src/app.rs`, `src/overlay.rs` (at most 5 if fixes are needed).

**Estimated scope:** Medium.

## Checkpoint: Window module complete

- [ ] All local success criteria in `SPEC-reminder-window.md` pass on macOS; X11 build and runtime statuses are explicit.
- [ ] Code review, user review, and the full app walkthrough are complete.
