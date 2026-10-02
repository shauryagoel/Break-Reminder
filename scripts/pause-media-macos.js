function run() {
    ObjC.import('AppKit');
    const applications = $.NSWorkspace.sharedWorkspace.runningApplications;
    const errors = new Set();
    const players = ['com.apple.Music', 'com.apple.iTunes', 'com.apple.TV', 'com.spotify.client'];
    const safari = ['com.apple.Safari', 'com.apple.SafariTechnologyPreview'];
    const chromium = ['com.google.Chrome', 'org.chromium.Chromium', 'com.brave.Browser',
        'com.microsoft.edgemac', 'com.vivaldi.Vivaldi'];
    const host = Application.currentApplication();
    host.includeStandardAdditions = true;
    // ponytail: same-origin frames only; a browser extension is needed for cross-origin frames.
    const javascript = `(function pause(w) {
        w.document.querySelectorAll('video, audio').forEach(function(media) { media.pause(); });
        for (var i = 0; i < w.frames.length; i++) { try { pause(w.frames[i]); } catch (_) {} }
        return 'paused';
    })(window);`;
    function attempt(id, action) {
        try { action(); } catch (error) { errors.add(id + ': ' + error.message); }
    }
    function pauseBrowser(id) {
        const isSafari = safari.indexOf(id) >= 0;
        const probe = isSafari ? 'do JavaScript "1" in current tab of browserWindow'
            : 'execute (active tab of browserWindow) javascript "1"';
        const active = isSafari ? 'current tab' : 'active tab';
        const pause = function(tab) {
            return isSafari ? `do JavaScript pauseCode in ${tab}` : `execute ${tab} javascript pauseCode`;
        };
        const backgroundTab = isSafari ? 'tab tabIndex of browserWindow'
            : '(tab id (item tabIndex of tabIds) of browserWindow)';
        // Chromium ids are re-read so URLs cannot be paired with tabs that changed mid-scan.
        // ponytail: Safari tabs have no ids; an index can shift if a tab closes mid-scan.
        const metadata = isSafari ? 'set pageURLs to get URL of every tab of browserWindow'
            : `set tabIds to get id of every tab of browserWindow
                            set pageURLs to get URL of every tab of browserWindow
                            if (get id of every tab of browserWindow) is not tabIds then error "tabs changed during scan"`;
        // ponytail: active tabs check permission; background requests do not wait for replies.
        // Selected tabs are paused again below; skipping them would cost one more Apple Event per window.
        // per-page execution errors need a browser extension if diagnostics matter.
        const source = `on run argv
            set pauseCode to item 1 of argv
            set failures to {}
            tell application id "${id}"
                set windowIds to get id of every window
                repeat with windowId in windowIds
                    set browserWindow to window id (contents of windowId)
                    try
                        with timeout of 2 seconds
                            set browserTab to ${active} of browserWindow
                            set pageURL to URL of browserTab
                            if pageURL starts with "http://" or pageURL starts with "https://" or pageURL starts with "file://" then
                                ${pause('browserTab')}
                            else
                                ${probe}
                            end if
                        end timeout
                    on error message
                        set end of failures to message
                    end try
                end repeat
                repeat with windowId in windowIds
                    set browserWindow to window id (contents of windowId)
                    try
                        with timeout of 2 seconds
                            ${metadata}
                            repeat with tabIndex from 1 to count of pageURLs
                                set pageURL to item tabIndex of pageURLs
                                if pageURL starts with "http://" or pageURL starts with "https://" or pageURL starts with "file://" then
                                    ignoring application responses
                                        ${pause(backgroundTab)}
                                    end ignoring
                                end if
                            end repeat
                        end timeout
                    on error message
                        set end of failures to message
                    end try
                end repeat
            end tell
            return failures
        end run`;
        const failures = host.runScript(source, {in: 'AppleScript', withParameters: [javascript]});
        failures.forEach(function(message) { errors.add(id + ': ' + message); });
    }
    const browsers = [];
    for (let i = 0; i < applications.count; i++) {
        const id = ObjC.unwrap(applications.objectAtIndex(i).bundleIdentifier);
        if (players.indexOf(id) >= 0) {
            attempt(id, function() {
                const app = Application(id);
                if (app.playerState() === 'playing') app.pause();
            });
        } else if (id === 'com.apple.QuickTimePlayerX') {
            attempt(id, function() {
                Application(id).documents().forEach(function(document) {
                    attempt(id, function() { if (document.playing()) document.rate = 0; });
                });
            });
        } else if (safari.indexOf(id) >= 0 || chromium.indexOf(id) >= 0) {
            browsers.push(id);
        }
    }
    // Native players are quick; pause them before browser scans can use up the helper timeout.
    browsers.forEach(function(id) { attempt(id, function() { pauseBrowser(id); }); });
    if (errors.size) throw new Error(Array.from(errors).join('\n'));
}
