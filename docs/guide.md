# Break Reminder guide

Detailed install, configuration, and troubleshooting notes. The [README](../README.md) has the short version.

## Install and start at login

Install Rust 1.95 or newer and the platform build prerequisites below, then run from this checkout:

```sh
./scripts/install.sh
```

The installer builds and installs for your user without `sudo`, and enables startup at the next graphical login after a restart. Quit Break Reminder from its menu before reinstalling or uninstalling; both commands refuse to change files while a `break-reminder` process for your user is running. Reinstalling preserves your settings. Opening the application again while it runs keeps the existing timer and status icon.

### macOS install

The app is installed in `~/Applications/Break Reminder.app` for Spotlight and Raycast discovery. Indexing or launcher search exclusions can delay its appearance. Start immediately with:

```sh
open "$HOME/Applications/Break Reminder.app"
```

Login startup uses `~/Library/LaunchAgents/com.breakreminder.app.plist`. Quit leaves the app stopped until you launch it again or log in again. Login-start errors are written to `~/Library/Logs/break-reminder.log`. The installer writes the registration without loading a login job in the current session.

### Linux/X11 install

The executable is installed in `~/.local/bin`; desktop and autostart entries go into `${XDG_DATA_HOME:-$HOME/.local/share}/applications` and `${XDG_CONFIG_HOME:-$HOME/.config}/autostart`, and the launcher icon goes into `${XDG_DATA_HOME:-$HOME/.local/share}/icons/com.breakreminder.app.png`. Rofi's `drun` mode discovers the desktop entry. For `dmenu_run` or Rofi's `run` mode, include `~/.local/bin` in your **graphical session's** `PATH`; a terminal's PATH may differ. Plain `dmenu` displays input supplied by its caller. Start immediately with:

```sh
"$HOME/.local/bin/break-reminder"
```

