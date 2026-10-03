# Spec: `reminder-window`

## Objective

Complete the [break reminder](CAPABILITY-MAP.md) as a Rust desktop app. Between breaks it lives in the macOS menu bar or an X11 tray without an application window. At each due time it covers every connected monitor with a temporary, interactive reminder window that does not create its own native fullscreen Space or become an AeroSpace tile. On macOS, it also appears above apps already in full-screen Spaces. The current machine is macOS; X11 is the other first-release target. The existing [`configuration`](SPEC-configuration.md) and [`reminder-timing`](SPEC-reminder-timing.md) modules provide validated settings and scheduling.

## User behavior

- Running `break-reminder [--config PATH]` loads the YAML settings, starts the timer, and creates one status icon. A startup load error uses the sample defaults and reports the error in the menu and on stderr until a successful reload. The existing `--check-config [--config PATH]` continues to validate and exit without UI, and fails on invalid settings. A missing config still gets the sample file.
- Clicking the static status icon opens a dropdown with a minutes:seconds countdown in its next-break status row, or the current Paused/Break in progress state, plus Pause or Resume, Restart Timer directly below it, Increase Timer, Take Break Now, Reload Config, Open Config, and Quit. The next-break row stays current to the second using deadline-based event-loop wakes. The icon has no countdown or state text beside it. Pause/Resume, Restart Timer, Increase Timer, and Take Break Now are disabled during an active reminder. Restart Timer uses the latest configured full interval and keeps a paused timer paused. Increase Timer has an Add N min submenu entry for each configured postpone duration, in config order; selections add to the current remainder and preserve pause. Take Break Now starts a normal reminder immediately from waiting or paused state, then uses the normal completion/reset rules. Open Config uses the desktop opener for the resolved file path, without a shell.
- Reload Config first loads and validates a complete replacement. On success it swaps active settings, updates Increase Timer choices, and preserves the current deadline or paused remainder. On a config error it leaves the timer/settings and menu choices untouched and shows a short error in the menu and full detail on stderr. The overlay already open keeps its launch-time image, text, colors, background transparency, display length, and postpone choices.
- When due, one overlay invocation creates a borderless covering window on each monitor. Each window has the same countdown and controls. The countdown begins after the main window and all surviving secondary windows are ready. Skip or any configured Postpone choice on any window dismisses all windows. Natural elapsed duration dismisses them and starts the next full interval. The child emits only its first action; the parent applies it once by invocation ID and never lets a delayed exit overwrite it. A launch failure or close without action starts a full interval and reports the failure in the menu and on stderr. Temporary break and media-pause diagnostics clear at the next break, restoring any configuration diagnostic.
- With `pause_media: true`, the parent makes one asynchronous attempt to pause supported media after the overlay's readiness signal is accepted and its START acknowledgment is queued. It captures the flag at launch, ignores duplicate/stale readiness signals, and does not pause on a failed launch or while quitting. Pause commands preserve already-paused media. A command failure or ten-second timeout reports an error without affecting scheduling or closing the overlay. Media is not resumed after any break outcome. Platform coverage and permissions are documented in [README.md](README.md).
- The overlay uses the YAML title, message, background/text/accent colors, optional image, image fit, and ordered Postpone choices. It supports 1–12 visible choices and labels each with its duration. The no-image layout remains complete if no image is configured or decoding fails.

## Visual and interaction contract

The presentation fills the screen with the configured deep background using `appearance.background_transparency_percent`, keeping text and controls opaque. The default 15% transparency renders at 217/255 opacity (about 85%). Convert percentages to eight-bit alpha with rounding so zero is opaque and 100 is fully transparent. It uses a centered content column with a remaining-time readout, an optional image, the title and message below it, and a clear action area. It uses restrained borders, spacing, and type hierarchy rather than gradients or heavy shadows. Reserve an image area of up to 640 × 280 logical points even when no image is configured or decoding fails, so the text retains its lower position. `contain` shows the whole image centered within this area; `cover` fills it with centered cropping. Preserve original source pixels and use linear mipmap filtering for smoother downscaling on high-resolution displays. The action area wraps Postpone buttons as needed, centers every row including incomplete rows, and keeps Skip visually distinct. Every monitor shows the same information; the first action wins.

