mod app;
mod config;
#[cfg(target_os = "macos")]
mod macos_window;
mod overlay;
mod protocol;
#[allow(dead_code)] // display_remaining remains part of the tested timer contract.
mod timing;

use std::{env, ffi::OsStr, path::PathBuf, process};

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        process::exit(2);
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args_os().skip(1);
    let mut check_config = false;
    let mut overlay_mode = false;
    let mut config_path = None;
    while let Some(arg) = args.next() {
        if arg == OsStr::new("--check-config") {
            check_config = true;
        } else if arg == OsStr::new("--overlay") {
            overlay_mode = true;
        } else if arg == OsStr::new("--config") {
            if config_path.is_some() {
                return Err("--config may only be given once".into());
            }
            let value = args.next().ok_or("--config requires a path")?;
            if value.is_empty() || value.to_string_lossy().starts_with("--") {
                return Err("--config requires a path".into());
            }
            config_path = Some(PathBuf::from(value));
        } else {
            return Err(format!("Unknown argument: {}", arg.to_string_lossy()));
        }
    }
    if overlay_mode {
        if check_config || config_path.is_some() {
            return Err("--overlay cannot be combined with other options".into());
        }
        ensure_supported_session()?;
        return overlay::run();
    }
    let path = match config_path {
        Some(path) => path,
        None => {
            let home = env::var_os("HOME")
                .filter(|home| !home.is_empty())
                .ok_or("HOME is not set")?;
            config::default_path(&PathBuf::from(home))
        }
    };
    if !check_config {
        ensure_supported_session()?;
        return app::run(&path);
    }
    config::load(&path).map_err(|error| error.to_string())?;
    let resolved = path
        .canonicalize()
        .map_err(|error| format!("{}: {error}", path.display()))?;
    println!("Config valid: {}", resolved.display());
    Ok(())
}

fn ensure_supported_session() -> Result<(), String> {
    #[cfg(target_os = "linux")]
    x11_session(
        env::var_os("XDG_SESSION_TYPE").as_deref(),
        env::var_os("DISPLAY").as_deref(),
    )
    .map_err(str::to_owned)?;
    Ok(())
}

#[cfg(any(test, target_os = "linux"))]
fn x11_session(session_type: Option<&OsStr>, display: Option<&OsStr>) -> Result<(), &'static str> {
    if session_type == Some(OsStr::new("wayland")) {
        return Err("Wayland sessions are unsupported; start Break Reminder in an X11 session");
    }
    if display.is_none_or(OsStr::is_empty) {
        return Err("DISPLAY is not set; start Break Reminder in an X11 session");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    use super::x11_session;

    #[test]
    fn linux_gui_requires_x11_session_and_display() {
        assert!(x11_session(Some(OsStr::new("x11")), Some(OsStr::new(":0"))).is_ok());
        assert!(x11_session(None, Some(OsStr::new(":0"))).is_ok());
        assert_eq!(
            x11_session(Some(OsStr::new("wayland")), Some(OsStr::new(":0"))),
            Err("Wayland sessions are unsupported; start Break Reminder in an X11 session")
        );
        assert_eq!(
            x11_session(Some(OsStr::new("x11")), None),
            Err("DISPLAY is not set; start Break Reminder in an X11 session")
        );
        assert_eq!(
            x11_session(None, Some(OsStr::new(""))),
            Err("DISPLAY is not set; start Break Reminder in an X11 session")
        );
    }
}
