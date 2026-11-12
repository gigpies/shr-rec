//! Read-only, descriptor-relative take validation. Files remain open and the
//! directory stays share-locked until the reader is released.
use anyhow::{Context, Result, ensure};
use serde_json::Value;
use std::{
    collections::HashSet,
    ffi::CString,
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::OpenOptionsExt,
    },
    path::{Path, PathBuf},
};

pub const MAX_TAKES: usize = 1024;
#[derive(Clone, Debug)]
pub struct Entry {
    pub path: PathBuf,
    pub label: String,
}

/// One directory level only; no traversal through symlinks and bounded results.
pub fn discover(root: &Path) -> Result<Vec<Entry>> {
    let directory = match std::fs::read_dir(root) {
        Ok(d) => d,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e).context("read take library"),
    };
    let mut entries = Vec::new();
    for (scanned, child) in directory.enumerate() {
        ensure!(
            scanned < 4096,
            "take folder exceeds 4096 entries; use a smaller folder"
        );
        let child = child?;
        ensure!(
            entries.len() < MAX_TAKES,
            "take list exceeds {MAX_TAKES}; use a smaller folder"
        );
        if child.file_type()?.is_dir()
            && child.path().join("session.json").symlink_metadata().is_ok()
        {
            entries.push(Entry {
                path: child.path(),
                label: child
                    .file_name()
                    .to_string_lossy()
                    .chars()
                    .map(|c| if c.is_control() { '�' } else { c })
                    .collect(),
            });
        }
    }
    entries.sort_by(|a, b| a.label.cmp(&b.label));
    Ok(entries)
}

pub struct Stem {
    pub id: String,
    pub label: String,
    pub file: File,
    pub data_offset: u64,
    pub frames: u64,
}
pub struct Take {
    pub path: PathBuf,
    pub label: String,
    pub sample_rate: u32,
    pub frames: u64,
    pub warning: Option<String>,
    pub stems: Vec<Stem>,
    _directory: File,
}

fn member(directory: &File, name: &str) -> Result<File> {
    ensure!(
        !name.is_empty()
            && name != "."
            && name != ".."
            && !name.contains('/')
            && !name.contains('\\'),
        "unsafe take member: {name}"
    );
    let name_c = CString::new(name)?;
    // SAFETY: owned directory and valid name; returned descriptor is owned once.
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name_c.as_ptr(),
            libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error()).with_context(|| format!("open {name}"));
    }
    let file = unsafe { File::from_raw_fd(fd) };
    ensure!(file.metadata()?.is_file(), "{name} must be a regular file");
    Ok(file)
}
fn optional_member(directory: &File, name: &str) -> Result<Option<File>> {
    match member(directory, name) {
        Ok(file) => Ok(Some(file)),
        Err(error)
            if error
                .root_cause()
                .downcast_ref::<std::io::Error>()
                .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
        {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}
fn json(directory: &File, name: &str) -> Result<Value> {
    let file = member(directory, name)?;
    ensure!(
        file.metadata()?.len() <= 256 * 1024,
        "{name} exceeds 256 KiB"
    );
    let mut data = Vec::new();
    file.take(256 * 1024 + 1).read_to_end(&mut data)?;
    ensure!(data.len() <= 256 * 1024, "{name} grew beyond limit");
    serde_json::from_slice::<Unique>(&data)
        .map(|v| v.0)
        .with_context(|| format!("parse {name}"))
}
// serde_json::Value normally keeps the last duplicate field. A take manifest
// must not admit two contradictory identities/versions at any nesting level.
struct Unique(Value);
impl<'de> serde::Deserialize<'de> for Unique {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = Unique;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("JSON without duplicate keys")
            }
            fn visit_map<M: serde::de::MapAccess<'de>>(
                self,
                mut m: M,
            ) -> std::result::Result<Unique, M::Error> {
                let mut value = serde_json::Map::new();
                while let Some(key) = m.next_key::<String>()? {
                    if value.contains_key(&key) {
                        return Err(serde::de::Error::custom(format!(
                            "duplicate JSON key: {key}"
                        )));
                    }
                    value.insert(key, m.next_value::<Unique>()?.0);
                }
                Ok(Unique(Value::Object(value)))
            }
            fn visit_seq<S: serde::de::SeqAccess<'de>>(
                self,
                mut s: S,
            ) -> std::result::Result<Unique, S::Error> {
                let mut value = Vec::new();
                while let Some(v) = s.next_element::<Unique>()? {
                    value.push(v.0);
                }
                Ok(Unique(Value::Array(value)))
            }
            fn visit_bool<E: serde::de::Error>(self, v: bool) -> std::result::Result<Unique, E> {
                Ok(Unique(v.into()))
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> std::result::Result<Unique, E> {
                Ok(Unique(v.into()))
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> std::result::Result<Unique, E> {
                Ok(Unique(v.into()))
            }
            fn visit_f64<E: serde::de::Error>(self, v: f64) -> std::result::Result<Unique, E> {
                serde_json::Number::from_f64(v)
                    .map(|v| Unique(Value::Number(v)))
                    .ok_or_else(|| E::custom("nonfinite JSON number"))
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> std::result::Result<Unique, E> {
                Ok(Unique(v.into()))
            }
            fn visit_string<E: serde::de::Error>(
                self,
                v: String,
            ) -> std::result::Result<Unique, E> {
                Ok(Unique(v.into()))
            }
            fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<Unique, E> {
                Ok(Unique(Value::Null))
            }
        }
        d.deserialize_any(Visitor)
    }
}

