# Break Reminder Design

## Intent and scope

Build a lightweight, Rust-focused desktop reminder for one person. It runs in the macOS menu bar or an X11 tray, reads settings from `~/.config/break-reminder/config.yaml`, and displays a timed reminder across every connected monitor. The macOS build is testable on the current machine; X11 is also a first-release target and needs an X11 runtime check before being called verified. Windows and Wayland are outside the first release. [winit does not support always-on-top window levels on Wayland](https://docs.rs/winit/latest/winit/window/enum.WindowLevel.html).

The approved [capability map](../../../CAPABILITY-MAP.md) defines `configuration` → `reminder-timing` → `reminder-window`. Each capability has its own spec, plan, task list, and review gate.

## Approach

Use one Rust binary in two modes. Normal mode owns a windowless `winit` event loop, a `tray-icon` menu, and the next-break deadline. When due, it launches its `--overlay` mode, which uses `eframe/egui` for the temporary reminder windows and returns the chosen action. The parent then schedules the next deadline. This avoids relying on a persistently hidden eframe root window: [eframe currently makes its root window visible after its first paint](https://github.com/emilk/egui/blob/main/crates/eframe/src/native/epi_integration.rs). The [tray-icon windowless winit example](https://github.com/tauri-apps/tray-icon/blob/dev/examples/winit.rs) demonstrates the parent event-loop pattern. All application UI and logic remain in Rust; macOS-specific window adjustment uses Rust AppKit bindings where needed.

A persistent eframe process would have less process coordination but risks an unwanted initial window under AeroSpace. Slint offers declarative styling, but adds another UI language and [license conditions](https://slint.dev/terms-and-conditions). Tauri's web UI conflicts with the Rust-focused interface requirement.

## Behavior and presentation

- The interval begins at app start. A reminder stays visible for the configured number of seconds, then closes and starts a fresh interval.
- Skip closes the reminder and starts a fresh interval. Postpone closes it and sets the next reminder for the selected relative delay. There is one pending reminder at a time.
- The YAML file configures interval, duration, an ordered list of visible postpone delays, title, message, colors, and an optional local image with fit mode. A missing config gets a sample file. Reloading invalid YAML keeps the last valid settings and reports the error in the menu.
- Every monitor receives a borderless window sized to its full screen frame, including the menu bar and Dock area, with synchronized countdown and controls. The window must not enter a native fullscreen Space. A single click on any monitor dismisses all reminder windows.
- The default layout has a calm dark background, centered message and optional image, prominent countdown, and clear Skip and postpone controls. It must scale to laptop and external-display sizes without clipping.
- The menu displays the next break and provides Pause/Resume, Reload Config, Open Config, and Quit.

## Platform and verification risks

On macOS, borderless sizing and always-on-top requests are not by themselves a guarantee of screen coverage. Test the actual window frame, level, first-click behavior, and AeroSpace tiling on the current machine. If native window settings are insufficient, provide a specific [AeroSpace floating rule](https://nikitabobko.github.io/AeroSpace/guide#dialog-heuristics) as a documented fallback. On X11, test the overlay and tray in a real desktop session; macOS testing alone cannot verify that runtime behavior. The app must not silently claim support for untested window managers.
