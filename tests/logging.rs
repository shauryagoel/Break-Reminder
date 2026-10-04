#![cfg(any(target_os = "macos", target_os = "linux"))]

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    directory: PathBuf,
    home: PathBuf,
    working_directory: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "break-reminder-logging-{}-{suffix}-{}",
            std::process::id(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed),
        ));
        let home = directory.join("home");
        let working_directory = directory.join("launcher working directory");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(&working_directory).unwrap();
        Self {
            directory,
            home,
            working_directory,
        }
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_break-reminder"));
        command
            .env_clear()
            .env("HOME", &self.home)
            .env("XDG_SESSION_TYPE", "x11")
            .env("DISPLAY", ":0")
            .current_dir(&self.working_directory)
            .stdin(Stdio::null())
            .stderr(Stdio::null());
        command
    }

    fn log_directory(&self) -> PathBuf {
        #[cfg(target_os = "macos")]
        {
            self.home.join("Library/Logs/break-reminder")
        }
        #[cfg(target_os = "linux")]
        {
            self.home.join(".local/state/break-reminder")
        }
    }

    fn logs(&self) -> String {
        read_logs(&self.log_directory())
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

fn read_logs(directory: &Path) -> String {
    let mut files: Vec<_> = fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", directory.display()))
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.is_file()
                && path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("break-reminder-")
                && path.extension().is_some_and(|extension| extension == "log")
        })
        .collect();
    files.sort();
    files
        .iter()
        .map(|path| fs::read_to_string(path).unwrap())
        .collect::<Vec<_>>()
        .join("")
}

fn assert_error(output: &Output) {
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
}

#[test]
fn launcher_errors_are_logged_without_stderr_or_a_project_working_directory() {
    let fixture = Fixture::new();
    let output = fixture
        .command()
        .arg("--invalid-startup-option")
        .output()
        .unwrap();
    assert_error(&output);
    assert!(output.stderr.is_empty());
    assert!(
        fixture
            .logs()
            .contains("Unknown argument: --invalid-startup-option")
    );
    assert_eq!(fs::read_dir(&fixture.working_directory).unwrap().count(), 0);
    let logs = fixture.logs();
    let entry: serde_json::Value = serde_json::from_str(logs.trim()).unwrap();
    assert_eq!(entry["event"], "startup_failed");
    assert_eq!(entry["level"], "ERROR");
    assert_eq!(entry["entry_point"], "app");
    assert!(entry["pid"].as_u64().unwrap() > 0);
    assert!(!entry["run_id"].as_str().unwrap().is_empty());
    assert!(entry["timestamp"].as_str().unwrap().ends_with('Z'));
}

#[test]
fn config_checks_log_errors_and_preserve_successful_stdout() {
    let fixture = Fixture::new();
    let config = fixture.directory.join("config.yaml");
    fs::write(&config, "interval_minutes: 0\n").unwrap();
    let invalid = fixture
        .command()
        .arg("--check-config")
        .arg("--config")
        .arg(&config)
        .output()
        .unwrap();
    assert_error(&invalid);
    assert!(fixture.logs().contains("interval_minutes"));

    fs::write(&config, "interval_minutes: 25\n").unwrap();
    let valid = fixture
        .command()
        .arg("--check-config")
        .arg("--config")
        .arg(&config)
        .output()
        .unwrap();
    assert!(valid.status.success(), "{valid:?}");
    assert_eq!(
        String::from_utf8(valid.stdout).unwrap(),
        format!(
            "Config valid: {}\n",
            config.canonicalize().unwrap().display()
        )
    );
}

#[test]
fn overlay_startup_errors_use_file_logging_without_writing_to_protocol_stdout() {
    let fixture = Fixture::new();
    let output = fixture.command().arg("--overlay").output().unwrap();
    assert_error(&output);
    assert!(fixture.logs().contains("invalid overlay settings"));
    let logs = fixture.logs();
    let entry: serde_json::Value = serde_json::from_str(logs.trim()).unwrap();
    assert_eq!(entry["entry_point"], "overlay");
}

