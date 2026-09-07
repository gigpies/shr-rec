use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs::{File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::OpenOptionsExt,
    path::Path,
};

pub const MAX_CHANNELS: usize = 64;
const MAX_CONFIG_BYTES: u64 = 128 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub format_version: u32,
    pub tracks: Vec<Track>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Track {
    pub id: String,
    pub label: String,
    pub armed: bool,
    pub preferred_source: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resolution {
    Unassigned,
    Missing,
    Ambiguous,
    Resolved,
}

impl Track {
    pub fn resolve(&self, sources: &[String]) -> Resolution {
        let Some(wanted) = self.preferred_source.as_deref() else {
            return Resolution::Unassigned;
        };
        match sources
            .iter()
            .filter(|source| source.as_str() == wanted)
            .count()
        {
            0 => Resolution::Missing,
            1 => Resolution::Resolved,
            _ => Resolution::Ambiguous,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            format_version: 1,
            tracks: (1..=18)
                .map(|i| Track {
                    id: format!("input-{i}"),
                    label: format!("Input {i}"),
                    armed: false,
                    preferred_source: None,
                })
                .collect(),
        }
    }
}

impl Config {
    pub fn validate(&self) -> Result<()> {
        if self.format_version != 1 {
            bail!("unsupported config version {}", self.format_version);
        }
        if self.tracks.is_empty() || self.tracks.len() > MAX_CHANNELS {
            bail!("configure 1–64 tracks");
        }
        let mut ids = HashSet::new();
        for track in &self.tracks {
            if track.id.is_empty()
                || track.id.len() > 64
                || !track
                    .id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
                || !ids.insert(&track.id)
            {
                bail!("invalid or duplicate track ID");
            }
            if track.label.trim().is_empty()
                || track.label.len() > 128
                || track.label.chars().any(char::is_control)
            {
                bail!("track label must be nonempty printable text (up to 128 bytes)");
            }
            if let Some(source) = &track.preferred_source
                && (source.trim().is_empty()
                    || source.len() > 256
                    || source.chars().any(char::is_control))
            {
                bail!("source must be an exact nonempty port name or null");
            }
        }
        Ok(())
    }

    /// Missing config uses inert defaults. Opening never creates or rewrites it.
    pub fn load(root: &Path) -> Result<Self> {
        let path = root.join("config.json");
        let file = match OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(&path)
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => return Err(error).with_context(|| format!("open {}", path.display())),
        };
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.len() > MAX_CONFIG_BYTES {
            bail!("config must be a regular file up to 128 KiB");
        }
        let mut bytes = Vec::new();
        file.take(MAX_CONFIG_BYTES + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_CONFIG_BYTES {
            bail!("config exceeds 128 KiB");
        }
        let config: Self = serde_json::from_slice(&bytes).context("parse config.json")?;
        config.validate()?;
        Ok(config)
    }

    /// Explicit, create-only setup. Never replaces an existing musician config.
    pub fn create_default(root: &Path) -> Result<()> {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(root)?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(root.join("config.json"))?;
        file.write_all(&serde_json::to_vec_pretty(&Self::default())?)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        File::open(root)?.sync_all()?;
        Ok(())
    }

    pub fn capture_ready(&self, sources: &[String]) -> Result<()> {
        self.validate()?;
        let armed: Vec<_> = self.tracks.iter().filter(|track| track.armed).collect();
        if armed.is_empty() {
            bail!("Arm at least one input");
        }
        if let Some(track) = armed
            .iter()
            .find(|track| track.resolve(sources) != Resolution::Resolved)
        {
            bail!("Unresolved input: {}", track.label);
        }
        Ok(())
    }
}
