use std::{
    env,
    ffi::OsStr,
    fs::{self, DirBuilder, File},
    io::{self, Write},
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Path, PathBuf},
    thread,
    time::Duration,
};

use log::{Level, LevelFilter, Log, Metadata, Record};
use time::{Date, Month, OffsetDateTime};

const MAX_DAILY_BYTES: u64 = 1024 * 1024;
const RETENTION_DAYS: i64 = 30;
const CLEANUP_INTERVAL: Duration = Duration::from_secs(60 * 60);

struct FileLogger {
    directory: Option<PathBuf>,
    entry_point: &'static str,
    run_id: String,
}

pub fn initialize() {
    let home = env::var_os("HOME");
    let state_home = env::var_os("XDG_STATE_HOME");
    let directory = directory(
        home.as_deref(),
        state_home.as_deref(),
        cfg!(target_os = "macos"),
    );
    if directory.is_none() {
        diagnostic("Cannot save logs: no absolute user log directory is available");
    }
    let entry_point = if env::args_os().any(|arg| arg == "--overlay") {
        "overlay"
    } else if env::args_os().any(|arg| arg == "--check-config") {
        "check-config"
    } else {
        "app"
    };
    let logger = FileLogger {
        directory: directory.clone(),
        entry_point,
        run_id: format!(
            "{}-{}",
            std::process::id(),
            OffsetDateTime::now_utc().unix_timestamp_nanos()
        ),
    };
    if let Err(error) = log::set_boxed_logger(Box::new(logger)) {
        diagnostic(format_args!("Cannot initialize logging: {error}"));
        return;
    }
    log::set_max_level(LevelFilter::Warn);
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log::error!(target: "panic", "{info}");
        previous_hook(info);
    }));
    if let Some(directory) = directory {
        maintain(&directory);
        if let Err(error) = thread::Builder::new()
            .name("log-retention".into())
            .spawn(move || {
                loop {
                    thread::park_timeout(CLEANUP_INTERVAL);
                    maintain(&directory);
                }
            })
        {
            log::warn!(target: "logging", "Cannot start log cleanup: {error}");
        }
    }
}

impl Log for FileLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= Level::Warn
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        diagnostic(record.args());
        let Some(directory) = &self.directory else {
            return;
        };
        let now = OffsetDateTime::now_utc();
        let line = serde_json::json!({
            "timestamp": format!("{}T{:02}:{:02}:{:02}.{:03}Z", now.date(), now.hour(), now.minute(), now.second(), now.millisecond()),
            "level": record.level().as_str(),
            "pid": std::process::id(),
            "run_id": self.run_id,
            "entry_point": self.entry_point,
            "event": record.target(),
            "message": record.args().to_string(),
        }).to_string() + "\n";
        if let Err(error) = append(directory, now, line.as_bytes()) {
            diagnostic(format_args!(
                "Cannot write error log in {}: {error}",
                directory.display()
            ));
        }
    }

    // Entries are written synchronously without a userspace buffer, including fatal exits.
    fn flush(&self) {}
}

fn diagnostic(message: impl std::fmt::Display) {
    let _ = writeln!(io::stderr().lock(), "{message}");
}

fn maintain(directory: &Path) {
    if let Err(error) = create_directory(directory)
        .and_then(|()| prune(directory, OffsetDateTime::now_utc().date()))
    {
        log::warn!(target: "logging", "Cannot clean up logs in {}: {error}", directory.display());
    }
}

fn create_directory(directory: &Path) -> io::Result<()> {
    DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(directory)
}

fn directory(home: Option<&OsStr>, state_home: Option<&OsStr>, macos: bool) -> Option<PathBuf> {
    let home = home.map(Path::new).filter(|path| path.is_absolute());
    if macos {
        home.map(|path| path.join("Library/Logs/break-reminder"))
    } else {
        state_home
            .map(Path::new)
            .filter(|path| path.is_absolute())
            .map(Path::to_path_buf)
            .or_else(|| home.map(|path| path.join(".local/state")))
            .map(|path| path.join("break-reminder"))
    }
}

fn prune(directory: &Path, today: Date) -> io::Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        if !file_type.is_file() {
            continue;
        }
        let name = entry.file_name();
        let Some(date) = name.to_str().and_then(log_date) else {
            continue;
        };
        if (today - date).whole_days() >= RETENTION_DAYS {
            match fs::remove_file(entry.path()) {
                Ok(()) => {}
                // The parent and overlay may clean up at the same time.
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
    }
    Ok(())
}

fn log_date(name: &str) -> Option<Date> {
    let date = name.strip_prefix("break-reminder-")?.strip_suffix(".log")?;
    if date.len() != 10 {
        return None;
    }
    let mut parts = date.split('-');
    let year = parts.next()?.parse().ok()?;
    let month = Month::try_from(parts.next()?.parse::<u8>().ok()?).ok()?;
    let day = parts.next()?.parse().ok()?;
    let parsed = Date::from_calendar_date(year, month, day).ok()?;
    (date == parsed.to_string()).then_some(parsed)
}