fn integer(v: &Value, key: &str) -> Result<u64> {
    v[key]
        .as_u64()
        .with_context(|| format!("missing/invalid {key}"))
}
fn text(v: &Value, key: &str) -> Result<String> {
    let s = v[key].as_str().with_context(|| format!("missing {key}"))?;
    ensure!(
        !s.is_empty() && s.len() <= 256 && !s.chars().any(char::is_control),
        "invalid {key}"
    );
    Ok(s.into())
}

/// Only aligned, finalized PCM24 mono files can be activated. An incomplete
/// manifest with valid finalized stems can be deliberately auditioned; broken
/// headers/partial stems require recovery first, never implicit truncation.
impl Take {
    pub fn open(path: &Path) -> Result<Self> {
        let directory = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path)
            .context("open take directory")?;
        ensure!(
            unsafe { libc::flock(directory.as_raw_fd(), libc::LOCK_SH | libc::LOCK_NB) } == 0,
            "take is owned by a writer/recovery"
        );
        let manifest = json(&directory, "session.json")?;
        ensure!(
            manifest["format"].is_null() || manifest["format"] == "shr-rec-raw",
            "unknown take format"
        );
        if let (Some(a), Some(b)) = (manifest.get("format_version"), manifest.get("version")) {
            ensure!(a == b, "ambiguous format version");
        }
        let mut specs = Vec::new();
        let (rate, frames, warning, label) = if manifest["format"] == "shr-rec-raw" {
            ensure!(
                integer(&manifest, "version")? == 1,
                "unsupported native take version"
            );
            ensure!(
                manifest["encoding"] == "PCM_S24LE",
                "unsupported native encoding"
            );
            let config = &manifest["config"];
            let count = integer(config, "channels")?;
            ensure!((1..=64).contains(&count), "take requires 1–64 channels");
            let result = json(&directory, "result.json")
                .context("take has no finalized result; recover before opening")?;
            let report: crate::recorder::Report =
                serde_json::from_value(result.clone()).context("invalid native result metadata")?;
            let raw_config: crate::recorder::Config =
                serde_json::from_value(config.clone()).context("invalid native recorder config")?;
            ensure!(
                report.epoch == raw_config.epoch,
                "result belongs to a different capture epoch"
            );
            ensure!(
                (1..=8192).contains(&raw_config.max_block)
                    && (2..=4096).contains(&raw_config.queue_blocks),
                "invalid native buffer metadata"
            );
            let status = text(&result, "status")?;
            ensure!(
                [
                    "complete",
                    "incomplete",
                    "recovered",
                    "recovered-incomplete"
                ]
                .contains(&status.as_str()),
                "unknown take status"
            );
            let failure = if optional_member(&directory, "failure.json")?.is_some() {
                Some(json(&directory, "failure.json").context("invalid failure marker")?)
            } else {
                None
            };
            let dirty = failure.is_some()
                || [
                    "dropped_frames",
                    "overflow_blocks",
                    "source_gap_frames",
                    "invalid_blocks",
                    "external_fault",
                    "clipped_samples",
                ]
                .iter()
                .any(|key| result[*key].as_u64() != Some(0))
                || !result["error"].is_null();
            for i in 1..=count {
                specs.push((
                    format!("channel-{i:03}"),
                    format!("Channel {i}"),
                    format!("channel-{i:03}.wav"),
                    integer(&result, "frames")?,
                    true,
                ));
            }
            (
                integer(config, "sample_rate")?,
                integer(&result, "frames")?,
                (status != "complete" || dirty)
                    .then(|| format!("{status}: dropped={} overflows={} gaps={} invalid={} host={} clipped={}; {}", report.dropped_frames,report.overflow_blocks,report.source_gap_frames,report.invalid_blocks,report.external_fault,report.clipped_samples,report.error.as_deref().unwrap_or(if failure.is_some(){"failure marker present; review retained report"}else{"review integrity"}))),
                path.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .chars()
                    .map(|c| if c.is_control() { '�' } else { c })
                    .collect(),
            )
        } else {
            let version = manifest
                .get("format_version")
                .or_else(|| manifest.get("version"))
                .and_then(Value::as_u64)
                .context("missing format_version")?;
            ensure!(version == 1, "unsupported SHR take version {version}");
            let tracks = manifest["tracks"].as_array().context("missing tracks")?;
            ensure!(
                (1..=64).contains(&tracks.len()),
                "take requires 1–64 tracks"
            );
            let mut stereo_groups = std::collections::HashMap::<String, (u32, u32)>::new();
            for track in tracks {
                let role = text(track, "role")?;
                ensure!(
                    ["mono", "stereo-left", "stereo-right"].contains(&role.as_str()),
                    "unknown track role"
                );
                if !track["preferred_source"].is_null() {
                    let _ = text(track, "preferred_source")?;
                }
                let _ = text(track, "actual_jack_port")?;
                if role != "mono" {
                    let group = text(track, "group").context("stereo track requires a group")?;
                    let pair = stereo_groups.entry(group).or_default();
                    if role == "stereo-left" {
                        pair.0 += 1;
                    } else {
                        pair.1 += 1;
                    }
                } else if !track["group"].is_null() {
                    let _ = text(track, "group")?;
                }
                specs.push((
                    text(track, "id")?,
                    text(track, "label")?,
                    text(track, "wav_file")?,
                    integer(track, "frames")?,
                    track["finalized"].as_bool().context("missing finalized")?,
                ));
            }
            ensure!(
                stereo_groups.values().all(|p| *p == (1, 1)),
                "stereo groups require exactly one left and right stem"
            );
            let duration = manifest["duration_seconds"]
                .as_f64()
                .context("invalid duration_seconds")?;
            let rate = integer(&manifest, "sample_rate")?;
            let total = integer(&manifest, "total_frames")?;
            ensure!(
                rate > 0
                    && duration.is_finite()
                    && (duration - total as f64 / rate as f64).abs() <= 1. / rate as f64,
                "duration does not match timeline"
            );
            let completeness = text(&manifest, "completeness")?;
            ensure!(
                ["complete", "incomplete", "recovered", "recording"]
                    .contains(&completeness.as_str()),
                "unknown completeness"
            );
            let dirty = [
                "dropped_frames",
                "overflow_events",
                "callback_violations",
                "xruns",
            ]
            .iter()
            .any(|key| manifest[*key].as_u64() != Some(0));
            (
                integer(&manifest, "sample_rate")?,
                integer(&manifest, "total_frames")?,
                (completeness != "complete"
                    || manifest["finalization"] != "finalized"
                    || dirty
                    || manifest["recovery"]
                        .as_array()
                        .is_none_or(|a| !a.is_empty()))
                .then(|| format!("{completeness}: dropped={} overflows={} callback={} xruns={}; finalization={}",manifest["dropped_frames"],manifest["overflow_events"],manifest["callback_violations"],manifest["xruns"],manifest["finalization"])),
                text(&manifest, "session_name")?,
            )
        };
        ensure!(
            (8_000..=384_000).contains(&rate),
            "invalid take sample rate"
        );
        ensure!(
            frames > 0 && frames <= crate::recorder::MAX_WAV_FRAMES,
            "empty or oversized take"
        );
        let mut ids = HashSet::new();
        let mut files = HashSet::new();
        let mut inodes = HashSet::new();
        let mut stems = Vec::new();
        for (id, label, name, track_frames, finalized) in specs {
            ensure!(
                ids.insert(id.clone()) && files.insert(name.clone()),
                "duplicate track identity/file"
            );
            ensure!(
                finalized && track_frames == frames,
                "unfinalized or unequal track metadata: {label}; recover first"
            );
            ensure!(
                optional_member(&directory, &format!("{name}.part"))?.is_none(),
                "ambiguous final/partial stem: {name}; recover first"
            );
            let mut file = member(&directory, &name)?;
            use std::os::unix::fs::MetadataExt;
            let stat = file.metadata()?;
            ensure!(
                inodes.insert((stat.dev(), stat.ino())),
                "duplicate stem inode"
            );
            let (wav_rate, wav_frames, offset) = wav(&mut file, manifest["format"].is_null())
                .with_context(|| format!("validate {name}"))?;
            ensure!(u64::from(wav_rate) == rate, "sample rate differs in {name}");
            ensure!(
                wav_frames == frames,
                "frame count differs in {name}: {wav_frames} vs {frames}"
            );
            stems.push(Stem {
                id,
                label,
                file,
                data_offset: offset,
                frames,
            });
        }
        Ok(Self {
            path: path.canonicalize().context("resolve take location")?,
            label,
            sample_rate: rate as u32,
            frames,
            warning,
            stems,
            _directory: directory,
        })
    }
    pub fn authorize(&self, sample_rate: u32, allow_damaged: bool) -> Result<()> {
        ensure!(
            self.sample_rate == sample_rate,
            "take {} Hz / JACK {sample_rate} Hz; resampling unsupported",
            self.sample_rate
        );
        ensure!(
            allow_damaged || self.warning.is_none(),
            "damaged/incomplete take: deliberate confirmation required"
        );
        Ok(())
    }
}

