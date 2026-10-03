use std::{
    collections::HashSet,
    io::{self, Read, Write},
    path::PathBuf,
};

use serde::{Deserialize, Serialize};

use crate::config::{
    Config, DEFAULT_BACKGROUND_TRANSPARENCY_PERCENT, ImageFit, MAX_DURATION_SECONDS,
    MAX_POSTPONE_CHOICES, MAX_POSTPONE_MINUTES, valid_color,
};

const MAX_SNAPSHOT_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub duration_seconds: u32,
    pub postpone_minutes: Vec<u32>,
    pub title: String,
    pub message: String,
    pub background_color: String,
    #[serde(default = "default_background_transparency_percent")]
    pub background_transparency_percent: u32,
    pub text_color: String,
    pub accent_color: String,
    pub image: Option<SnapshotImage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotImage {
    pub path: PathBuf,
    pub fit: Fit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Fit {
    Contain,
    Cover,
}

fn default_background_transparency_percent() -> u32 {
    DEFAULT_BACKGROUND_TRANSPARENCY_PERCENT
}

impl Snapshot {
    pub fn from_config(config: &Config) -> Self {
        let appearance = &config.appearance;
        Self {
            duration_seconds: config.display.as_secs() as u32,
            postpone_minutes: config
                .postpone
                .iter()
                .map(|delay| (delay.as_secs() / 60) as u32)
                .collect(),
            title: appearance.title.clone(),
            message: appearance.message.clone(),
            background_color: appearance.background_color.clone(),
            background_transparency_percent: appearance.background_transparency_percent,
            text_color: appearance.text_color.clone(),
            accent_color: appearance.accent_color.clone(),
            image: appearance.image.as_ref().map(|image| SnapshotImage {
                path: image.path.clone(),
                fit: match image.fit {
                    ImageFit::Contain => Fit::Contain,
                    ImageFit::Cover => Fit::Cover,
                },
            }),
        }
    }

    fn validate(&self) -> io::Result<()> {
        if !(1..=MAX_DURATION_SECONDS).contains(&self.duration_seconds)
            || !(1..=MAX_POSTPONE_CHOICES).contains(&self.postpone_minutes.len())
            || self
                .postpone_minutes
                .iter()
                .any(|minutes| !(1..=MAX_POSTPONE_MINUTES).contains(minutes))
            || self
                .postpone_minutes
                .iter()
                .copied()
                .collect::<HashSet<_>>()
                .len()
                != self.postpone_minutes.len()
            || self.title.trim().is_empty()
            || self.message.trim().is_empty()
            || !valid_color(&self.background_color)
            || self.background_transparency_percent > 100
            || !valid_color(&self.text_color)
            || !valid_color(&self.accent_color)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid overlay settings snapshot",
            ));
        }
        Ok(())
    }
}

pub fn write_snapshot(writer: &mut impl Write, snapshot: &Snapshot) -> io::Result<()> {
    snapshot.validate()?;
    let yaml = serde_saphyr::to_string(snapshot).map_err(io::Error::other)?;
    let length = u32::try_from(yaml.len()).map_err(io::Error::other)?;
    if yaml.len() > MAX_SNAPSHOT_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "overlay settings snapshot exceeds 256 KiB",
        ));
    }
    writer.write_all(&length.to_be_bytes())?;
    writer.write_all(yaml.as_bytes())?;
    writer.flush()
}

pub fn read_snapshot(reader: &mut impl Read) -> io::Result<Snapshot> {
    let mut length = [0; 4];
    reader.read_exact(&mut length)?;
    let length = u32::from_be_bytes(length) as usize;
    if !(1..=MAX_SNAPSHOT_BYTES).contains(&length) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "overlay settings snapshot has invalid length",
        ));
    }
    let mut yaml = vec![0; length];
    reader.read_exact(&mut yaml)?;
    let snapshot: Snapshot = serde_saphyr::from_slice_with_options(
        &yaml,
        serde_saphyr::options! {
            budget: serde_saphyr::budget! { max_reader_input_bytes: Some(MAX_SNAPSHOT_BYTES), },
            reject_unsupported_tags: true,
        },
    )
    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    snapshot.validate()?;
    Ok(snapshot)
}

pub fn read_start(reader: &mut impl Read) -> io::Result<()> {
    let mut line = [0; 6];
    reader.read_exact(&mut line)?;
    if &line == b"START\n" {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "expected START line",
        ))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Elapsed,
    Skip,
    Postpone(usize),
}

impl Action {
    pub fn parse_line(line: &str, postpone_count: usize) -> Result<Self, String> {
        match line.strip_suffix('\n') {
            Some("ELAPSED") => Ok(Self::Elapsed),
            Some("SKIP") => Ok(Self::Skip),
            Some(value) => match value.strip_prefix("POSTPONE ") {
                Some(index) => {
                    let index = index
                        .parse::<usize>()
                        .map_err(|_| "invalid POSTPONE index".to_owned())?;
                    if index >= postpone_count {
                        return Err("POSTPONE index is out of range".into());
                    }
                    Ok(Self::Postpone(index))
                }
                None => Err("unknown overlay action".into()),
            },
            None => Err("overlay action is missing newline".into()),
        }
    }
}

pub struct Output<W: Write> {
    writer: W,
    ready: bool,
    terminal: bool,
}

impl<W: Write> Output<W> {
    pub fn new(writer: W) -> Self {
        Self {
            writer,
            ready: false,
            terminal: false,
        }
    }

    pub fn ready(&mut self) -> io::Result<()> {
        if !self.ready {
            self.ready = true;
            self.writer.write_all(b"READY\n")?;
            self.writer.flush()?;
        }
        Ok(())
    }

