#!/bin/sh
set -eu

project_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
scratch=$(mktemp -d "${TMPDIR:-/tmp}/break-reminder-install.XXXXXX")
scratch=$(CDPATH= cd -- "$scratch" && pwd -P)
trap 'rm -rf "$scratch"' 0
mkdir -p "$scratch/project/scripts" "$scratch/project/assets" "$scratch/stubs"
cp "$project_dir/scripts/"*.sh "$scratch/project/scripts/"
cp "$project_dir/assets/app-icon.icns" "$scratch/project/assets/"
project="$scratch/project"
CHECK_NATIVE_MV=$(command -v mv)

cat > "$scratch/stubs/uname" <<'STUB'
#!/bin/sh
printf '%s\n' "$CHECK_OS"
STUB
cat > "$scratch/stubs/pgrep" <<'STUB'
#!/bin/sh
[ "$*" = '-x break-reminder' ] || exit 99
if [ -n "${CHECK_PGREP_AFTER_BUILD:-}" ] && grep -qx cargo "$CHECK_LOG"; then
    exit "$CHECK_PGREP_AFTER_BUILD"
fi
exit "$CHECK_PGREP_STATUS"
STUB
cat > "$scratch/stubs/cargo" <<'STUB'
#!/bin/sh
printf '%s\n' cargo >> "$CHECK_LOG"
[ "$*" = 'build --release' ] || exit 99
if [ "$CHECK_BUILD_STATUS" != 0 ]; then
    printf '%s\n' 'stub build failed' >&2
    exit "$CHECK_BUILD_STATUS"
fi
mkdir -p target/release
printf 'build %s\n' "$CHECK_BUILD_MARKER" > target/release/break-reminder
chmod +x target/release/break-reminder
STUB
cat > "$scratch/stubs/plutil" <<'STUB'
#!/bin/sh
[ "$1" = -lint ] || exit 99
[ -f "$2" ]
STUB
cat > "$scratch/stubs/launchctl" <<'STUB'
#!/bin/sh
printf '%s\n' launchctl > "$CHECK_LOG.launchctl"
exit 99
STUB
cat > "$scratch/stubs/mv" <<'STUB'
#!/bin/sh
if [ "${CHECK_FAIL_PUBLISH:-0}" = 1 ] || [ "${CHECK_MV_INTERRUPT:-0}" = 1 ]; then
    for argument do
        case "$argument" in
            */.break-reminder.*/Break\ Reminder.app)
                printf '%s\n' 'stub bundle publication failed' >&2
                if [ "${CHECK_MV_INTERRUPT:-0}" = 1 ]; then kill -TERM "$PPID"; fi
                exit 99 ;;
        esac
    done
fi
exec "$CHECK_NATIVE_MV" "$@"
STUB
chmod +x "$scratch/stubs/"*
PATH="$scratch/stubs:$PATH"
CHECK_LOG="$scratch/commands"
CHECK_OS=Linux
CHECK_PGREP_STATUS=1
CHECK_BUILD_STATUS=0
CHECK_BUILD_MARKER=one
export PATH CHECK_LOG CHECK_OS CHECK_PGREP_STATUS CHECK_BUILD_STATUS CHECK_BUILD_MARKER CHECK_NATIVE_MV
unset XDG_DATA_HOME XDG_CONFIG_HOME CHECK_PGREP_AFTER_BUILD CHECK_FAIL_PUBLISH CHECK_MV_INTERRUPT
: > "$CHECK_LOG"

fail() { printf 'FAIL: %s\n' "$*" >&2; exit 1; }
has() { grep -F -e "$2" "$1" >/dev/null || fail "$1 lacks $2"; }
absent() { [ ! -e "$1" ] && [ ! -L "$1" ] || fail "unexpected artifact: $1"; }
run_ok() {
    if ! sh "$project/scripts/$1" > "$scratch/output" 2>&1; then
        cat "$scratch/output" >&2
        fail "$1 failed"
    fi
}
run_bad() {
    if sh "$project/scripts/$1" > "$scratch/output" 2>&1; then
        fail "$1 accepted an unsafe operation"
    fi
    [ -s "$scratch/output" ] || fail "$1 failed without a diagnostic"
}
seed_settings() {
    mkdir -p "$HOME/.config/break-reminder"
    printf '%s\n' 'preserve configuration' > "$HOME/.config/break-reminder/config.yaml"
    printf '%s\n' 'preserve lock inode contents' > "$HOME/.config/break-reminder/instance.lock"
    cp "$HOME/.config/break-reminder/config.yaml" "$scratch/config.expected"
    cp "$HOME/.config/break-reminder/instance.lock" "$scratch/lock.expected"
    lock_identity=$(ls -i "$HOME/.config/break-reminder/instance.lock")
}
settings_preserved() {
    cmp "$scratch/config.expected" "$HOME/.config/break-reminder/config.yaml" || fail 'configuration changed'
    cmp "$scratch/lock.expected" "$HOME/.config/break-reminder/instance.lock" || fail 'instance lock changed'
    [ "$(ls -i "$HOME/.config/break-reminder/instance.lock")" = "$lock_identity" ] || fail 'lock inode replaced'
}
HOME="$scratch/refused"
export HOME

