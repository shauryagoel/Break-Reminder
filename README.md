# Break Reminder

A small Rust menu-bar app for regular breaks. It waits quietly between reminders, then shows a temporary, borderless window across each connected display. The reminder does not enter a native fullscreen Space. The first release targets macOS and Linux/X11.

## Install and start at login

Install Rust 1.95 or newer and the platform build prerequisites below, then run from this checkout:

```sh
./scripts/install.sh
```

The installer builds and installs for your user without `sudo`, and enables startup at the next graphical login after a restart. Quit Break Reminder from its menu before reinstalling or uninstalling; both commands refuse to change files while a `break-reminder` process is running. Reinstalling preserves your settings. Opening the application again while it runs keeps the existing timer and status icon.

On macOS, the app is installed in `~/Applications/Break Reminder.app` for Spotlight and Raycast discovery. Indexing or launcher search exclusions can delay its appearance. Start immediately with:

```sh
open "$HOME/Applications/Break Reminder.app"
```

Login startup uses `~/Library/LaunchAgents/com.breakreminder.app.plist`. Quit leaves the app stopped until you launch it again or log in again. Login-start errors are written to `~/Library/Logs/break-reminder.log`. The installer writes the registration without loading a login job in the current session.

On Linux/X11, the executable is installed in `~/.local/bin`; desktop and autostart entries go into `${XDG_DATA_HOME:-$HOME/.local/share}/applications` and `${XDG_CONFIG_HOME:-$HOME/.config}/autostart`. Rofi's `drun` mode discovers the desktop entry. For `dmenu_run` or Rofi's `run` mode, include `~/.local/bin` in your **graphical session's** `PATH`; a terminal's PATH may differ. Plain `dmenu` displays input supplied by its caller. Start immediately with:

```sh
"$HOME/.local/bin/break-reminder"
```