    pub fn terminal(&mut self, action: Action) -> io::Result<bool> {
        if !self.ready {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "overlay is not ready",
            ));
        }
        if self.terminal {
            return Ok(false);
        }
        self.terminal = true;
        match action {
            Action::Elapsed => self.writer.write_all(b"ELAPSED\n")?,
            Action::Skip => self.writer.write_all(b"SKIP\n")?,
            Action::Postpone(index) => writeln!(self.writer, "POSTPONE {index}")?,
        }
        self.writer.flush()?;
        Ok(true)
    }

    #[cfg(test)]
    fn into_inner(self) -> W {
        self.writer
    }
}

#[cfg(test)]
mod tests {
    use std::{io::Cursor, path::PathBuf};

    use super::{
        Action, Fit, Output, Snapshot, SnapshotImage, read_snapshot, read_start, write_snapshot,
    };

    fn sample() -> Snapshot {
        Snapshot::from_config(&crate::config::load("assets/default-config.yaml".as_ref()).unwrap())
    }

    #[test]
    fn framed_snapshot_keeps_yaml_newlines_separate_from_start() {
        let mut snapshot = sample();
        snapshot.message = "Rest your eyes.\nLook outside.".into();
        snapshot.image = Some(SnapshotImage {
            path: PathBuf::from("/tmp/rest.png"),
            fit: Fit::Cover,
        });
        let mut bytes = Vec::new();
        write_snapshot(&mut bytes, &snapshot).unwrap();
        bytes.extend_from_slice(b"START\n");
        let mut input = Cursor::new(bytes);
        assert_eq!(read_snapshot(&mut input).unwrap(), snapshot);
        read_start(&mut input).unwrap();
    }

    #[test]
    fn framed_snapshot_accepts_configured_background_transparency() {
        for percent in [0, 15, 42, 100] {
            let mut config = crate::config::Config::default();
            config.appearance.background_transparency_percent = percent;
            let snapshot = Snapshot::from_config(&config);
            assert_eq!(snapshot.background_transparency_percent, percent);
            let mut bytes = Vec::new();
            write_snapshot(&mut bytes, &snapshot).unwrap();
            assert_eq!(read_snapshot(&mut Cursor::new(bytes)).unwrap(), snapshot);
        }
    }

    #[test]
    fn omitted_snapshot_background_transparency_preserves_current_default() {
        let snapshot = sample();
        let yaml = serde_saphyr::to_string(&snapshot)
            .unwrap()
            .lines()
            .filter(|line| !line.starts_with("background_transparency_percent:"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut bytes = Vec::from((yaml.len() as u32).to_be_bytes());
        bytes.extend_from_slice(yaml.as_bytes());
        let decoded = read_snapshot(&mut Cursor::new(bytes)).unwrap();
        assert_eq!(decoded.background_transparency_percent, 15);
        assert_eq!(decoded, snapshot);
    }

    #[test]
    fn rejects_invalid_snapshot_background_transparency() {
        for percent in [101, u32::MAX] {
            let mut snapshot = sample();
            snapshot.background_transparency_percent = percent;
            let mut bytes = Vec::new();
            assert!(write_snapshot(&mut bytes, &snapshot).is_err());
            assert!(bytes.is_empty());

            let yaml = serde_saphyr::to_string(&snapshot).unwrap();
            let mut bytes = Vec::from((yaml.len() as u32).to_be_bytes());
            bytes.extend_from_slice(yaml.as_bytes());
            assert!(read_snapshot(&mut Cursor::new(bytes)).is_err());
        }
    }

    #[test]
    fn rejects_oversized_truncated_and_malformed_frames() {
        let oversized = (256_u32 * 1024 + 1).to_be_bytes();
        assert!(read_snapshot(&mut Cursor::new(oversized)).is_err());

        let truncated = [0, 0, 0, 5, b'a', b'b'];
        assert!(read_snapshot(&mut Cursor::new(truncated)).is_err());

        let mut malformed = Vec::from(6_u32.to_be_bytes());
        malformed.extend_from_slice(b"bad: [");
        assert!(read_snapshot(&mut Cursor::new(malformed)).is_err());

        let mut invalid = sample();
        invalid.duration_seconds = 0;
        assert!(write_snapshot(&mut Vec::new(), &invalid).is_err());

        assert!(read_start(&mut Cursor::new(b"WRONG\n")).is_err());
    }

    #[test]
    fn rejects_unknown_actions_and_invalid_postpone_indices() {
        assert_eq!(Action::parse_line("SKIP\n", 2).unwrap(), Action::Skip);
        assert_eq!(Action::parse_line("ELAPSED\n", 2).unwrap(), Action::Elapsed);
        assert_eq!(
            Action::parse_line("POSTPONE 1\n", 2).unwrap(),
            Action::Postpone(1)
        );
        for line in [
            "UNKNOWN\n",
            "POSTPONE\n",
            "POSTPONE 2\n",
            "POSTPONE -1\n",
            "SKIP extra\n",
        ] {
            assert!(Action::parse_line(line, 2).is_err(), "{line}");
        }
    }

    #[test]
    fn writer_flushes_ready_and_only_first_terminal_action() {
        let mut output = Output::new(Vec::new());
        output.ready().unwrap();
        assert!(output.terminal(Action::Postpone(0)).unwrap());
        assert!(!output.terminal(Action::Elapsed).unwrap());
        assert!(!output.terminal(Action::Skip).unwrap());
        assert_eq!(output.into_inner(), b"READY\nPOSTPONE 0\n");
    }
}
