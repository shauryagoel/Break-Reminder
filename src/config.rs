use std::{
    collections::HashSet,
    env,
    error::Error,
    fmt, fs,
    io::{self, Write},
    path::{Path, PathBuf},
    time::Duration,
};

use serde::Deserialize;

const SAMPLE: &str = include_str!("../assets/default-config.yaml");

pub const MAX_INTERVAL_MINUTES: u32 = 1_440;
pub const MAX_DURATION_SECONDS: u32 = 3_600;
pub const MAX_POSTPONE_MINUTES: u32 = 1_440;
pub const MAX_POSTPONE_CHOICES: usize = 12;
pub const DEFAULT_BACKGROUND_TRANSPARENCY_PERCENT: u32 = 15;

#[derive(Debug, PartialEq)]
pub struct Config {
    pub interval: Duration,
    pub display: Duration,
    pub postpone: Vec<Duration>,
    pub pause_media: bool,
    pub appearance: Appearance,
}

impl Default for Config {
    fn default() -> Self {
        from_raw(RawConfig::default())
    }
}

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RawConfig {
    interval_minutes: u32,
    duration_seconds: u32,
    postpone_minutes: Vec<u32>,
    pause_media: bool,
    appearance: Appearance,
}

impl Default for RawConfig {
    fn default() -> Self {
        Self {
            interval_minutes: 60,
            duration_seconds: 30,
            postpone_minutes: vec![10, 15],
            pause_media: false,
            appearance: Appearance::default(),
        }
    }
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Appearance {
    pub title: String,
    pub message: String,
    pub background_color: String,
    pub background_transparency_percent: u32,
    pub text_color: String,
    pub accent_color: String,
    pub image: Option<Image>,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            title: "Time for a break".into(),
            message: "Step away from your screen and rest your eyes.".into(),
            background_color: "#101827".into(),
            background_transparency_percent: DEFAULT_BACKGROUND_TRANSPARENCY_PERCENT,
            text_color: "#F8FAFC".into(),
            accent_color: "#69D5B2".into(),
            image: None,
        }
    }
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Image {
    pub path: PathBuf,
    #[serde(default)]
    pub fit: ImageFit,
}

#[derive(Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ImageFit {
    #[default]
    Contain,
    Cover,
}

#[derive(Debug)]
pub struct ConfigError {
    path: PathBuf,
    source: Box<dyn Error>,
}

impl ConfigError {
    fn new(path: &Path, source: impl Error + 'static) -> Self {
        Self {
            path: path.to_owned(),
            source: Box::new(source),
        }
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.path.display(), self.source)
    }
}

impl Error for ConfigError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&*self.source)
    }
}

pub fn default_path(home: &Path) -> PathBuf {
    home.join(".config/break-reminder/config.yaml")
}

pub fn load(path: &Path) -> Result<Config, ConfigError> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if let Some(parent) = path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
            {
                fs::create_dir_all(parent).map_err(|error| ConfigError::new(path, error))?;
            }
            // ponytail: failed writes are removed; a simultaneous first launch may still read a partial sample.
            match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
            {
                Ok(mut file) => {
                    if let Err(error) = file.write_all(SAMPLE.as_bytes()) {
                        let _ = fs::remove_file(path);
                        return Err(ConfigError::new(path, error));
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(ConfigError::new(path, error)),
            }
            fs::File::open(path).map_err(|error| ConfigError::new(path, error))?
        }
        Err(error) => return Err(ConfigError::new(path, error)),
    };
    let mut raw: RawConfig = serde_saphyr::from_reader_with_options(
        file,
        serde_saphyr::options! {
            budget: serde_saphyr::budget! { max_reader_input_bytes: Some(64 * 1024), },
            reject_unsupported_tags: true,
            strict_booleans: true,
        },
    )
    .map_err(|error| ConfigError::new(path, error))?;
    validate(path, &mut raw)?;
    Ok(from_raw(raw))
}

fn from_raw(raw: RawConfig) -> Config {
    Config {
        interval: Duration::from_secs(u64::from(raw.interval_minutes) * 60),
        display: Duration::from_secs(u64::from(raw.duration_seconds)),
        postpone: raw
            .postpone_minutes
            .into_iter()
            .map(|minutes| Duration::from_secs(u64::from(minutes) * 60))
            .collect(),
        pause_media: raw.pause_media,
        appearance: raw.appearance,
    }
}

fn invalid(path: &Path, field: &str, reason: &str) -> ConfigError {
    ConfigError::new(
        path,
        io::Error::new(io::ErrorKind::InvalidData, format!("{field}: {reason}")),
    )
}