XDG autostart requires a session that processes desktop autostart entries. If your window manager does not, add the installed executable to its existing startup configuration. For example, an [i3 startup entry](https://i3wm.org/docs/userguide.html#_automatically_starting_applications_on_i3_startup) is:

```text
exec --no-startup-id ~/.local/bin/break-reminder
```

Uninstall from this checkout after quitting:

```sh
./scripts/uninstall.sh
```

On Linux, use the same `XDG_DATA_HOME` and `XDG_CONFIG_HOME` overrides for install and uninstall. Both commands require absolute paths and refuse symlink installation paths and control characters. Linux paths with double quotes, backticks, dollar signs, backslashes, percent signs, or equals signs are also rejected. Uninstall preserves `~/.config/break-reminder`, including your configuration and the persistent instance-lock file, and macOS logs. The lock is released when the app exits; its file can safely remain.

## macOS

Install Rust 1.95 or newer, then build and launch the menu-bar app:

```sh
./scripts/package-macos.sh
open "target/macos/Break Reminder.app"
```

The bundle has identifier `com.breakreminder.app` and `LSUIElement=true`, so it runs in the menu bar without a Dock icon. It includes a green clock [application icon](assets/app-icon.png) for native launchers, with the multi-resolution `assets/app-icon.icns` copied into the bundle's Resources directory. Reinstall after quitting to update an existing installation; launcher icon caches may take time to refresh. For development, `cargo run --release -- --config /path/to/config.yaml` accepts a separate config file. Quit from the menu-bar icon.

To regenerate the icon artwork on macOS, run `swift scripts/generate-app-icon.swift`, then package or reinstall the app. Ordinary builds use the checked-in icon and do not require Swift.

On first launch, the app creates `~/.config/break-reminder/config.yaml` from [the sample](assets/default-config.yaml). Edit it, then choose **Reload Config** from the menu; reloading preserves the current countdown and paused state. **Open Config** opens the file in your desktop's default app. **Pause** keeps the remaining interval; **Resume** continues it. **Restart Timer**, directly below Pause/Resume, resets the countdown to the latest configured full interval and keeps a paused timer paused. Pause/Resume and Restart Timer are disabled during an active break. The status item shows the next break, a paused state, or a break in progress. An invalid reload leaves the previous settings active and reports an error in the menu and on stderr. You can validate a file without starting the UI:

```sh
cargo run -- --check-config
cargo run -- --check-config --config /path/to/config.yaml
```

## Settings

```yaml
interval_minutes: 60
duration_seconds: 30
postpone_minutes: [10, 15]
appearance:
  title: "Time for a break"
  message: "Step away from your screen and rest your eyes."
  background_color: "#101827"
  text_color: "#F8FAFC"
  accent_color: "#69D5B2"
  image:
    path: "~/Pictures/break.png"
    fit: contain # or cover
```

Remove the `image` block if you do not want a picture. PNG, JPEG, and WebP are supported, up to 20 MiB and 4096 pixels on either side. An unreadable or invalid image produces a diagnostic while the reminder continues without it. A relative image path is resolved from the config file's directory. The `postpone_minutes` list controls both the button labels and their order; it accepts 1–12 distinct choices. All duration values are positive integers. The full field rules are in [the configuration spec](SPEC-configuration.md).

During a break, choose **Skip** or a configured **Postpone** button. Tab selects controls, Enter or Space activates the focused control, and Escape skips. The countdown also dismisses the reminder when `duration_seconds` elapses.

### AeroSpace

The packaged overlay has covered the tested Mac display without being listed as an AeroSpace tile. If AeroSpace tiles it on your setup, place this rule after any broad tiling rule in your AeroSpace config and reload AeroSpace. An earlier matching rule must set `check-further-callbacks = true` for this rule to run:

```toml
[[on-window-detected]]
if = 'test %{app-bundle-id} = com.breakreminder.app'
run = 'layout floating'
```

## Linux/X11

Build with Rust 1.95 or newer in an X11 session. For Debian and Ubuntu, the [pinned eframe 0.36.2 instructions](https://docs.rs/crate/eframe/0.36.2/source/README.md) recommend these packages:

```sh
sudo apt install libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev libxkbcommon-dev libssl-dev
cargo test --all-targets
cargo build --release
./target/release/break-reminder
```

The tray uses the StatusNotifier protocol; your desktop needs a StatusNotifier watcher and host to display the icon. The selected [tray-icon KSNI backend](https://docs.rs/crate/tray-icon/0.25.1/source/README.md) does not require the GTK/AppIndicator packages listed for its default backend. **Open Config** also needs `xdg-open`. Both the parent and overlay select X11 if both X11 and Wayland display variables exist, but a session identified by `XDG_SESSION_TYPE=wayland` is rejected even if XWayland provides `DISPLAY`. A missing or empty `DISPLAY` is rejected too. Wayland and Windows are outside this release.

## Verification status

`./scripts/check-install.sh` checks macOS/Linux installation layouts in temporary homes, including running-app refusal, registration data, settings preservation, repeat install/uninstall, and interrupted macOS replacement rollback. A temporary packaged macOS copy passed native parent launch, duplicate prevention, refusal to uninstall while running, and restart after process exit. Login-job loading, logout/reboot startup, and Spotlight/Raycast discovery of a real installation have not been checked. The existing running app was left untouched; installation and uninstall refused it as intended.

All 48 macOS tests, formatting, Clippy, and the release bundle build pass. A native menu smoke check confirmed Restart Timer placement and action routing, reload preserving the countdown, restart preserving pause, and active-break/quitting guards. The packaged overlay covered the built-in display, including the menu bar and Dock, and AeroSpace did not list it as a tiled window. The user confirmed centered content, twelve visible postpone choices, Skip, Postpone, Tab/Enter, Escape, and a usable reminder when its image was missing. The packaged menu's Pause, Resume, Reload Config, Open Config, and Quit actions were checked manually. A one-minute parent run showed a reminder; choosing 10 min changed the menu to a next break in about 10 min. An idle snapshot showed 0.0% CPU, about 46 MiB resident memory, and no AeroSpace-managed parent window. Sleep/wake behavior, multiple monitors, workspace switching, and scrolling on a shorter display still require manual checks.

Linux compilation and a live X11 tray/overlay check have **not** run yet. The current Mac lacks a matching Linux target standard library/toolchain and has no X11 session; macOS checks cannot establish X11 behavior.