#[test]
fn startup_expires_managed_logs_and_preserves_other_files_and_existing_entries() {
    let fixture = Fixture::new();
    let logs = fixture.log_directory();
    fs::create_dir_all(&logs).unwrap();
    let expired = logs.join("break-reminder-2000-01-01.log");
    let foreign = logs.join("unrelated.txt");
    let archive = logs.join("break-reminder-2000-01-01.log.bak");
    let invalid_date = logs.join("break-reminder-2000-13-01.log");
    let future = logs.join("break-reminder-2999-12-31.log");
    let directory = logs.join("break-reminder-2000-01-02.log");
    for path in [&expired, &foreign, &archive, &invalid_date, &future] {
        fs::write(path, "preserve this content\n").unwrap();
    }
    fs::create_dir(&directory).unwrap();
    fs::write(directory.join("keep.txt"), "keep directory contents").unwrap();

    let first = fixture
        .command()
        .arg("--first-logged-error")
        .output()
        .unwrap();
    assert_error(&first);
    assert!(!expired.exists(), "expired managed log was retained");
    for path in [&foreign, &archive, &invalid_date, &future] {
        assert_eq!(fs::read_to_string(path).unwrap(), "preserve this content\n");
    }
    assert_eq!(
        fs::read_to_string(directory.join("keep.txt")).unwrap(),
        "keep directory contents"
    );

    let second = fixture
        .command()
        .arg("--second-logged-error")
        .output()
        .unwrap();
    assert_error(&second);
    let content = fixture.logs();
    assert_eq!(
        content
            .matches("Unknown argument: --first-logged-error")
            .count(),
        1
    );
    assert_eq!(
        content
            .matches("Unknown argument: --second-logged-error")
            .count(),
        1
    );
}

#[test]
fn unusable_log_directory_preserves_stderr_and_error_exit_status() {
    let fixture = Fixture::new();
    let logs = fixture.log_directory();
    fs::create_dir_all(logs.parent().unwrap()).unwrap();
    fs::write(&logs, "a file blocks the log directory").unwrap();
    let output = fixture
        .command()
        .arg("--unwritable-log-directory")
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    assert_error(&output);
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("Unknown argument: --unwritable-log-directory")
    );
    assert_eq!(
        fs::read_to_string(logs).unwrap(),
        "a file blocks the log directory"
    );
}

#[test]
fn concurrent_processes_append_complete_records_and_respect_the_daily_cap() {
    let fixture = Fixture::new();
    let run_batch = || {
        let mut children: Vec<_> = (0..8)
            .map(|index| {
                fixture
                    .command()
                    .arg(format!("--concurrent-error-{index}"))
                    .stdout(Stdio::null())
                    .spawn()
                    .unwrap()
            })
            .collect();
        for child in &mut children {
            assert_eq!(child.wait().unwrap().code(), Some(2));
        }
    };
    run_batch();
    let logs = fixture.logs();
    let entries: Vec<serde_json::Value> = logs
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(entries.len(), 8);
    for index in 0..8 {
        assert_eq!(
            entries
                .iter()
                .filter(|entry| entry["message"]
                    == format!("Unknown argument: --concurrent-error-{index}"))
                .count(),
            1
        );
    }

    let path = fs::read_dir(fixture.log_directory())
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    const CAP: usize = 1024 * 1024;
    let seed = serde_json::json!({"padding": "x".repeat(CAP - 600)}).to_string() + "\n";
    fs::write(&path, &seed).unwrap();
    run_batch();
    let size = fs::metadata(&path).unwrap().len() as usize;
    assert!(
        size > seed.len(),
        "the remaining budget should allow at least one error"
    );
    assert!(size <= CAP, "concurrent processes exceeded the daily cap");
    for line in fs::read_to_string(path).unwrap().lines() {
        serde_json::from_str::<serde_json::Value>(line).unwrap();
    }
}

#[cfg(target_os = "linux")]
#[test]
fn linux_logging_honors_absolute_state_home_and_ignores_relative_overrides() {
    let fixture = Fixture::new();
    let state = fixture.directory.join("state");
    let absolute = fixture
        .command()
        .env("XDG_STATE_HOME", &state)
        .arg("--absolute-state-error")
        .output()
        .unwrap();
    assert_error(&absolute);
    assert!(
        read_logs(&state.join("break-reminder"))
            .contains("Unknown argument: --absolute-state-error")
    );

    let relative = fixture
        .command()
        .env("XDG_STATE_HOME", "relative-state")
        .arg("--relative-state-error")
        .output()
        .unwrap();
    assert_error(&relative);
    assert!(
        fixture
            .logs()
            .contains("Unknown argument: --relative-state-error")
    );
    assert!(!fixture.working_directory.join("relative-state").exists());
}
