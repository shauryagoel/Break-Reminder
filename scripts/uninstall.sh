#!/bin/sh
set -eu

project_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
. "$project_dir/scripts/install-common.sh"

case "$install_os" in
    Darwin) rm -rf -- "$application" ;;
    Linux) rm -f -- "$application" "$launcher" "$icon" ;;
esac
rm -f -- "$startup"
printf '%s\n' 'Break Reminder uninstalled. Your configuration and logs have been preserved.'
