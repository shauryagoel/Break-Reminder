use std::{
    io::Read,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

#[cfg(target_os = "macos")]
const MACOS_SCRIPT: &str = include_str!("../scripts/pause-media-macos.js");

pub fn pause() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = Command::new("/usr/bin/osascript");
        command.args(["-l", "JavaScript", "-e", MACOS_SCRIPT]);
        command
    };
    #[cfg(target_os = "linux")]
    let mut command = {
        let mut command = Command::new("playerctl");
        command.args(["--all-players", "pause"]).env("LC_ALL", "C");
        command
    };
    run_command(&mut command, Duration::from_secs(10))
}

fn run_command(command: &mut Command, timeout: Duration) -> Result<(), String> {
    let program = command.get_program().to_string_lossy().into_owned();
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("cannot start {program}: {error}"))?;
    // Drain stderr while waiting; a full pipe would block the helper until the timeout.
    let mut stderr = child.stderr.take().expect("piped stderr");
    let stderr = thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stderr.read_to_end(&mut bytes);
        bytes
    });
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let error = stderr.join().unwrap_or_default();
                let error = String::from_utf8_lossy(&error);
                return if status.success()
                    || (cfg!(target_os = "linux") && error.trim() == "No players found")
                {
                    Ok(())
                } else {
                    Err(format!("{program} exited with {status}: {}", error.trim()))
                };
            }
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(25)),
            result => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(match result {
                    Err(error) => format!("cannot inspect {program}: {error}"),
                    _ => format!("{program} timed out; check media automation permissions"),
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_commands_report_failures_and_time_out() {
        assert!(run_command(&mut Command::new("/usr/bin/true"), Duration::from_secs(1)).is_ok());
        let error = run_command(
            Command::new("/bin/sh").args(["-c", "printf 'permission denied' >&2; exit 7"]),
            Duration::from_secs(1),
        )
        .unwrap_err();
        assert!(
            error.contains('7') && error.contains("permission denied"),
            "{error}"
        );
        assert!(
            run_command(
                &mut Command::new("/no/such/media-helper"),
                Duration::from_secs(1)
            )
            .unwrap_err()
            .contains("cannot start")
        );
        let error = run_command(
            Command::new("/bin/sh")
                .args(["-c", "head -c 200000 /dev/zero | tr '\\0' x >&2; exit 1"]),
            Duration::from_secs(5),
        )
        .unwrap_err();
        assert!(
            !error.contains("timed out"),
            "large stderr blocked the helper"
        );
        assert!(
            run_command(Command::new("/bin/sleep").arg("1"), Duration::ZERO)
                .unwrap_err()
                .contains("timed out")
        );
        #[cfg(target_os = "linux")]
        assert!(
            run_command(
                Command::new("/bin/sh").args(["-c", "printf 'No players found\\n' >&2; exit 1"]),
                Duration::from_secs(1),
            )
            .is_ok()
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_automation_pauses_all_supported_media_and_continues_after_errors() {
        let script = format!(
            r#"(function() {{
                const media = [false, true, false, false].map(function(paused) {{
                    return {{paused: paused, pause: function() {{ this.paused = true; }} }};
                }});
                const frame = {{ document: {{ querySelectorAll: function() {{ return [media[2]]; }} }}, frames: [] }};
                const blockedFrame = {{ get document() {{ throw Error('cross-origin'); }} }};
                const window = {{
                    document: {{ querySelectorAll: function() {{ return media.slice(0, 2); }} }},
                    frames: [blockedFrame, frame]
                }};
                const tab = {{
                    url: function() {{ return 'https://example.test'; }},
                    execute: function(options) {{ Function('window', options.javascript)(window); }}
                }};
                const blockedTab = {{
                    url: tab.url, execute: function() {{ throw Error('JavaScript permission denied'); }}
                }};
                const unresponsiveTab = {{
                    url: tab.url, execute: function() {{ throw Error('waited for a background tab'); }}
                }};
                const internalTab = {{
                    url: function() {{ return 'chrome://settings'; }},
                    execute: function() {{ throw Error('internal page was not skipped'); }}
                }};
                let music = 'playing';
                const movie = {{ playing: function() {{ return true; }}, rate: 1 }};
                const pausedMovie = {{ playing: function() {{ return false; }}, rate: 0 }};
                let safariPaused = false;
                const apps = {{
                    'com.apple.Music': {{ playerState: function() {{ return music; }}, pause: function() {{ music = 'paused'; }} }},
                    'com.apple.TV': {{ playerState: function() {{ return 'paused'; }}, pause: function() {{ throw Error('paused player changed'); }} }},
                    'com.spotify.client': {{ playerState: function() {{ throw Error('Automation permission denied'); }} }},
                    'com.google.Chrome': {{ windows: function() {{ return [
                        {{ activeTab: blockedTab, tabs: function() {{ return [blockedTab]; }} }},
                        {{ activeTab: tab, tabs: function() {{ return [unresponsiveTab, tab, internalTab]; }} }}
                    ]; }} }},
                    'com.apple.Safari': {{
                        windows: function() {{ return [{{ currentTab: tab, tabs: function() {{ return [tab]; }} }}]; }},
                        doJavaScript: function(source) {{
                            const window = {{ document: {{ querySelectorAll: function() {{ return [media[3]]; }} }}, frames: [] }};
                            Function('window', source)(window); safariPaused = true;
                        }}
                    }},
                    'com.apple.QuickTimePlayerX': {{ documents: function() {{ return [movie, pausedMovie]; }} }}
                }};
                const ids = ['unsupported.app'].concat(Object.keys(apps));
                const Application = function(id) {{ if (!apps[id]) throw Error('unsupported app was opened'); return apps[id]; }};
                Application.currentApplication = function() {{ return {{ runScript: function(source, options) {{
                    if (source.indexOf('ignoring application responses') < 0) throw Error('waited for a background tab');
                    const id = source.match(/tell application id "([^"]+)"/)[1];
                    if (id === 'com.apple.Safari' && source.indexOf('tab id') >= 0) throw Error('Safari tabs have no id');
                    const activePause = id === 'com.apple.Safari'
                        ? 'do JavaScript pauseCode in browserTab' : 'execute browserTab javascript pauseCode';
                    const firstPause = source.indexOf(activePause);
                    const metadata = source.indexOf('every tab of browserWindow');
                    if (metadata < 0 || firstPause < 0 || firstPause > metadata)
                        throw Error('active tabs must pause before bulk metadata');
                    if (firstPause > source.indexOf('ignoring application responses'))
                        throw Error('active-tab pause must wait for permission errors');
                    if (id !== 'com.apple.Safari' && source.indexOf('get id of every tab of browserWindow',
                        source.indexOf('get URL of every tab of browserWindow')) < 0)
                        throw Error('Chromium tab ids must be re-read after URLs');
                    if (music !== 'paused' || movie.rate !== 0) throw Error('native players must pause before browsers');
                    const errors = [];
                    apps[id].windows().forEach(function(win) {{
                        try {{
                            if (id === 'com.apple.Safari') apps[id].doJavaScript(options.withParameters[0], {{in: win.currentTab}});
                            else win.activeTab.execute({{javascript: options.withParameters[0]}});
                        }} catch (error) {{ errors.push(error.message); }}
                        win.tabs().forEach(function(item) {{
                            if (item === unresponsiveTab || !/^(https?|file):/i.test(item.url())) return;
                            try {{
                                if (id === 'com.apple.Safari') apps[id].doJavaScript(options.withParameters[0], {{in: item}});
                                else item.execute({{javascript: options.withParameters[0]}});
                            }} catch (_) {{}}
                        }});
                    }});
                    return errors;
                }} }}; }};
                const ObjC = {{ import: function() {{}}, unwrap: function(value) {{ return value; }} }};
                const $ = {{ NSWorkspace: {{ sharedWorkspace: {{ runningApplications: {{
                    count: ids.length, objectAtIndex: function(i) {{ return {{ bundleIdentifier: ids[i] }}; }}
                }} }} }} }};
                {MACOS_SCRIPT}
                let error;
                try {{ run(); }} catch (caught) {{ error = caught.message; }}
                if (music !== 'paused' || movie.rate !== 0 || pausedMovie.rate !== 0 || !safariPaused ||
                    media.some(function(item) {{ return !item.paused; }})) throw Error('media kept playing: ' +
                        JSON.stringify({{music: music, rate: movie.rate, safari: safariPaused,
                            paused: media.map(function(item) {{ return item.paused; }}), errors: error}}));
                if (!error || error.indexOf('Automation permission denied') < 0 ||
                    error.indexOf('JavaScript permission denied') < 0 ||
                    error.indexOf('internal page was not skipped') >= 0 ||
                    error.indexOf('waited for a background tab') >= 0 ||
                    error.indexOf('paused player changed') >= 0) throw Error('incorrect error handling: ' + error);
            }})();"#
        );
        run_command(
            Command::new("/usr/bin/osascript").args(["-l", "JavaScript", "-e", &script]),
            Duration::from_secs(5),
        )
        .unwrap();
    }
}
