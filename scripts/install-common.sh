# Shared preflight for install.sh and uninstall.sh; sourced, not run directly.

fail() {
    printf '%s\n' "$*" >&2
    exit 2
}

validate_path() {
    case "$2" in
        /*) ;;
        *) fail "$1 must be a nonempty absolute path" ;;
    esac
    case "$2" in
        *[[:cntrl:]]*) fail "$1 contains unsupported control characters" ;;
    esac
    if [ "$install_os" = Linux ]; then
        case "$2" in
            *'"'*|*'`'*|*'$'*|*'\'*|*'%'*|*'='*)
                fail "$1 contains characters unsupported in Linux desktop entries" ;;
        esac
    fi
}

check_destination() {
    destination=$1
    while [ "$destination" != / ]; do
        [ ! -L "$destination" ] || fail "Refusing symlink installation path: $destination"
        destination=$(dirname -- "$destination")
    done
}

ensure_stopped() {
    if pgrep -u "$(id -u)" -x break-reminder >/dev/null; then
        fail "Quit Break Reminder from its menu before installing or uninstalling."
    else
        pgrep_status=$?
        [ "$pgrep_status" -eq 1 ] || fail "Cannot check running instances (pgrep status $pgrep_status)"
    fi
}

install_os=$(uname -s)
case "$install_os" in
    Darwin|Linux) ;;
    *) fail "Break Reminder installation supports macOS and Linux only" ;;
esac
validate_path HOME "${HOME:-}"
case "$install_os" in
    Darwin)
        application="$HOME/Applications/Break Reminder.app"
        startup="$HOME/Library/LaunchAgents/com.breakreminder.app.plist"
        log_path="$HOME/Library/Logs/break-reminder.log"
        check_destination "$application"
        check_destination "$startup"
        check_destination "$log_path"
        [ ! -e "$application" ] || [ -d "$application" ] || fail "Expected app bundle directory: $application"
        ;;
    Linux)
        data_home=${XDG_DATA_HOME:-$HOME/.local/share}
        config_home=${XDG_CONFIG_HOME:-$HOME/.config}
        validate_path XDG_DATA_HOME "$data_home"
        validate_path XDG_CONFIG_HOME "$config_home"
        application="$HOME/.local/bin/break-reminder"
        launcher="$data_home/applications/com.breakreminder.app.desktop"
        icon="$data_home/icons/com.breakreminder.app.png"
        startup="$config_home/autostart/com.breakreminder.app.desktop"
        check_destination "$application"
        check_destination "$launcher"
        check_destination "$icon"
        check_destination "$startup"
        [ ! -e "$application" ] || [ -f "$application" ] || fail "Expected executable file: $application"
        [ ! -e "$launcher" ] || [ -f "$launcher" ] || fail "Expected launcher file: $launcher"
        [ ! -e "$icon" ] || [ -f "$icon" ] || fail "Expected icon file: $icon"
        ;;
esac
[ ! -e "$startup" ] || [ -f "$startup" ] || fail "Expected startup file: $startup"
ensure_stopped
