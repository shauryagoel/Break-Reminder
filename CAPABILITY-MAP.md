# Capability Map: Break Reminder

| Module id | Responsibility | Depends on |
|---|---|---|
| `configuration` | Load and validate YAML settings for timing, postpone choices, appearance, and image path. | — |
| `reminder-timing` | Schedule breaks and apply skip, postpone, and countdown rules. | `configuration` |
| `reminder-window` | Render the display-covering reminder, image, and controls; handle platform window behavior and optional media pause. | `configuration`, `reminder-timing` |
| `installation` | Install the per-user app, launcher and login-start entries; prevent duplicate parent instances. [Design](docs/superpowers/specs/2026-10-02-installation-design.md). | `reminder-window` |

Build order: `configuration` → `reminder-timing` → `reminder-window` → `installation`.