The default palette must keep body text and controls legible. Buttons have visible hover and keyboard-focus states and labels that do not depend on color alone. Skip keeps its distinct idle appearance while using the same accent hover and pressed states as Postpone. When START enables the controls, initially highlight and focus **Skip current break** in each viewport, so Enter or Space skips without an initial Tab or click. Request that focus once without overriding later keyboard navigation. On macOS, acquire native keyboard focus before READY: the overlay application must be active and its root window must be key, so Tab reaches the popup rather than the previous foreground application. Retry asynchronous activation within the bounded readiness wait; stop requesting native focus after READY so normal app switching and secondary-window interaction remain available. Pointer movement, hovering other controls, and transient native-window focus notifications must preserve the selected action's ring; actual Tab navigation or clicks keep their normal behavior. Tab reaches all actions; Enter or Space activates the focused action; Escape skips the break. Text, countdown, image area, and all 12 choices remain usable on an 800×600 logical window, with content scrolling if necessary. Verify on the Mac's built-in display and any connected external monitor, including different scale factors when available. Custom YAML RGB colors are rendered as supplied, with translucency applied only to the background.

Accept static PNG, JPEG, and WebP images. Decode a local image once per overlay, with a 20 MiB file limit and a 4096-pixel limit on either dimension, then upload one texture for reuse across windows. Unsupported, corrupt, oversized, or unreadable images produce a concise diagnostic and the no-image layout. Image failure never suppresses a break.

## Runtime and platform design

Use one binary in two modes. Normal mode owns a windowless `winit` event loop, the tray/menu, validated `Config`, and `Timer`. It uses `ControlFlow::WaitUntil` for the next due time or second-level menu refresh; paused and active-break states wait for events without periodic refresh. Its UI thread never blocks on child I/O or process waits. On a due event or Take Break Now it starts the same executable in private `--overlay` mode; only that process initializes the `eframe/egui` renderer. The overlay receives a serialized snapshot of the active display settings through stdin and writes protocol messages to stdout. Its appearance and Postpone choices do not change mid-break.

Stdin starts with a 4-byte big-endian byte length followed by at most 256 KiB of serialized snapshot bytes. A worker flushes the snapshot but keeps the pipe open for a later `START\n` line; the child reads exactly the stated byte count before parsing that line, so message text and YAML newlines cannot confuse framing. The child writes `READY\n` to stdout only after the main window and all surviving secondary windows have been created, sized, shown, and painted. The parent then calls `Timer::visible` and asks its I/O worker to send `START`; the child starts its display countdown on that acknowledgment. This keeps the parent's validation deadline no later than the child's elapsed event. The child centrally latches one terminal line: `ELAPSED`, `SKIP`, or `POSTPONE <index>`, flushes it, closes all windows, and exits. The parent maps a Postpone index through that invocation's saved config snapshot. A reader thread forwards ordered stdout lines and EOF to the child supervisor; process exit is tracked separately. No pipe write or process wait blocks the parent event loop. Normally, exit without a terminal line becomes `Closed` or `Failed` only after stdout EOF, so a buffered choice cannot lose to an exit event. If a descendant keeps stdout open after the direct child has been reaped, a bounded drain grace ends that wait: preserve any valid first action already read, otherwise report failure and schedule a full interval. The parent confirms or forces direct-child closure before launching another overlay. Protocol errors are reported and schedule a full interval.

