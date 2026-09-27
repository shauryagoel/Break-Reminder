use std::{
    error::Error,
    fmt, fs,
    io::Write,
    path::{Path, PathBuf},
};

use serde::Deserialize;

const SAMPLE: &str = include_str!("../assets/default-config.yaml");

#[derive(Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub interval_minutes: u32,
    pub duration_seconds: u32,
    pub postpone_minutes: Vec<u32>,
    pub appearance: Appearance,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            interval_minutes: 60,
            duration_seconds: 30,
            postpone_minutes: vec![10, 15],
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
            match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
            {
                Ok(mut file) => file
                    .write_all(SAMPLE.as_bytes())
                    .map_err(|error| ConfigError::new(path, error))?,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(ConfigError::new(path, error)),
            }
            fs::File::open(path).map_err(|error| ConfigError::new(path, error))?
        }
        Err(error) => return Err(ConfigError::new(path, error)),
    };
    serde_saphyr::from_reader_with_options(
        file,
        serde_saphyr::options! {
            budget: serde_saphyr::budget! { max_reader_input_bytes: Some(64 * 1024), },
            reject_unsupported_tags: true,
        },
    )
    .map_err(|error| ConfigError::new(path, error))
}

#[cfg(test)]
mod tests {
    use std::{fs, time::SystemTime};

    use super::{default_path, load};

    #[test]
    fn creates_sample_once_and_applies_defaults_to_partial_yaml() {
        let sample = load("assets/default-config.yaml".as_ref()).unwrap();
        assert_eq!(sample.interval_minutes, 60);
        assert_eq!(sample.duration_seconds, 30);
        assert_eq!(sample.postpone_minutes, vec![10, 15]);
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
        assert_eq!(configured.interval_minutes, 25);
        assert_eq!(configured.duration_seconds, 30);
        assert_eq!(configured.postpone_minutes, vec![10, 15]);
        assert_eq!(configured.appearance.title, "Stretch");
        assert_eq!(configured.appearance.message, sample.appearance.message);
        assert_eq!(fs::read(&path).unwrap(), partial);
        fs::remove_dir_all(directory).unwrap();
    }
}
