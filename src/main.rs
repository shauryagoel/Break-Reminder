mod config;
mod overlay;
#[allow(dead_code)] // Snapshot writing and action parsing are used by the tray parent in Task 3.
mod protocol;
#[allow(dead_code)] // The app shell uses this module in the reminder-window increment.
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
        return overlay::run();
    }
    if !check_config {
        return Err("Usage: break-reminder --check-config [--config PATH]".into());
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
    config::load(&path).map_err(|error| error.to_string())?;
    let resolved = path
        .canonicalize()
        .map_err(|error| format!("{}: {error}", path.display()))?;
    println!("Config valid: {}", resolved.display());
    Ok(())
}
