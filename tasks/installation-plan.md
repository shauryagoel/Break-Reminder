# Installation implementation plan

Implement the [reviewed installation spec](../docs/superpowers/specs/2026-10-02-installation-design.md). The user authorized implementation after checking their edits. Keep the existing window-verification plan and its unfinished checks intact.

1. [x] Add a parent-only standard-library file lock before config loading. Verify contention, release, and persistent-file reuse; preserve validation/overlay dispatch.
2. [x] Add shared installer validation plus native install/uninstall scripts. Verify next-login registrations, running-app refusal, safe replacement, settings preservation, and both platform layouts through one isolated shell check.
3. [x] Document installation, removal, logs, Linux PATH and window-manager startup requirements. Run Rust tests, formatting, Clippy, shell syntax/integration checks, and the release bundle build; review the diff and record platform limits.

Rust locking and installer checks can be developed independently. Native runtime tests use temporary paths. Real-home install, logout/reboot, and Linux/X11 runtime checks remain separate unless authorized and available.

## Verification results

All 48 Rust tests, formatting, Clippy, release bundle build, shell syntax, and isolated installer checks passed. The installer check catches removal of the running-app guard and interrupt-safe rollback through isolated mutations. An actual packaged Mac parent with a temporary HOME passed launch, duplicate prevention, running uninstall refusal, and restart; validation-only and overlay subprocess dispatch bypassed a held lock. Independent review's rollback and XML-control-path findings were fixed and checked.

The real installer refused because the user's app was already running. The existing process and real installation were not changed. Live login-job loading, Spotlight/Raycast indexing, logout/reboot, and Linux compilation/X11 runtime remain unverified; see README verification status.
