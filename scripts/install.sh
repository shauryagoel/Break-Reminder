#!/bin/sh
set -eu

project_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
. "$project_dir/scripts/install-common.sh"
cd "$project_dir"

case "$install_os" in
    Darwin) ./scripts/package-macos.sh ;;
    Linux) cargo build --release ;;
esac
ensure_stopped

mkdir -p "$(dirname -- "$application")" "$(dirname -- "$startup")"
staging=$(mktemp -d "$(dirname -- "$application")/.break-reminder.XXXXXX")
icon_staging=
cleanup() {
    if [ -d "$staging/previous.app" ] && [ ! -e "$application" ]; then
        mv -- "$staging/previous.app" "$application" || {
            printf 'Could not restore previous bundle; recover it from %s/previous.app\n' "$staging" >&2
            return 2
        }
    fi
    if [ -n "$icon_staging" ]; then
        rm -f -- "$icon_staging"
    fi
    rm -rf -- "$staging"
}
trap cleanup EXIT
trap 'exit 2' HUP INT TERM

case "$install_os" in
    Darwin)
        cp -R "target/macos/Break Reminder.app" "$staging/Break Reminder.app"
        xml_escape() {
            printf '%s' "$1" | sed -e 's/&/\&amp;/g' -e 's/</\&lt;/g' -e 's/>/\&gt;/g' -e 's/"/\&quot;/g' -e "s/'/\&apos;/g"
        }
        executable_xml=$(xml_escape "$application/Contents/MacOS/break-reminder")
        cat > "$staging/startup" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key><string>com.breakreminder.app</string>
    <key>ProgramArguments</key><array><string>$executable_xml</string></array>
    <key>RunAtLoad</key><true/>
    <key>LimitLoadToSessionType</key><string>Aqua</string>
</dict>
</plist>
PLIST
        plutil -lint "$staging/startup"
        ensure_stopped
        if [ -d "$application" ]; then
            mv -- "$application" "$staging/previous.app"
        fi
        if ! mv -- "$staging/Break Reminder.app" "$application"; then
            fail "Could not publish the application bundle"
        fi
        chmod 644 "$staging/startup"
        mv -f -- "$staging/startup" "$startup"
        printf 'Installed: %s\nStarts at next graphical login. Launch now: open "$HOME/Applications/Break Reminder.app"\n' "$application"
        ;;
    Linux)
        mkdir -p "$(dirname -- "$launcher")" "$(dirname -- "$icon")"
        cp target/release/break-reminder "$staging/break-reminder"
        chmod 755 "$staging/break-reminder"
        icon_staging=$(mktemp "$(dirname -- "$icon")/.break-reminder-icon.XXXXXX")
        cp assets/app-icon.png "$icon_staging"
        chmod 644 "$icon_staging"
        cat > "$staging/launcher" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Break Reminder
Comment=Regular screen break reminders
Exec="$application"
Icon=$icon
Terminal=false
Categories=Utility;
DESKTOP
        chmod 644 "$staging/launcher"
        ensure_stopped
        mv -f -- "$icon_staging" "$icon"
        icon_staging=
        mv -f -- "$staging/break-reminder" "$application"
        cp "$staging/launcher" "$launcher"
        cp "$staging/launcher" "$startup"
        printf 'Installed: %s\nStarts at next graphical login in sessions supporting XDG autostart. Launch now: "$HOME/.local/bin/break-reminder"\n' "$application"
        case ":${PATH:-}:" in
            *":$HOME/.local/bin:"*) ;;
            *) printf 'For dmenu_run and rofi run, add "%s" to your graphical session PATH.\n' "$HOME/.local/bin" ;;
        esac
        ;;
esac