fn in_range(path: &Path, field: &str, value: u32, end: u32) -> Result<(), ConfigError> {
    if !(1..=end).contains(&value) {
        return Err(invalid(
            path,
            field,
            &format!("must be between 1 and {end}"),
        ));
    }
    Ok(())
}

fn validate(path: &Path, config: &mut RawConfig) -> Result<(), ConfigError> {
    in_range(
        path,
        "interval_minutes",
        config.interval_minutes,
        MAX_INTERVAL_MINUTES,
    )?;
    in_range(
        path,
        "duration_seconds",
        config.duration_seconds,
        MAX_DURATION_SECONDS,
    )?;
    if !(1..=MAX_POSTPONE_CHOICES).contains(&config.postpone_minutes.len()) {
        return Err(invalid(
            path,
            "postpone_minutes",
            &format!("must contain 1 to {MAX_POSTPONE_CHOICES} delays"),
        ));
    }
    let mut seen = HashSet::new();
    for &minutes in &config.postpone_minutes {
        in_range(path, "postpone_minutes", minutes, MAX_POSTPONE_MINUTES)?;
        if !seen.insert(minutes) {
            return Err(invalid(
                path,
                "postpone_minutes",
                "must not contain duplicates",
            ));
        }
    }
    for (field, value) in [
        ("appearance.title", config.appearance.title.as_str()),
        ("appearance.message", config.appearance.message.as_str()),
    ] {
        if value.trim().is_empty() {
            return Err(invalid(path, field, "must not be empty"));
        }
    }
    for (field, value) in [
        (
            "appearance.background_color",
            config.appearance.background_color.as_str(),
        ),
        (
            "appearance.text_color",
            config.appearance.text_color.as_str(),
        ),
        (
            "appearance.accent_color",
            config.appearance.accent_color.as_str(),
        ),
    ] {
        if !valid_color(value) {
            return Err(invalid(path, field, "must be a #RRGGBB color"));
        }
    }
    if config.appearance.background_transparency_percent > 100 {
        return Err(invalid(
            path,
            "appearance.background_transparency_percent",
            "must be between 0 and 100",
        ));
    }
    if let Some(image) = &mut config.appearance.image {
        if image.path.as_os_str().is_empty() {
            return Err(invalid(path, "appearance.image.path", "must not be empty"));
        }
        let config_dir = path
            .parent()
            .filter(|dir| !dir.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let config_dir =
            std::path::absolute(config_dir).map_err(|error| ConfigError::new(path, error))?;
        let image_path = image.path.to_string_lossy();
        let resolved = if let Some(rest) = image_path.strip_prefix("~/") {
            let home = env::var_os("HOME")
                .filter(|home| !home.is_empty())
                .ok_or_else(|| invalid(path, "appearance.image.path", "HOME is not set"))?;
            PathBuf::from(home).join(rest)
        } else if image.path.is_absolute() {
            image.path.clone()
        } else {
            config_dir.join(&image.path)
        };
        image.path = resolved;
    }
    Ok(())
}

pub fn valid_color(value: &str) -> bool {
    value.len() == 7
        && value.starts_with('#')
        && value[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
        time::{Duration, SystemTime},
    };

    use super::{Config, default_path, load};

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    fn minutes(value: u64) -> Duration {
        Duration::from_secs(value * 60)
    }

    struct Fixture {
        directory: PathBuf,
        path: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let unique = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let directory = std::env::temp_dir().join(format!(
                "break-reminder-config-{}-{unique}-{}",
                std::process::id(),
                NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&directory).unwrap();
            let path = directory.join("config.yaml");
            Self { directory, path }
        }

        fn write(&self, yaml: impl AsRef<[u8]>) {
            fs::write(&self.path, yaml).unwrap();
        }

        fn rejects(&self, yaml: &str, field: &str) {
            self.write(yaml);
            let error = load(&self.path).expect_err(yaml).to_string();
            assert!(error.contains(&self.path.display().to_string()), "{error}");
            assert!(error.contains(field), "{error}");
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.directory).unwrap();
        }
    }

    #[test]
    fn defaults_match_the_sample_configuration() {
        assert_eq!(
            Config::default(),
            load("assets/default-config.yaml".as_ref()).unwrap()
        );
    }

    #[test]
    fn creates_sample_once_and_applies_defaults_to_partial_yaml() {
        let sample = load("assets/default-config.yaml".as_ref()).unwrap();
        assert_eq!(sample.interval, minutes(60));
        assert_eq!(sample.display, Duration::from_secs(30));
        assert_eq!(sample.postpone, vec![minutes(10), minutes(15)]);
        assert!(!sample.pause_media);
        assert_eq!(sample.appearance.title, "Time for a break");

        let unique = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory =
            std::env::temp_dir().join(format!("break-reminder-{}-{unique}", std::process::id()));
        assert_eq!(
            default_path(&directory),
            directory.join(".config/break-reminder/config.yaml")
        );
        let path = default_path(&directory);
        assert_eq!(load(&path).unwrap(), sample);
        let generated = fs::read(&path).unwrap();
        assert_eq!(generated, fs::read("assets/default-config.yaml").unwrap());

        let partial = b"interval_minutes: 25\nappearance:\n  title: Stretch\n";
        fs::write(&path, partial).unwrap();
        let configured = load(&path).unwrap();
        assert_eq!(configured.interval, minutes(25));
        assert_eq!(configured.display, Duration::from_secs(30));
        assert_eq!(configured.postpone, vec![minutes(10), minutes(15)]);
        assert!(!configured.pause_media);
        assert_eq!(configured.appearance.title, "Stretch");
        assert_eq!(configured.appearance.message, sample.appearance.message);
        assert_eq!(fs::read(&path).unwrap(), partial);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn pause_media_requires_a_boolean_and_defaults_to_false() {
        let fixture = Fixture::new();
        for (yaml, expected) in [
            ("interval_minutes: 25\n", false),
            ("pause_media: false\n", false),
            ("pause_media: true\n", true),
        ] {
            fixture.write(yaml);
            assert_eq!(load(&fixture.path).unwrap().pause_media, expected);
        }
        for value in ["maybe", "yes", "1", "null", "[]"] {
            fixture.rejects(&format!("pause_media: {value}\n"), "pause_media");
        }
        fixture.rejects("pause_media: true\npause_media: false\n", "pause_media");
    }

    #[test]
    fn accepts_background_transparency_boundaries() {
        let fixture = Fixture::new();
        for percent in [0, 15, 42, 100] {
            fixture.write(format!(
                "appearance:\n  background_transparency_percent: {percent}\n"
            ));
            assert_eq!(
                load(&fixture.path)
                    .unwrap()
                    .appearance
                    .background_transparency_percent,
                percent
            );
        }
    }

    #[test]
    fn omitted_background_transparency_preserves_current_default() {
        let fixture = Fixture::new();
        assert_eq!(
            Config::default().appearance.background_transparency_percent,
            15
        );
        for yaml in ["interval_minutes: 25\n", "appearance:\n  title: Stretch\n"] {
            fixture.write(yaml);
            assert_eq!(
                load(&fixture.path)
                    .unwrap()
                    .appearance
                    .background_transparency_percent,
                15
            );
        }
    }

    #[test]
    fn rejects_invalid_background_transparency_values() {
        let fixture = Fixture::new();
        for value in ["101", "-1", "15.5", "fifteen", "true", "null", "[]"] {
            fixture.rejects(
                &format!("appearance:\n  background_transparency_percent: {value}\n"),
                "background_transparency_percent",
            );
        }
    }

    #[test]
    fn accepts_duration_and_postpone_boundaries() {
        let fixture = Fixture::new();
        for yaml in [
            "interval_minutes: 1\n",
            "interval_minutes: 1440\n",
            "duration_seconds: 1\n",
            "duration_seconds: 3600\n",
            "postpone_minutes: [1, 1440]\n",
            "postpone_minutes: [12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1]\n",
        ] {
            fixture.write(yaml);
            assert!(load(&fixture.path).is_ok(), "{yaml}");
        }
        fixture.write("postpone_minutes: [15, 10]\n");
        assert_eq!(
            load(&fixture.path).unwrap().postpone,
            [minutes(15), minutes(10)]
        );
    }

    #[test]
    fn rejects_invalid_duration_and_postpone_values() {
        let fixture = Fixture::new();
        for (yaml, field) in [
            ("interval_minutes: 0\n", "interval_minutes"),
            ("interval_minutes: 1441\n", "interval_minutes"),
            ("duration_seconds: 0\n", "duration_seconds"),
            ("duration_seconds: 3601\n", "duration_seconds"),
            ("postpone_minutes: [0]\n", "postpone_minutes"),
            ("postpone_minutes: [1441]\n", "postpone_minutes"),
            ("postpone_minutes: []\n", "postpone_minutes"),
            ("postpone_minutes: [10, 10]\n", "postpone_minutes"),
            (
                "postpone_minutes: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13]\n",
                "postpone_minutes",
            ),
            ("interval_minutes: 1.5\n", "interval_minutes"),
            ("duration_seconds: -1\n", "duration_seconds"),
        ] {
            fixture.rejects(yaml, field);
        }
    }

    #[test]
    fn rejects_blank_text_and_invalid_colors() {
        let fixture = Fixture::new();
        for (yaml, field) in [
            ("appearance:\n  title: '  '\n", "title"),
            ("appearance:\n  message: ''\n", "message"),
            (
                "appearance:\n  background_color: '#abc'\n",
                "background_color",
            ),
            ("appearance:\n  text_color: '#12GG56'\n", "text_color"),
            ("appearance:\n  accent_color: '123456'\n", "accent_color"),
        ] {
            fixture.rejects(yaml, field);
        }
        fixture.write("appearance:\n  accent_color: '#abcdef'\n");
        assert!(load(&fixture.path).is_ok());
    }

    #[test]
    fn rejects_bad_yaml_keys_and_tags_with_location() {
        let fixture = Fixture::new();
        for (yaml, field) in [
            ("appearance: [\n", "line"),
            ("unexpected: true\n", "unexpected"),
            ("appearance:\n  unexpected: true\n", "unexpected"),
            (
                "appearance:\n  image:\n    path: photo.png\n    unexpected: true\n",
                "unexpected",
            ),
            ("appearance:\n  title: First\n  title: Second\n", "title"),
            (
                "appearance:\n  image:\n    path: photo.png\n    fit: contain\n    fit: cover\n",
                "fit",
            ),
            ("interval_minutes: !env 20\n", "!env"),
        ] {
            fixture.rejects(yaml, field);
        }
    }

    #[test]
    fn enforces_exact_64_kib_yaml_limit() {
        let fixture = Fixture::new();
        let prefix = b"interval_minutes: 60\n";
        let mut yaml = prefix.to_vec();
        yaml.push(b'#');
        yaml.resize(64 * 1024 - 1, b'x');
        yaml.push(b'\n');
        fixture.write(&yaml);
        assert!(load(&fixture.path).is_ok());

        yaml.push(b'\n');
        fixture.write(&yaml);
        let error = load(&fixture.path).unwrap_err().to_string();
        assert!(
            error.contains(&fixture.path.display().to_string()),
            "{error}"
        );
    }

    #[test]
    fn resolves_relative_and_absolute_image_paths_and_defaults_fit() {
        let fixture = Fixture::new();
        let image_path = fixture.directory.join("break.png");
        fs::write(&image_path, b"image bytes are checked by the window module").unwrap();

        fixture.write("appearance:\n  image:\n    path: break.png\n");
        let image = load(&fixture.path).unwrap().appearance.image.unwrap();
        assert_eq!(image.path, image_path);
        assert_eq!(image.fit, super::ImageFit::Contain);

        fixture.write(format!(
            "appearance:\n  image:\n    path: '{}'\n    fit: cover\n",
            image_path.display()
        ));
        let image = load(&fixture.path).unwrap().appearance.image.unwrap();
        assert_eq!(image.path, image_path);
        assert_eq!(image.fit, super::ImageFit::Cover);
    }

    #[test]
    fn resolves_home_relative_image_path_when_checkout_is_under_home() {
        let fixture = Fixture::new();
        let home = PathBuf::from(std::env::var_os("HOME").unwrap());
        let existing_file = fs::canonicalize("assets/default-config.yaml").unwrap();
        let Ok(relative) = existing_file.strip_prefix(home) else {
            return; // The test may only create fixtures in this repository or a temp directory.
        };
        fixture.write(format!(
            "appearance:\n  image:\n    path: '~/{}'\n",
            relative.display()
        ));
        let image = load(&fixture.path).unwrap().appearance.image.unwrap();
        assert_eq!(image.path, existing_file);
    }

    #[test]
    fn accepts_unreadable_image_paths_for_window_fallback() {
        let fixture = Fixture::new();
        for (yaml, expected) in [
            (
                "appearance:\n  image:\n    path: missing.png\n",
                fixture.directory.join("missing.png"),
            ),
            (
                "appearance:\n  image:\n    path: .\n",
                fixture.directory.join("."),
            ),
        ] {
            fixture.write(yaml);
            let image = load(&fixture.path).unwrap().appearance.image.unwrap();
            assert_eq!(image.path, expected);
        }
    }

    #[test]
    fn rejects_missing_image_path_and_invalid_fit() {
        let fixture = Fixture::new();
        for yaml in [
            "appearance:\n  image:\n    fit: cover\n",
            "appearance:\n  image:\n    path: ''\n",
        ] {
            fixture.rejects(yaml, "path");
        }
        fixture.rejects(
            "appearance:\n  image:\n    path: image.png\n    fit: stretch\n",
            "fit",
        );
    }

    #[test]
    fn failed_reload_keeps_the_previous_configuration() {
        let fixture = Fixture::new();
        fixture.write("interval_minutes: 25\n");
        let previous = load(&fixture.path).unwrap();
        fixture.write("interval_minutes: 0\n");
        assert!(load(&fixture.path).is_err());
        assert_eq!(previous.interval, minutes(25));
    }
}