fn wav(file: &mut File, legacy_odd_tail: bool) -> Result<(u32, u64, u64)> {
    let len = file.metadata()?.len();
    let mut header = [0u8; 12];
    file.read_exact(&mut header)?;
    ensure!(
        &header[..4] == b"RIFF" && &header[8..] == b"WAVE",
        "not RIFF/WAVE"
    );
    let end = u64::from(u32::from_le_bytes(header[4..8].try_into()?)) + 8;
    ensure!(end == len, "RIFF length does not match file");
    let mut rate = None;
    let mut data = None;
    let mut at = 12u64;
    for _ in 0..128 {
        if at == end {
            break;
        }
        ensure!(at + 8 <= end, "truncated WAV chunk");
        file.seek(SeekFrom::Start(at))?;
        let mut h = [0u8; 8];
        file.read_exact(&mut h)?;
        let size = u64::from(u32::from_le_bytes(h[4..].try_into()?));
        let unpadded = at.checked_add(8 + size).context("WAV chunk overflow")?;
        // SHR format-1 omitted RIFF's final odd-byte data pad. Accept that exact
        // legacy form read-only, without relaxing native or interior chunks.
        let next = if legacy_odd_tail && &h[..4] == b"data" && unpadded == end {
            unpadded
        } else {
            unpadded + size % 2
        };
        ensure!(next <= end, "truncated WAV data");
        match &h[..4] {
            b"fmt " => {
                ensure!(
                    rate.is_none() && size == 16,
                    "unsupported/duplicate WAV fmt"
                );
                let mut fmt = [0u8; 16];
                file.read_exact(&mut fmt)?;
                let r = u32::from_le_bytes(fmt[4..8].try_into()?);
                ensure!(
                    u16::from_le_bytes(fmt[..2].try_into()?) == 1
                        && u16::from_le_bytes(fmt[2..4].try_into()?) == 1
                        && u16::from_le_bytes(fmt[14..16].try_into()?) == 24
                        && u16::from_le_bytes(fmt[12..14].try_into()?) == 3
                        && u32::from_le_bytes(fmt[8..12].try_into()?)
                            == r.checked_mul(3).context("rate overflow")?,
                    "requires mono PCM24 WAV"
                );
                rate = Some(r);
            }
            b"data" => {
                ensure!(
                    data.is_none() && size % 3 == 0,
                    "duplicate or unaligned WAV data"
                );
                data = Some((size / 3, at + 8));
            }
            _ => {}
        }
        at = next;
    }
    ensure!(at == end, "too many WAV chunks");
    let (frames, offset) = data.context("missing WAV data")?;
    Ok((rate.context("missing WAV fmt")?, frames, offset))
}
