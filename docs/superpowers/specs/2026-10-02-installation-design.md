# Break Reminder installation and automatic startup

## Objective

Add a per-user installation command that makes Break Reminder discoverable in macOS Spotlight/Raycast and Linux application launchers, and enables startup after graphical login following a restart. This is one installation capability depending on the existing `reminder-window` application. The user approved the native installer approach on 2026-10-02; this document defines its behavior for review before implementation.

Use the existing Rust application, macOS bundle command, and native OS registration files. Installation requires no administrator access. Linux retains the existing X11 and StatusNotifier-host requirements.

## Commands and user behavior

```sh
./scripts/install.sh
./scripts/uninstall.sh
```

The installer detects macOS or Linux, builds the release application using the current project toolchain, installs it for the current user, and enables startup at the next graphical login. Its output gives the installed location and command for launching immediately. Unsupported operating systems and invalid installation paths fail with a clear diagnostic and a nonzero exit status.

Quit through the app menu before reinstalling or uninstalling, so the parent closes any active overlay normally. The installer enforces this: before writing, it detects a running instance with `pgrep -u "$(id -u)" -x break-reminder` and exits nonzero with a Quit-first diagnostic, without terminating it. A running parent launches its overlay from the executable path recorded at startup, so replacing files underneath it would run a mismatched overlay. Reinstallation replaces application files and refreshes registration without overwriting user settings. Uninstall removes only the application's installed bundle/binary, Linux launcher icon, and launcher/startup registrations; it preserves the configuration directory and instance-lock file. Repeated uninstall succeeds when these installed artifacts are already absent. Uninstall does not terminate arbitrary processes by executable name.

## Platform registration

| Platform | Installed application | Launcher registration | Login startup |
|---|---|---|---|
| macOS | `~/Applications/Break Reminder.app` | Native app bundle in the user's Applications directory | `~/Library/LaunchAgents/com.breakreminder.app.plist` |
| Linux/X11 | `~/.local/bin/break-reminder` | `$XDG_DATA_HOME/applications/com.breakreminder.app.desktop` | `$XDG_CONFIG_HOME/autostart/com.breakreminder.app.desktop` |

On Linux, unset or empty `XDG_DATA_HOME` and `XDG_CONFIG_HOME` use `~/.local/share` and `~/.config`. Reject relative XDG paths rather than writing registrations relative to the current directory. Apply the same path validation to uninstall, and document that uninstall must use the same XDG overrides as installation to locate all registrations. Preserve the app's existing configuration location, `~/.config/break-reminder/config.yaml`.

### macOS

Reuse `scripts/package-macos.sh` and retain `com.breakreminder.app`, the existing executable name, and `LSUIElement=true`. The LaunchAgent uses an absolute executable path in `ProgramArguments`, `RunAtLoad=true`, `LimitLoadToSessionType=Aqua`, and `StandardErrorPath` set to the absolute path of `~/Library/Logs/break-reminder.log`, so startup diagnostics such as fallback from an invalid configuration are visible. Omit `KeepAlive` so Quit leaves the app stopped for that login session. Installation writes the plist without bootstrapping it in the current session; launchd loads it at the next graphical login. Installation and uninstall never call `launchctl`: reinstallation rewrites the same plist and uninstall deletes it. Unloading a job terminates its running app, and launchd rereads the plist at each login, so no unload is needed.

Normal application indexing supplies Spotlight and Raycast discovery. Installing in `~/Applications` is the selected user-scoped location; verify its discovery on the current Mac because indexing and launcher exclusions can affect visibility. Use native public mechanisms, without a Raycast extension or the private `lsregister` utility. The documented immediate launch command is `open "$HOME/Applications/Break Reminder.app"`.