XDG autostart requires a session that processes desktop autostart entries. If your window manager does not, add the installed executable to its existing startup configuration. For example, an [i3 startup entry](https://i3wm.org/docs/userguide.html#_automatically_starting_applications_on_i3_startup) is:

```text
exec --no-startup-id ~/.local/bin/break-reminder
```

### Uninstall

Uninstall from this checkout after quitting:

```sh
./scripts/uninstall.sh
```

On Linux, use the same `XDG_DATA_HOME` and `XDG_CONFIG_HOME` overrides for install and uninstall. Both commands require absolute paths and refuse symlink installation paths and control characters. Linux paths with double quotes, backticks, dollar signs, backslashes, percent signs, or equals signs are also rejected. Uninstall removes the Linux launcher icon along with the executable and desktop/autostart entries. It preserves `~/.config/break-reminder`, including your configuration and the persistent instance-lock file, and macOS logs. The lock is released when the app exits; its file can safely remain.

## Building

### macOS

```sh
./scripts/package-macos.sh
open "target/macos/Break Reminder.app"
```

The bundle has identifier `com.breakreminder.app` and `LSUIElement=true`, so it runs in the menu bar without a Dock icon. It includes a green clock [application icon](../assets/app-icon.png) for native launchers, with the multi-resolution `assets/app-icon.icns` copied into the bundle's Resources directory. Reinstall after quitting to update an existing installation; launcher icon caches may take time to refresh. For development, `cargo run --release -- --config /path/to/config.yaml` accepts a separate config file.

To regenerate the icon artwork on macOS, run `swift scripts/generate-app-icon.swift`, then package or reinstall the app. Ordinary builds use the checked-in icon and do not require Swift.

Packaging signs and verifies the entire bundle. It uses ad-hoc signing by default. To use a certificate, list available identities with `security find-identity -v -p codesigning`, then set `BREAK_REMINDER_SIGN_IDENTITY` when running the packaging or installation script:

```sh
BREAK_REMINDER_SIGN_IDENTITY='Apple Development: Your Name (TEAMID)' ./scripts/package-macos.sh
```

Ad-hoc signatures identify a specific build, so rebuilding may prompt for Automation permission again. A stable certificate identity (for example, an Apple Development certificate) should help retain authorization across builds; persistence for this app still needs a grant, reinstall, and repeat-launch check. See [Apple's code-signing requirements](https://developer.apple.com/documentation/technotes/tn3127-inside-code-signing-requirements).

### Linux/X11

Build in an X11 session. For Debian and Ubuntu, the [pinned eframe 0.36.2 instructions](https://docs.rs/crate/eframe/0.36.2/source/README.md) recommend these packages:

```sh
sudo apt install libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev libxkbcommon-dev libssl-dev
cargo test --all-targets
cargo build --release
./target/release/break-reminder
```

The tray uses the StatusNotifier protocol; your desktop needs a StatusNotifier watcher and host to display the icon. The selected [tray-icon KSNI backend](https://docs.rs/crate/tray-icon/0.25.1/source/README.md) does not require the GTK/AppIndicator packages listed for its default backend. **Open Config** also needs `xdg-open`. Both the parent and overlay select X11 if both X11 and Wayland display variables exist, but a session identified by `XDG_SESSION_TYPE=wayland` is rejected even if XWayland provides `DISPLAY`. A missing or empty `DISPLAY` is rejected too.

## Menu

- **Next break in M:SS** status row updates once per second while running; it shows **Paused** or **Break in progress** in those states. The menu-bar icon itself stays static.
- **Pause** keeps the remaining interval; **Resume** continues it.
- **Restart Timer** resets the countdown to the latest configured full interval and keeps a paused timer paused.
- **Increase Timer → Add N min** offers every configured postpone choice, in the same order. Each selection adds that duration to the current remaining time, including a postponed countdown; repeated selections accumulate, and a paused timer stays paused. Reloading updates these choices.
- **Take Break Now** starts a normal break immediately, including while paused. When that break ends or is skipped, the countdown runs again from the full configured interval; a Postpone button keeps its usual behavior.
- Timer controls are disabled during an active break.
- **Reload Config** preserves the current countdown and paused state. **Open Config** opens the file in your desktop's default app.

Errors: an invalid startup config uses the sample defaults so reminders continue, and reports an error in the menu and on stderr. An invalid reload leaves the previous settings active and reports the same diagnostics. Configuration errors remain available until a successful reload; temporary break and media-pause errors clear at the next break, restoring any configuration error.

## Settings

On first launch, the app creates `~/.config/break-reminder/config.yaml` from [the sample](../assets/default-config.yaml). Validate a file without starting the UI; unlike the app, the check reports a missing file as an error and creates nothing:

```sh
cargo run -- --check-config
cargo run -- --check-config --config /path/to/config.yaml
```

```yaml
interval_minutes: 60
duration_seconds: 30
postpone_minutes: [10, 15]
pause_media: false
appearance:
  title: "Time for a break"
  message: "Step away from your screen and rest your eyes."
  background_color: "#101827"
  background_transparency_percent: 15 # 0 is opaque; 100 is fully transparent.
  text_color: "#F8FAFC"
  accent_color: "#69D5B2"
  image:
    path: "~/Pictures/break.png"
    fit: contain # or cover
```

Remove the `image` block if you do not want a picture. PNG, JPEG, and WebP are supported, up to 20 MiB and 4096 pixels on either side. An unreadable or invalid image produces a diagnostic while the reminder continues without it. A relative image path is resolved from the config file's directory. The `postpone_minutes` list controls the Postpone buttons and Increase Timer menu choices, including their labels and order; it accepts 1–12 distinct choices. All duration values are positive integers. The full field rules are in [the configuration spec](../SPEC-configuration.md).

Set `appearance.background_transparency_percent` to an integer from `0` (opaque) to `100` (fully transparent). It defaults to `15`, giving the background about 85% opacity even in existing configs that omit the field. Text and controls stay opaque. Choose **Reload Config** after editing; the new transparency applies to the next break. Images use their original resolution in a larger area of up to 640 × 280 logical points, with smooth downscaling for high-resolution displays. The title and message sit below this area; their position stays the same when no image is configured or an image cannot be loaded. Postpone rows are centered, including a partially filled final row.

## Reminder window

During a break, **Skip current break** is highlighted and focused as soon as the countdown starts, so Enter or Space skips immediately. Moving the mouse or hovering another button preserves the selected action's highlight, including during transient native-window focus updates. Tab and Shift+Tab move between controls, Enter or Space activates the focused control, and Escape skips. The countdown also dismisses the reminder when `duration_seconds` elapses.

## Media pause

Set `pause_media: true` to pause supported media once a reminder becomes visible. The default is `false`, including in existing configs without this field. Choose **Reload Config** after editing; the setting applies to future breaks. Media stays paused after the break ends, is skipped, or is postponed. A media-control failure appears in the menu and on stderr while the reminder continues.

### Linux

Install [`playerctl`](https://github.com/altdesktop/playerctl) and make it available in the app's graphical-session `PATH` (for example, `sudo apt install playerctl` on Debian/Ubuntu). The app runs `playerctl --all-players pause` for every available MPRIS player, including browsers exposing YouTube through MPRIS. Players without MPRIS support cannot be controlled; no running players is harmless.

### macOS

The app controls running Music, iTunes, TV, Spotify, and QuickTime Player, plus HTML video/audio in all tabs and windows of Safari, Safari Technology Preview, Chrome, Chromium, Brave, Edge, and Vivaldi. Grant **Automation** access when macOS requests it; permissions can be reviewed in **System Settings → Privacy & Security → Automation**. Browser videos such as YouTube also require **Allow JavaScript from Apple Events**: [Chrome uses View → Developer](https://www.chromium.org/developers/applescript/); [Safari exposes this in its Developer settings](https://developer.apple.com/documentation/safari-developer-tools/developer-settings) after enabling developer features. The app does not change these permissions. Firefox and other unsupported macOS apps, cross-origin embedded frames, and media outside HTML video/audio elements cannot be paused by this integration. Media that starts playing after the initial pause attempt is not polled.

The selected tab in each browser window is paused before scanning background tabs. These first pause commands also check JavaScript permission with a bounded timeout. Background-tab URLs and references are fetched in batches per window, then pause requests use Apple Events without waiting for page replies, so an unresponsive background tab cannot hold up the scan. Individual background-page execution errors are unavailable because those requests do not wait for replies. Native players are paused before any browser scan. Chromium tab ids are re-read after the URL batch; if tabs changed in between, that window reports an error instead of pairing URLs with the wrong tabs. Safari tabs have no ids, so a tab closed mid-scan can shift which tab a request reaches.

## Troubleshooting

### AeroSpace

The packaged overlay has covered the tested Mac display without being listed as an AeroSpace tile. If AeroSpace tiles it on your setup, place this rule after any broad tiling rule in your AeroSpace config and reload AeroSpace. An earlier matching rule must set `check-further-callbacks = true` for this rule to run:

```toml
[[on-window-detected]]
if = 'test %{app-bundle-id} = com.breakreminder.app'
run = 'layout floating'
```

### Chrome media pause on macOS

If the menu shows a Chrome error saying that executing JavaScript through AppleScript is turned off, enable **View → Developer → Allow JavaScript from Apple Events** in Chrome's menu bar ([Chromium AppleScript notes](https://www.chromium.org/developers/applescript/)). Also ensure Break Reminder can control Google Chrome under **macOS System Settings → Privacy & Security → Automation**. Keep `pause_media: true` in your selected YAML and choose **Reload Config**.

### Vivaldi media pause on macOS

If Vivaldi reports that executing JavaScript through AppleScript is turned off, enable **Vivaldi Settings → Privacy → Apple Events → Allow JavaScript from Apple Events**. Also ensure Break Reminder can control Vivaldi under **macOS System Settings → Privacy & Security → Automation**. Keep `pause_media: true` in your selected YAML and choose **Reload Config**.

### Logs on macOS

The login-start job saves stderr to `~/Library/Logs/break-reminder.log`. A direct Finder/Spotlight launch can send stderr to `/dev/null`, so it may have no saved log. To capture errors from the installed app, first quit its current instance, then run:

```sh
"$HOME/Applications/Break Reminder.app/Contents/MacOS/break-reminder" 2>>"$HOME/Library/Logs/break-reminder.log"
```