fn append(directory: &Path, now: OffsetDateTime, line: &[u8]) -> io::Result<()> {
    create_directory(directory)?;
    if let Err(error) = prune(directory, now.date()) {
        diagnostic(format_args!(
            "Cannot clean up logs in {}: {error}",
            directory.display()
        ));
    }
    let mut file = File::options()
        .append(true)
        .create(true)
        .mode(0o600)
        .open(directory.join(format!("break-reminder-{}.log", now.date())))?;
    // Serialize app/overlay writers so records and the daily size limit stay intact.
    file.lock()?;
    if file.metadata()?.len().saturating_add(line.len() as u64) <= MAX_DAILY_BYTES {
        file.write_all(line)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        sync::atomic::{AtomicU64, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };

    struct TestDirectory(PathBuf);
    static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    impl TestDirectory {
        fn new() -> Self {
            let suffix = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "break-reminder-logs-{}-{suffix}-{}",
                std::process::id(),
                NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[test]
    fn paths_are_absolute_and_follow_platform_conventions() {
        let home = Some(OsStr::new("/users/test"));
        let state = Some(OsStr::new("/state"));
        assert_eq!(
            directory(home, state, true),
            Some(PathBuf::from("/users/test/Library/Logs/break-reminder"))
        );
        assert_eq!(
            directory(home, state, false),
            Some(PathBuf::from("/state/break-reminder"))
        );
        for state in [None, Some(OsStr::new("")), Some(OsStr::new("relative"))] {
            assert_eq!(
                directory(home, state, false),
                Some(PathBuf::from("/users/test/.local/state/break-reminder"))
            );
        }
        assert_eq!(
            directory(None, state, false),
            Some(PathBuf::from("/state/break-reminder"))
        );
        assert_eq!(directory(Some(OsStr::new("relative")), None, true), None);
    }

    #[test]
    fn retention_keeps_today_and_previous_29_days_and_preserves_other_entries() {
        let dir = TestDirectory::new();
        let today = Date::from_calendar_date(2026, time::Month::October, 4).unwrap();
        for name in [
            "break-reminder-2026-09-04.log",
            "break-reminder-2026-09-05.log",
            "break-reminder-2026-10-04.log",
            "break-reminder-2999-01-01.log",
            "other-2000-01-01.log",
            "break-reminder-2026-02-30.log",
            "break-reminder-2026-09-04.log.backup",
        ] {
            fs::write(dir.0.join(name), b"keep or delete").unwrap();
        }
        fs::create_dir(dir.0.join("break-reminder-2000-01-01.log")).unwrap();
        prune(&dir.0, today).unwrap();
        assert!(!dir.0.join("break-reminder-2026-09-04.log").exists());
        assert_eq!(fs::read_dir(&dir.0).unwrap().count(), 7);
    }

    #[test]
    fn appends_survive_restarts_and_roll_over_without_restart() {
        let dir = TestDirectory::new();
        let first = Date::from_calendar_date(2026, time::Month::October, 4)
            .unwrap()
            .midnight()
            .assume_utc();
        fs::write(
            dir.0.join("break-reminder-2026-09-05.log"),
            b"expires tomorrow",
        )
        .unwrap();
        append(&dir.0, first, b"first\n").unwrap();
        append(&dir.0, first, b"second\n").unwrap();
        append(&dir.0, first + time::Duration::days(1), b"next day\n").unwrap();
        assert_eq!(
            fs::read(dir.0.join("break-reminder-2026-10-04.log")).unwrap(),
            b"first\nsecond\n"
        );
        assert_eq!(
            fs::read(dir.0.join("break-reminder-2026-10-05.log")).unwrap(),
            b"next day\n"
        );
        assert!(!dir.0.join("break-reminder-2026-09-05.log").exists());
    }

    #[test]
    fn daily_size_cap_holds_across_writers_and_resets_next_day() {
        let dir = TestDirectory::new();
        let now = Date::from_calendar_date(2026, time::Month::October, 4)
            .unwrap()
            .midnight()
            .assume_utc();
        let path = dir.0.join("break-reminder-2026-10-04.log");
        fs::write(&path, vec![b'x'; MAX_DAILY_BYTES as usize - 4]).unwrap();
        append(&dir.0, now, b"ok\n").unwrap();
        append(&dir.0, now, b"too large\n").unwrap();
        assert_eq!(fs::metadata(path).unwrap().len(), MAX_DAILY_BYTES - 1);
        append(&dir.0, now + time::Duration::days(1), b"new budget\n").unwrap();
        assert_eq!(
            fs::read(dir.0.join("break-reminder-2026-10-05.log")).unwrap(),
            b"new budget\n"
        );
    }

    #[test]
    fn concurrent_writers_keep_complete_records() {
        let dir = TestDirectory::new();
        let now = OffsetDateTime::now_utc();
        std::thread::scope(|scope| {
            for _ in 0..8 {
                let directory = &dir.0;
                scope.spawn(move || {
                    for _ in 0..20 {
                        append(directory, now, b"complete record\n").unwrap();
                    }
                });
            }
        });
        let text =
            fs::read_to_string(dir.0.join(format!("break-reminder-{}.log", now.date()))).unwrap();
        assert_eq!(text.lines().count(), 160);
        assert!(text.lines().all(|line| line == "complete record"));
    }

    #[test]
    fn panic_hook_writes_before_process_exit() {
        const PANIC_TEST: &str = "BREAK_REMINDER_TEST_PANIC_LOG";
        if std::env::var_os(PANIC_TEST).is_some() {
            initialize();
            panic!("simulated panic for file logging");
        }
        let dir = TestDirectory::new();
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "logging::tests::panic_hook_writes_before_process_exit",
            ])
            .env("HOME", &dir.0)
            .env_remove("XDG_STATE_HOME")
            .env(PANIC_TEST, "1")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(!result.success());
        let logs = directory(Some(dir.0.as_os_str()), None, cfg!(target_os = "macos")).unwrap();
        let file = fs::read_dir(logs).unwrap().next().unwrap().unwrap().path();
        let text = fs::read_to_string(file).unwrap();
        let entry: serde_json::Value = serde_json::from_str(text.trim()).unwrap();
        assert_eq!(entry["event"], "panic");
        assert!(
            entry["message"]
                .as_str()
                .unwrap()
                .contains("simulated panic for file logging")
        );
    }
}
