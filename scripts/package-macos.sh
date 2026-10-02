#!/bin/sh
set -eu

if [ "$(uname -s)" != Darwin ]; then
    echo "macOS is required to build the app bundle" >&2
    exit 2
fi

project_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$project_dir"
cargo build --release

bundle="$project_dir/target/macos/Break Reminder.app"
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources"
cp target/release/break-reminder "$bundle/Contents/MacOS/break-reminder"
cp assets/app-icon.icns "$bundle/Contents/Resources/app-icon.icns"
cat > "$bundle/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key><string>break-reminder</string>
    <key>CFBundleIdentifier</key><string>com.breakreminder.app</string>
    <key>CFBundleName</key><string>Break Reminder</string>
    <key>CFBundleIconFile</key><string>app-icon.icns</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>0.1.0</string>
    <key>CFBundleVersion</key><string>2</string>
    <key>LSUIElement</key><true/>
</dict>
</plist>
PLIST
plutil -lint "$bundle/Contents/Info.plist"
printf '%s\n' "$bundle"