[Apple documents app-bundle layout](https://developer.apple.com/documentation/bundleresources/placing-content-in-a-bundle), [user LaunchAgents and RunAtLoad](https://developer.apple.com/library/archive/documentation/MacOSX/Conceptual/BPSystemStartup/Chapters/CreatingLaunchdJobs.html), and [Applications-folder Spotlight discovery](https://support.apple.com/guide/mac-help/open-apps-in-spotlight-mh35840/mac). [Raycast documents installed application search](https://manual.raycast.com/search-bar). `~/Applications` discovery is an implementation choice to confirm at runtime.

### Linux/X11

Both desktop files use `Type=Application`, `Name=Break Reminder`, `Terminal=false`, an absolute, quoted `Exec` path, and an absolute `Icon` path pointing to `$XDG_DATA_HOME/icons/com.breakreminder.app.png`. The launcher file identifies a utility application; the autostart file launches the same installed executable after desktop login. Do not treat `Exec` as a shell command. Instead of implementing two-layer desktop-entry escaping (string escaping over `Exec` quoting), reject installation paths containing characters that would need it; see Installation safety.

Rofi `drun` reads the application desktop entry. Rofi `run` and stock `dmenu_run` find executable names through the graphical session's `PATH`; document that this must include `~/.local/bin`, and print a setup hint when the installer's `PATH` lacks it. Plain `dmenu` displays whatever input its caller supplies. The installer must not silently modify shell or window-manager configuration.

XDG autostart requires a desktop/session that processes its entries. Document a startup hook invoking `~/.local/bin/break-reminder` for minimal window managers without an autostart runner. The installed application continues to reject Wayland sessions. The immediate command is `"$HOME/.local/bin/break-reminder"` or activation from a configured launcher.

These behaviors follow the [XDG base directory specification](https://specifications.freedesktop.org/basedir/latest/), [desktop entry specification](https://specifications.freedesktop.org/desktop-entry/latest-single/), [autostart specification](https://specifications.freedesktop.org/autostart/latest/), [Rofi modes](https://davatorium.github.io/rofi/current/rofi.1/), and [dmenu executable discovery](https://git.suckless.org/dmenu/file/dmenu_path.html).

## One running instance

In the normal GUI startup branch of `src/main.rs`, acquire a nonblocking, process-lifetime standard-library file lock before calling `app::run` or loading/creating configuration. Use one persistent lock path, `~/.config/break-reminder/instance.lock`, regardless of a custom `--config` path, so all launcher and autostart routes share the same guard.

A second parent exits successfully with a concise already-running diagnostic and leaves the first parent's timer untouched. The first parent retains the open lock file for its lifetime. Process exit releases the lock automatically; a leftover file does not prevent restart. Do not delete the file on release or uninstall, because deleting a locked inode can allow a second instance to lock a different file at the same path. Lock and directory I/O failures remain errors.

The validation-only `--check-config` command and private `--overlay` child mode bypass this lock. No additional IPC, activation window, or single-instance dependency is needed.

## Installation safety and code style

Use small POSIX shell scripts with `set -eu`, quoted paths, and the existing project-directory lookup. Validate a nonempty absolute `HOME` and relevant XDG paths before installation writes, and reject control characters on both platforms (XML can normalize or reject them). On Linux, also reject paths containing double quotes, backticks, dollar signs, backslashes, percent signs, or equals signs. The first groups need desktop-entry escaping; the Exec specification forbids equals signs in executable paths. Escape XML content when generating the plist. Treat remaining path punctuation as data, including spaces and ampersands, and on macOS also quotes, backslashes, dollar signs, backticks, and percent signs.

Build before replacing installed application files. Prepare the replacement bundle/binary in a temporary sibling before replacing the existing application. On macOS, move the existing bundle aside, move the prepared bundle into place, then remove the old bundle; `mv` onto an existing directory would nest the new bundle inside it. Publish the Linux executable by rename rather than truncating a possibly running executable. Reject destination symlinks that would redirect writes outside the selected installation paths. Report failed copies, registration writes, and removals; do not report a complete installation after a partial failure. Do not overwrite or remove unrelated files in shared Applications, bin, applications, or autostart directories.

Use Rust 2024, the existing Rust 1.95 minimum, `rustfmt`, `Result` for I/O failures, and the standard library. For example, the instance guard retains a `std::fs::File` after a successful [`try_lock()`](https://doc.rust-lang.org/std/fs/struct.File.html#method.try_lock) instead of recording a PID and guessing whether it is stale. No new runtime or package dependency is required.

## Files and verification

Expected files: `scripts/install.sh`, `scripts/uninstall.sh`, a small runnable installer check under `scripts/`, `src/main.rs`, a focused instance-lock implementation/test if needed, and `README.md`. Reuse the existing macOS packager. Keep tests beside the Rust logic and avoid unrelated refactoring.

```sh
cargo test --all-targets
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo build --release
sh -n scripts/install.sh scripts/uninstall.sh
./scripts/package-macos.sh
```

Leave one runnable installation check that uses an isolated temporary home and command stubs: verify both platform layouts, plist escaping, Linux unsafe-path rejection, XDG defaults and overrides, executable permissions, running-instance refusal, repeat installation over an existing bundle, configuration preservation, and uninstall cleanup. It must not register real login jobs or alter the user's actual installed application. Add a Rust lock check proving contention, release/reacquisition, and safe reuse of the persistent file. Verify dispatch preserves config validation and overlay startup while the parent lock is held.

On macOS, separately verify the installed bundle can launch, LaunchAgent loading starts one parent, manual activation while it is running creates no second timer, Quit stays quit, and the installed app is discoverable in Spotlight/Raycast where available. Logout/reboot startup remains a separate manual check unless performed. Real-home installation/service changes for testing require explicit user authorization. On a Linux/X11 host, verify build, desktop launch, login startup, and Rofi/dmenu discovery in a correctly configured session. Mac-only checks cannot establish Linux runtime behavior.

## Boundaries and acceptance criteria

- Always: preserve settings, use per-user paths, escape generated registration data, retain X11 restrictions, surface launcher/session prerequisites, and record which runtime checks actually ran.
- Ask first: install into the user's real home for testing, modify shell/window-manager settings, add dependencies, or change supported platforms or timer/config semantics.
- Never: require `sudo`, use broad process-name kills, delete the locked file while it may be held, force-relaunch after Quit, or claim an unperformed reboot/Linux check passed.

Success means installation creates the specified executable/bundle and launcher/login entries; repeat installation and removal preserve user settings; autostart and manual launch share one parent; Quit remains effective; existing tests and required build/lint checks pass; and documentation explains the Linux PATH/autostart and macOS indexing prerequisites and the macOS login-start log location. Unperformed platform checks must remain explicitly identified.

No product-behavior questions remain. On 2026-10-02 the user reviewed and updated this spec and authorized implementation after a consistency check.