On macOS, create one eframe root viewport and an immediate child viewport for each additional `NSScreen`. Use each [`NSScreen.frame`](https://developer.apple.com/documentation/appkit/nsscreen/frame), including the menu bar and Dock area, rather than its visible frame. Configure borderless, non-fullscreen windows with no native close button. Set the app's activation policy to Accessory before creating windows; the packaged app also sets `LSUIElement=true`. For the root, use eframe's public `CreationContext::winit_window()` handle; for immediate children, use unique titles and `NSApplication.windows()` to locate their `NSWindow`s before showing them, then set exact screen frames and a level above ordinary application and menu/Dock windows. Keep all AppKit work in a small macOS-only Rust module. [eframe's viewport API](https://docs.rs/egui/0.36.2/egui/viewport/index.html) and [AppKit window enumeration](https://developer.apple.com/documentation/appkit/nsapplication/windows) support this approach; actual AeroSpace classification, stacking, clicks, Space behavior, and absence of window flash require a Mac runtime test. If AeroSpace still manages a window, document a specific floating rule as the fallback.

On X11, use winit's monitor positions, sizes, and scale factors with borderless eframe viewports at absolute coordinates and always-on-top window hints; do not use eframe's `with_monitor`, which requests borderless fullscreen. Force the X11 event-loop backend in both processes, since winit otherwise chooses Wayland first when both display variables are set. Reject `XDG_SESSION_TYPE=wayland` even when XWayland sets `DISPLAY`, because an X11 window cannot reliably cover native Wayland windows. A painted-frame acknowledgment is the readiness signal because winit does not implement native `is_visible()` on X11. Verify mixed-DPI placement, stacking, clicks, and tray behavior in a real X11 session. The first-release tray backend is KSNI, which requires an X11 desktop with a StatusNotifier watcher and host; a missing host must be called out in the run instructions. A Wayland-only Linux session exits with a clear unsupported-session message. Wayland and Windows are outside this release.

Monitor enumeration occurs for each break. A monitor connected during an already visible break is covered at the next reminder; a disconnected monitor must not strand the remaining windows or timer. A secondary display whose window reports a configuration, showing, or placement failure is skipped with a stderr diagnostic; only a main-display readiness failure fails the break. This recovery does not cover native child-window creation failures inside eframe, and real monitor-disconnect behavior still requires runtime verification.

## Tech stack

- Rust 2024 edition. Raise the package minimum to Rust 1.95 because [eframe 0.36.2 requires it](https://docs.rs/crate/eframe/0.36.2/source/Cargo.toml); the current Mac has Rust 1.97.
- `eframe/egui` 0.36.2 with `glow`, `default_fonts`, `accesskit`, and `x11`, without the default `wgpu` or Wayland features. [eframe documents the smaller glow build](https://docs.rs/crate/eframe/0.36.2/features). Use `image` 0.25 with only PNG, JPEG, and WebP decoders for local images.
- `winit` 0.30.13 and `tray-icon` 0.25.1 for the windowless parent. Set direct winit `default-features = false, features = ["rwh_06", "x11"]` and tray-icon `default-features = false, features = ["ksni"]` to avoid their default backends. Eframe's transitive `egui-winit` dependencies still enable winit's Wayland feature through Cargo feature unification; explicitly select X11 at runtime where supported and reject Wayland-only sessions. The [tray-icon platform notes](https://docs.rs/tray-icon/0.25.1/tray_icon/) explain the StatusNotifier host requirement.
- `serde-saphyr`'s serialize feature for the child settings snapshot; retain its bounded deserialize path. Rust standard-library pipes, threads, process APIs, and channels for the small child protocol. `objc2-app-kit` behind `cfg(target_os = "macos")` for native window adjustment. No Swift runtime code or web UI.

## Commands

```sh
cargo run -- --config ./assets/default-config.yaml
cargo run -- --check-config --config ./assets/default-config.yaml
cargo test --all-targets
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo build --release
```

On a Debian/Ubuntu X11 build host, install `libxcb-render0-dev`, `libxcb-shape0-dev`, `libxcb-xfixes0-dev`, `libxkbcommon-dev`, and `libssl-dev`, then run `cargo test --all-targets` and `cargo build --release`. The [eframe Linux instructions](https://docs.rs/crate/eframe/0.36.2) list these native prerequisites. These commands check compilation and logic; actual tray/window behavior needs a separate X11 session. The current Mac has no running Linux container, so an X11 host or CI runner is needed for that build gate.

Normal `cargo run` uses `~/.config/break-reminder/config.yaml`. A short-lived temporary config with the minimum one-minute interval is used for manual overlay checks; the user's real config is not modified. Add a macOS packaging command that builds an `.app` with `LSUIElement=true` and a stable bundle ID, and document the equivalent X11 binary/run prerequisites.

## Project structure

```text
src/main.rs            CLI dispatch for check, parent, and private overlay modes
src/app.rs             Windowless tray app, config reload, timer events, child lifecycle
src/media.rs           Bounded media commands, called off the parent UI thread
src/overlay.rs         eframe viewports, rendering, image, and single action latch
src/overlay/image.rs   Bounded image decode/fallback and contain/cover geometry
src/macos_window.rs    macOS-only AppKit window frame/level adjustment
src/config.rs          Existing validated YAML provider
src/timing.rs          Existing pure timer provider
assets/               Sample YAML and status/app icon assets if needed
scripts/              Small macOS app-bundle command
scripts/pause-media-macos.js  Built-in macOS automation, embedded in the binary
README.md             Run, configure, AeroSpace fallback, and X11 host requirements
```

The file split may be smaller if a focused implementation stays readable. Keep protocol parsing and its tests beside the code that uses it; do not add a generic IPC framework.

## Code style

Use `rustfmt`, `snake_case`, explicit duration units, and `Result` for I/O and platform errors. Keep native calls under a platform `cfg`, and preserve one owner for the active overlay. For example:

```rust
if let Ok(replacement) = config::load(&path) {
    timer.reload(replacement.interval, replacement.display);
    config = replacement;
}
```

Real reload code also reports an error without changing either value.

## Testing strategy

On macOS, `cargo build && python3 scripts/check-macos-fullscreen.py` runs the actual overlay above a temporary native full-screen test app. The test app reactivates as the reminder window appears to reproduce a focus handoff race. The check requires the reminder to own native keyboard input at READY, keeps both windows in the same visible Space with the reminder above the full-screen window, and verifies the countdown begins only after `START` and emits `ELAPSED` before exit. The shared AppKit configuration uses `FullScreenAuxiliary` with `CanJoinAllSpaces` so root and additional-display windows can join an existing full-screen Space. It must not use `FullScreenNone`, which reproduced a hidden reminder despite `READY` on the tested Mac. The check removes its temporary host and overlay afterward.

Keep deterministic tests for protocol parsing and event ordering: valid snapshot, READY/START, one terminal result, duplicate and stale messages, child exit before buffered outcome, missing outcome, reload during display retaining old Postpone choices, spawn failure, and elapsed only after readiness/deadline. Add focused image decode/fallback and contain/cover geometry checks. Existing config and timing tests must remain green. Use a short-interval Mac run to inspect actual screen bounds, menu bar/Dock coverage, AeroSpace behavior, focus, button clicks, Escape/Tab/Enter, multi-monitor action sync, and dismissal. Sleep/wake the Mac partway through an interval and a visible break and confirm the remaining time is unchanged. Inspect idle CPU/memory and ensure no hidden app window persists. An X11 build and real-session check are required before claiming its tray, stacking, focus, and monitor behavior verified; macOS results alone cannot establish those.

## Boundaries

- **Always:** Keep the parent event loop responsive and windowless; render one child overlay per due break; retain a validated launch snapshot; reject malformed child messages; preserve the first terminal action; report failures without losing the timer; run tests, formatting, Clippy, release build, and Mac visual checks.
- **Ask first:** Change any approved timing or YAML semantics, change the first-release platforms, add a second GUI stack or Swift UI, or make the overlay use native fullscreen.
- **Never:** Block the parent UI thread on child I/O; queue missed breaks; let an invalid reload replace active settings; launch overlapping overlays; shell-execute a configured path; claim X11 runtime verified from a Mac-only build.

## Success criteria

1. Normal launch has a functioning macOS menu bar icon, no ordinary app window, and the specified menu actions. The equivalent X11 tray path compiles and is documented for a StatusNotifier-capable session; its real-session behavior remains provisional until tested there.
2. At each due time, every currently connected monitor receives a clickable, non-fullscreen covering window, with no AeroSpace tile or uncovered menu bar/Dock on the tested Mac. One action closes them all. A documented AeroSpace floating rule exists if the native behavior needs it.
3. The overlay shows the configured text, colors, optional bounded image, contain/cover fit, countdown, Skip, and all ordered Postpone choices without clipping at the tested sizes. Bad images fall back with an error.
4. Parent/child event ordering matches `SPEC-reminder-timing.md`: one ready signal, one terminal outcome, no timeout/Button race, no duplicate reschedule, no overlap, and correct pause/reload/failure behavior. Mac sleep does not consume remaining time.
5. Tests, formatting, Clippy, release build, and macOS menu/overlay verification pass on the current Mac. The documented Linux commands pass on an X11 build host before calling the X11 build verified; X11 runtime behavior remains explicitly unverified until checked in an X11 session.

## Open questions

None about the intended behavior. Native frame/level calls and X11 window-manager hints are implementation probes whose results must be documented against the success criteria.
