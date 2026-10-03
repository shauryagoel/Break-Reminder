# Break Reminder

A small Rust menu-bar app that reminds you to take regular breaks.

## What it does

- Waits quietly in the menu bar, then shows a borderless reminder on every connected display.
- On macOS, the reminder appears above full-screen apps without creating its own Space.
- Each break has a countdown, a **Skip** button, and configurable **Postpone** buttons.
- Keyboard: **Skip** is focused when the countdown starts. Tab / Shift+Tab move between buttons, Enter/Space presses the focused button, Escape skips.
- Optional: pause playing media when a break starts (`pause_media: true`).
- Supports macOS and Linux/X11. Wayland and Windows are not supported.

## Install

- Needs Rust 1.95+. On Linux, also install the [X11 build packages](docs/guide.md#linuxx11).
- Install for your user (no `sudo`; starts at next login): `./scripts/install.sh`
- Uninstall (settings are kept): `./scripts/uninstall.sh`
- Quit the app from its menu before installing or uninstalling.
- Start now:
  - macOS: `open "$HOME/Applications/Break Reminder.app"`
  - Linux: `"$HOME/.local/bin/break-reminder"`

## Configure

- Config file: `~/.config/break-reminder/config.yaml`. The app creates it from [the sample](assets/default-config.yaml) on first launch.
- Every option is optional; an omitted one uses its default. Unknown keys are errors.

| Option | Default | Allowed values |
|---|---|---|
| `interval_minutes` | `60` | Time between breaks, 1–1440 |
| `duration_seconds` | `30` | Length of each break, 1–3600 |
| `postpone_minutes` | `[10, 15]` | 1–12 distinct values, each 1–1440; order sets button order |
| `pause_media` | `false` | `true` pauses supported media when a break starts ([details](docs/guide.md#media-pause)) |
| `appearance.title` | `"Time for a break"` | Non-empty text |
| `appearance.message` | `"Step away from your screen and rest your eyes."` | Non-empty text |
| `appearance.background_color` | `"#101827"` | `#RRGGBB` |
| `appearance.background_transparency_percent` | `15` | 0 (opaque) – 100 (fully transparent); background only |
| `appearance.text_color` | `"#F8FAFC"` | `#RRGGBB` |
| `appearance.accent_color` | `"#69D5B2"` | `#RRGGBB` |
| `appearance.image.path` | no image | PNG/JPEG/WebP, ≤ 20 MiB, ≤ 4096 px per side; absolute, `~/…`, or relative to the config file |
| `appearance.image.fit` | `contain` | `contain` (whole image) or `cover` (fill and crop) |

- After editing, choose **Reload Config** from the menu. Your countdown is kept; changes apply from the next break.
- Check a config without starting the app: `cargo run -- --check-config [--config PATH]`. A missing file is an error.
- Full field rules: [configuration spec](SPEC-configuration.md).

## Menu

- Status row: time to next break, **Paused**, or **Break in progress**.
- **Pause / Resume**, **Restart Timer**, **Increase Timer**, **Take Break Now**.
- **Reload Config**, **Open Config**, **Quit Break Reminder**.

## Develop

- Test: `cargo test --all-targets`
- Run with a separate config: `cargo run --release -- --config /path/to/config.yaml`
- Build the macOS bundle: `./scripts/package-macos.sh`

## More docs

- [Guide](docs/guide.md): install paths, menu details, media pause permissions, AeroSpace, logs.
- [Verification status](docs/verification.md): which checks have run and which are still open.
- Specs: [configuration](SPEC-configuration.md), [timing](SPEC-reminder-timing.md), [reminder window](SPEC-reminder-window.md), [capability map](CAPABILITY-MAP.md).
