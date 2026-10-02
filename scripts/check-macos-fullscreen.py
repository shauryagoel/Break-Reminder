"""Check overlay visibility in another app's macOS full-screen Space.

Run cargo build, then python3 scripts/check-macos-fullscreen.py.
The temporary full-screen test window and overlay close automatically.
"""

import json
import selectors
import struct
import subprocess
import sys
import tempfile
import time
from pathlib import Path

if sys.platform != 'darwin':
    raise SystemExit('macOS is required for this window check')

SWIFT = r'''
import AppKit
import CoreGraphics

if CommandLine.arguments.count > 1 {
    let pids = Set(CommandLine.arguments.dropFirst().compactMap(Int32.init))
    // Only the onscreen-only list guarantees front-to-back order.
    let windows = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as! [[String: Any]]
    let owned = windows.enumerated().compactMap { index, row -> [String: Any]? in
        guard let pid = row[kCGWindowOwnerPID as String] as? Int32, pids.contains(pid) else { return nil }
        return ["pid": pid, "order": index, "onscreen": row[kCGWindowIsOnscreen as String] as? Bool ?? false,
                "level": row[kCGWindowLayer as String] ?? 0, "bounds": row[kCGWindowBounds as String] ?? [:]]
    }
    print(String(data: try! JSONSerialization.data(withJSONObject: owned), encoding: .utf8)!)
    exit(0)
}
let app = NSApplication.shared
app.setActivationPolicy(.regular)
let window = NSWindow(contentRect: NSRect(x: 100, y: 100, width: 800, height: 600),
                      styleMask: [.titled, .closable, .resizable], backing: .buffered, defer: false)
window.title = "Break Reminder full-screen test (closes automatically)"
window.collectionBehavior = [.fullScreenPrimary]
let entered = NotificationCenter.default.addObserver(forName: NSWindow.didEnterFullScreenNotification,
    object: window, queue: .main) { _ in print("FULLSCREEN"); fflush(stdout) }
window.makeKeyAndOrderFront(nil)
app.activate(ignoringOtherApps: true)
DispatchQueue.main.asyncAfter(deadline: .now() + 0.3) { window.toggleFullScreen(nil) }
DispatchQueue.main.asyncAfter(deadline: .now() + 20) { app.terminate(nil) }
app.run()
'''


def line(process, timeout):
    with selectors.DefaultSelector() as selector:
        selector.register(process.stdout, selectors.EVENT_READ)
        assert selector.select(timeout), 'timed out waiting for child output'
        return process.stdout.readline().decode().strip()


def stop(process):
    if process is not None and process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=3)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=3)


binary = Path(sys.argv[1] if len(sys.argv) > 1 else 'target/debug/break-reminder').resolve()
assert binary.is_file(), 'build the app first with cargo build'
host = overlay = None
with tempfile.TemporaryDirectory(prefix='break-reminder-fullscreen-') as temporary:
    directory = Path(temporary)
    source = directory / 'host.swift'
    fixture = directory / 'host'
    source.write_text(SWIFT)
    subprocess.run(['xcrun', 'swiftc', '-module-cache-path', str(directory / 'cache'),
                    str(source), '-o', str(fixture)], check=True, timeout=60)
    try:
        host = subprocess.Popen([str(fixture)], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        assert line(host, 8) == 'FULLSCREEN', 'fixture did not enter native full screen'
        time.sleep(0.4)
        overlay = subprocess.Popen([str(binary), '--overlay'], stdin=subprocess.PIPE,
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        snapshot = b'''duration_seconds: 2
postpone_minutes: [1, 5, 10]
title: Full-screen reminder check
message: Checking countdown, Skip and Postpone
background_color: '#16202A'
text_color: '#FFFFFF'
accent_color: '#51B682'
image: null
'''
        overlay.stdin.write(struct.pack('>I', len(snapshot)) + snapshot)
        overlay.stdin.flush()
        ready = line(overlay, 8)
        assert ready == 'READY', 'overlay did not become ready: ' + ready
        rows = json.loads(subprocess.check_output([str(fixture), str(host.pid), str(overlay.pid)], timeout=4))
        visible = {pid: [row for row in rows if row['pid'] == pid and row['onscreen']
                         and row['bounds'].get('Width', 0) >= 800]
                   for pid in (host.pid, overlay.pid)}
        if visible[host.pid]:
            host_bounds = max(visible[host.pid], key=lambda row: row['bounds']['Width'] * row['bounds']['Height'])['bounds']
            visible[overlay.pid] = [row for row in visible[overlay.pid] if row['bounds'] == host_bounds]
        print(json.dumps({'host_visible': bool(visible[host.pid]),
                          'overlay_visible': bool(visible[overlay.pid])}), flush=True)
        assert visible[host.pid], 'overlay switched away from the existing full-screen Space'
        assert visible[overlay.pid], 'reminder is absent from the full-screen Space despite READY'
        assert min(row['order'] for row in visible[overlay.pid]) < min(row['order'] for row in visible[host.pid]), 'reminder is behind full-screen window'
        start = time.monotonic()
        overlay.stdin.write(b'START\n')
        overlay.stdin.flush()
        assert line(overlay, 5) == 'ELAPSED', 'countdown did not finish'
        assert time.monotonic() - start >= 1.8, 'countdown started before START'
        assert overlay.wait(timeout=3) == 0, 'overlay failed to exit'
        print('Full-screen overlay visibility and countdown check passed')
    finally:
        stop(overlay)
        stop(host)