# Refusal happens before building or creating installation directories.
for CHECK_OS in Darwin Linux; do
    for CHECK_PGREP_STATUS in 0 2 3; do
        for command in install.sh uninstall.sh; do
            : > "$CHECK_LOG"
            run_bad "$command"
            absent "$HOME"
            [ ! -s "$CHECK_LOG" ] || fail 'refusal performed a build or service operation'
            if [ "$CHECK_PGREP_STATUS" = 0 ]; then
                has "$scratch/output" Quit
            fi
        done
    done
done
CHECK_PGREP_STATUS=1

CHECK_OS=Darwin
HOME="$scratch/mac spaces &<>\"'\`\$\\%"
seed_settings
mkdir -p "$HOME/Applications/Other.app"
printf '%s\n' untouched > "$HOME/Applications/Other.app/unrelated"
run_ok install.sh
bundle="$HOME/Applications/Break Reminder.app"
plist="$HOME/Library/LaunchAgents/com.breakreminder.app.plist"
[ -x "$bundle/Contents/MacOS/break-reminder" ] || fail 'Mac executable missing or not executable'
has "$bundle/Contents/Info.plist" '<string>com.breakreminder.app</string>'
has "$bundle/Contents/Info.plist" '<key>LSUIElement</key><true/>'
has "$bundle/Contents/Info.plist" '<key>CFBundleIconFile</key><string>app-icon.icns</string>'
cmp "$project_dir/assets/app-icon.icns" "$bundle/Contents/Resources/app-icon.icns" || fail 'Mac app icon missing or changed during installation'
encoded_home=$(printf '%s' "$HOME" | sed 's/\&/\&amp;/g; s/</\&lt;/g; s/>/\&gt;/g; s/"/\&quot;/g; s/'"'"'/\&apos;/g')
has "$plist" "$encoded_home/Applications/Break Reminder.app/Contents/MacOS/break-reminder"
has "$plist" "$encoded_home/Library/Logs/break-reminder.log"
has "$plist" '<key>RunAtLoad</key>'
has "$plist" '<true/>'
has "$plist" '<string>Aqua</string>'
if grep -F -e KeepAlive "$plist" >/dev/null; then fail 'KeepAlive would relaunch after Quit'; fi
[ -d "$HOME/Library/Logs" ] || fail 'Mac login error-log directory missing'
settings_preserved
CHECK_BUILD_MARKER=two
run_ok install.sh
has "$bundle/Contents/MacOS/break-reminder" 'build two'
absent "$bundle/Break Reminder.app"
settings_preserved
CHECK_BUILD_STATUS=7
run_bad install.sh
has "$bundle/Contents/MacOS/break-reminder" 'build two'
settings_preserved
CHECK_BUILD_STATUS=0
CHECK_BUILD_MARKER=failed-replacement
CHECK_FAIL_PUBLISH=1
export CHECK_FAIL_PUBLISH
cp "$plist" "$scratch/plist.expected"
run_bad install.sh
has "$bundle/Contents/MacOS/break-reminder" 'build two'
cmp "$scratch/plist.expected" "$plist" || fail 'failed bundle publication replaced registration'
settings_preserved
unset CHECK_FAIL_PUBLISH
CHECK_MV_INTERRUPT=1
export CHECK_MV_INTERRUPT
run_bad install.sh
has "$bundle/Contents/MacOS/break-reminder" 'build two'
cmp "$scratch/plist.expected" "$plist" || fail 'interrupted installation replaced registration'
settings_preserved
unset CHECK_MV_INTERRUPT
run_ok uninstall.sh
absent "$bundle"
absent "$plist"
settings_preserved
run_ok uninstall.sh
settings_preserved
has "$HOME/Applications/Other.app/unrelated" untouched

CHECK_OS=Linux
HOME="$scratch/linux spaces & '"
seed_settings
mkdir -p "$HOME/.local/bin"
printf '%s\n' untouched > "$HOME/.local/bin/unrelated"
XDG_DATA_HOME=
XDG_CONFIG_HOME=
export XDG_DATA_HOME XDG_CONFIG_HOME
run_ok install.sh
binary="$HOME/.local/bin/break-reminder"
desktop="$HOME/.local/share/applications/com.breakreminder.app.desktop"
autostart="$HOME/.config/autostart/com.breakreminder.app.desktop"
[ -x "$binary" ] || fail 'Linux executable missing or not executable'
for entry in "$desktop" "$autostart"; do
    has "$entry" 'Type=Application'
    has "$entry" 'Name=Break Reminder'
    has "$entry" 'Terminal=false'
    has "$entry" "Exec=\"$binary\""
