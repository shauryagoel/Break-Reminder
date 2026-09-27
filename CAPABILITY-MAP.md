# Capability Map: Break Reminder

| Module id | Responsibility | Depends on |
|---|---|---|
| `configuration` | Load and validate YAML settings for timing, postpone choices, appearance, and image path. | — |
| `reminder-timing` | Schedule breaks and apply skip, postpone, and countdown rules. | `configuration` |
| `reminder-window` | Render the display-covering reminder, image, and controls; handle platform window behavior. | `configuration`, `reminder-timing` |

Build order: `configuration` → `reminder-timing` → `reminder-window`.