done
has "$desktop" 'Categories=Utility;'
has "$scratch/output" PATH
settings_preserved
CHECK_BUILD_MARKER=three
run_ok install.sh
has "$binary" 'build three'
settings_preserved
CHECK_BUILD_STATUS=7
run_bad install.sh
has "$binary" 'build three'
settings_preserved
CHECK_BUILD_STATUS=0
run_ok uninstall.sh
absent "$binary"
absent "$desktop"
absent "$autostart"
settings_preserved
run_ok uninstall.sh
settings_preserved
has "$HOME/.local/bin/unrelated" untouched

HOME="$scratch/xdg-home"
XDG_DATA_HOME="$scratch/custom data"
XDG_CONFIG_HOME="$scratch/custom config"
seed_settings
run_ok install.sh
desktop="$XDG_DATA_HOME/applications/com.breakreminder.app.desktop"
autostart="$XDG_CONFIG_HOME/autostart/com.breakreminder.app.desktop"
[ -f "$desktop" ] && [ -f "$autostart" ] || fail 'XDG overrides ignored'
absent "$HOME/.local/share/applications/com.breakreminder.app.desktop"
absent "$HOME/.config/autostart/com.breakreminder.app.desktop"
run_ok uninstall.sh
absent "$desktop"
absent "$autostart"
settings_preserved
unset XDG_DATA_HOME XDG_CONFIG_HOME

# Registration paths must be files, so a directory cannot swallow a copy.
HOME="$scratch/directory-registration"
registration="$HOME/.config/autostart/com.breakreminder.app.desktop"
mkdir -p "$registration"
printf '%s\n' untouched > "$registration/unrelated"
run_bad install.sh
run_bad uninstall.sh
has "$registration/unrelated" untouched
absent "$HOME/.local/bin/break-reminder"

# Desktop Exec does not accept these paths without extra escaping.
for unsafe in '"' '`' '$' '\' '%' '=' "$(printf 'line\nbreak')" "$(printf 'line\rbreak')" "$(printf '\t')" "$(printf '\001')"; do
    HOME="$scratch/unsafe-$unsafe"
    for command in install.sh uninstall.sh; do
        run_bad "$command"
        absent "$HOME"
    done
done
CHECK_OS=Darwin
HOME="$scratch/mac-control-$(printf '\r')"
run_bad install.sh
run_bad uninstall.sh
absent "$HOME"
CHECK_OS=Linux
HOME="$scratch/invalid-xdg"
for variable in XDG_DATA_HOME XDG_CONFIG_HOME; do
    for unsafe in relative "$scratch/unsafe%data"; do
        export "$variable=$unsafe"
        run_bad install.sh
        run_bad uninstall.sh
        absent "$HOME"
    done
    unset "$variable"
done
for HOME in '' relative; do
    run_bad install.sh
    run_bad uninstall.sh
done
HOME="$scratch/unsupported"
CHECK_OS=FreeBSD
run_bad install.sh
run_bad uninstall.sh
absent "$HOME"

# A parent appearing during the build must also prevent publication.
for CHECK_OS in Darwin Linux; do
    HOME="$scratch/raced-$CHECK_OS"
    : > "$CHECK_LOG"
    CHECK_PGREP_AFTER_BUILD=0
    export CHECK_PGREP_AFTER_BUILD
    run_bad install.sh
    has "$scratch/output" Quit
    absent "$HOME"
done
unset CHECK_PGREP_AFTER_BUILD

# Neither a destination nor an ancestor may redirect installation/removal.
printf '%s\n' untouched > "$scratch/outside"
for CHECK_OS in Darwin Linux; do
    if [ "$CHECK_OS" = Darwin ]; then
        targets='Applications/Break Reminder.app
Library/LaunchAgents/com.breakreminder.app.plist'
    else
        targets='.local/bin/break-reminder
.local/share/applications/com.breakreminder.app.desktop
.config/autostart/com.breakreminder.app.desktop'
    fi
    printf '%s\n' "$targets" | while IFS= read -r target; do
        HOME="$scratch/symlink-$CHECK_OS"
        rm -rf "$HOME"
        mkdir -p "$(dirname -- "$HOME/$target")"
        ln -s "$scratch/outside" "$HOME/$target"
        run_bad install.sh
        run_bad uninstall.sh
        has "$scratch/outside" untouched
        [ -L "$HOME/$target" ] || fail 'destination symlink removed'
    done
done
CHECK_OS=Linux
HOME="$scratch/symlink-ancestor"
mkdir -p "$HOME" "$scratch/outside-directory"
ln -s "$scratch/outside-directory" "$HOME/.local"
run_bad install.sh
run_bad uninstall.sh
absent "$scratch/outside-directory/bin"
absent "$CHECK_LOG.launchctl"
printf '%s\n' 'Installer checks passed (isolated macOS/Linux layouts; no live login registration).'
