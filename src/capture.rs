//! Owner-thread capture lifecycle. Only the callback owns the producer while
//! attached; preparing/finalizing workers communicate by consuming ownership.
use crate::{
    config::{Config, Resolution, Track},
    jack_playback::capture::{Device, Inputs, JackCapture, SharedCapture, Source},
    recorder::{self, Observer, Recorder},
    session::{Effect, RetainedTake, Session, Transport},
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::Arc,
    thread::{self, JoinHandle},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Identity {
    pub version: u32,
    pub tracks: Vec<Track>,
    pub source_uuids: Vec<u64>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Preferences {
    version: u32,
    destination: PathBuf,
    config: Config,
}

enum Work {
    Preparing(JoinHandle<Result<Recorder>>),
    Finishing(JoinHandle<Result<recorder::Report>>),
    Failed(String),
}
pub struct Capture {
    pub inputs: Option<Inputs>,
    pub destination: PathBuf,
    pub path: Option<PathBuf>,
    pub status: String,
    pub observer: Option<Observer>,
    pub operation: Option<u64>,
    pub recovering: bool,
    device: Box<dyn Device>,
    shared: Option<Arc<SharedCapture>>,
    work: Option<Work>,
    selected_sources: Vec<Source>,
    rate: u32,
    startup_error: Option<String>,
}
impl Capture {
    pub fn new(destination: PathBuf) -> Self {
        Self::with_device(destination, Box::<JackCapture>::default())
    }
    pub fn with_device(destination: PathBuf, device: Box<dyn Device>) -> Self {
        Self {
            inputs: None,
            destination,
            path: None,
            status: "REFRESH inputs before RECORD".into(),
            observer: None,
            operation: None,
            recovering: false,
            device,
            shared: None,
            work: None,
            selected_sources: vec![],
            rate: 0,
            startup_error: None,
        }
    }
    pub fn busy(&self) -> bool {
        self.work.is_some() || self.shared.is_some() || self.operation.is_some()
    }
    pub fn sources(&self) -> Vec<String> {
        self.inputs.as_ref().map_or_else(Vec::new, |i| {
            i.sources.iter().map(|s| s.name.clone()).collect()
        })
    }
    pub fn refresh(&mut self) -> Result<()> {
        ensure!(!self.busy(), "STOP before refreshing");
        // Retain preferences, but never leave stale discovery eligible to start.
        self.inputs = None;
        let inputs = self.device.inputs()?;
        ensure!(
            (8000..=384000).contains(&inputs.rate),
            "unsupported JACK rate"
        );
        self.status = format!(
            "{} sources; {} Hz; inspect routes",
            inputs.sources.len(),
            inputs.rate
        );
        self.inputs = Some(inputs);
        Ok(())
    }
    pub fn save(&self, root: &Path, config: &Config) -> Result<()> {
        ensure!(!self.busy(), "STOP before SAVE");
        ensure!(
            self.destination.is_absolute(),
            "destination must be absolute"
        );
        config.validate()?;
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(root)?;
        let tmp = root.join(".capture.json.part");
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)
            .context("stale .capture.json.part requires inspection")?;
        let bytes = serde_json::to_vec_pretty(&Preferences {
            version: 1,
            destination: self.destination.clone(),
            config: config.clone(),
        })?;
        f.write_all(&bytes)?;
        f.sync_all()?;
        fs::rename(tmp, root.join("capture.json"))?;
        File::open(root)?.sync_all()?;
        Ok(())
    }
    pub fn load(&mut self, root: &Path, config: &mut Config) -> Result<()> {
        let f = match OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(root.join("capture.json"))
        {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e.into()),
        };
        ensure!(
            f.metadata()?.is_file() && f.metadata()?.len() <= 128 * 1024,
            "invalid capture preferences"
        );
        let mut bytes = Vec::new();
        f.take(128 * 1024 + 1).read_to_end(&mut bytes)?;
        ensure!(bytes.len() <= 128 * 1024, "capture settings too large");
        let saved: Preferences = serde_json::from_slice(&bytes)?;
        ensure!(
            saved.version == 1 && saved.destination.is_absolute(),
            "invalid capture settings version/destination"
        );
        saved.config.validate()?;
        self.destination = saved.destination;
        *config = saved.config;
        Ok(())
    }
    pub fn start(&mut self, config: &Config, session: &mut Session) -> Result<()> {
        ensure!(!self.busy(), "capture busy");
        let inputs = self
            .inputs
            .as_ref()
            .context("REFRESH and inspect inputs first")?;
        config.capture_ready(&self.sources())?;
        self.selected_sources = config
            .tracks
            .iter()
            .filter(|t| t.armed)
            .map(|t| {
                inputs
                    .sources
                    .iter()
                    .find(|s| Some(&s.name) == t.preferred_source.as_ref())
                    .unwrap()
                    .clone()
            })
            .collect();
        self.rate = inputs.rate;
        ensure!(
            self.destination.is_absolute(),
            "destination must be absolute"
        );
        let operation = match session.record().map_err(anyhow::Error::msg)? {
            Effect::PrepareCapture(id) => id,
            _ => unreachable!(),
        };
        let path = new_path(&self.destination, "take");
        self.path = Some(path.clone());
        self.operation = Some(operation);
        self.observer = None;
        self.startup_error = None;
        let identity = Identity {
            version: 1,
            tracks: config.tracks.iter().filter(|t| t.armed).cloned().collect(),
            source_uuids: self.selected_sources.iter().map(|s| s.uuid).collect(),
        };
        let rate = self.rate;
        let destination = self.destination.clone();
        let worker = thread::Builder::new()
            .name("rec-prepare".into())
            .spawn(move || {
                fs::DirBuilder::new()
                    .recursive(true)
                    .mode(0o700)
                    .create(&destination)?;
                let recorder = Recorder::create(
                    &path,
                    recorder::Config {
                        sample_rate: rate,
                        channels: identity.tracks.len() as u32,
                        max_block: 8192,
                        queue_blocks: 16,
                        epoch: operation,
                    },
                )?;
                if let Err(e) = write_identity(&path, &identity) {
                    recorder.mark_fault(100);
                    let _ = recorder.finish();
                    return Err(e);
                }
                Ok(recorder)
            });
        match worker {
            Ok(w) => self.work = Some(Work::Preparing(w)),
            Err(e) => {
                self.complete(session, false, format!("prepare worker failed: {e}"));
                return Err(e.into());
            }
        }
        self.status = "Preparing writer; capture disabled".into();
        Ok(())
    }
    pub fn stop(&mut self, session: &mut Session) {
        session.stop();
        if let Some(shared) = &self.shared {
            shared.disable();
        }
        if let Some(recorder) = self.device.quiesce() {
            self.shared = None;
            self.finish(recorder);
        }
    }
    fn finish(&mut self, recorder: Recorder) {
        // Already past the producer barrier. If the OS cannot spawn a worker,
        // closure destruction drains the writer on this owner thread and keeps
        // its incomplete evidence; the next tick reports the explicit error.
        self.work = Some(
            match thread::Builder::new()
                .name("rec-finalize".into())
                .spawn(move || recorder.finish())
            {
                Ok(worker) => Work::Finishing(worker),
                Err(error) => Work::Failed(format!("cannot spawn finalizer: {error}")),
            },
        );
        self.status = "Finalizing; draining and syncing stems".into();
    }
    fn complete(&mut self, session: &mut Session, complete: bool, detail: String) {
        self.status = format!(
            "{}: {} — {}",
            if complete { "Complete" } else { "INCOMPLETE" },
            self.path
                .as_ref()
                .map_or_else(String::new, |p| p.display().to_string()),
            detail
        );
        if let Some(id) = self.operation.take() {
            let _ = session.capture_finished(
                id,
                RetainedTake {
                    path: self.path.clone().unwrap(),
                    complete,
                    detail: self.status.clone(),
                },
            );
        }
        self.recovering = false;
    }
    pub fn tick(&mut self, session: &mut Session) {
        self.device.poll();
        if self.shared.as_ref().is_some_and(|s| {
            s.failure() != 0
                || self.observer.as_ref().is_some_and(|o| {
                    let p = o.snapshot();
                    p.writer_fault != 0
                        || p.overflow_blocks != 0
                        || p.invalid_blocks != 0
                        || p.written_frames >= recorder::MAX_WAV_FRAMES
                })
        }) {
            if let Some(s) = &self.shared {
                self.startup_error = Some(format!(
                    "capture fault {} (1 route, 2 device, 3 rate, 4 xrun, 5 period, 6 queue/write)",
                    s.failure()
                ));
            }
            self.stop(session);
        }
        let finished = match &self.work {
            Some(Work::Preparing(w)) => w.is_finished(),
            Some(Work::Finishing(w)) => w.is_finished(),
            Some(Work::Failed(_)) => true,
            None => false,
        };
        if !finished {
            return;
        }
        match self.work.take().unwrap() {
            Work::Failed(error) => self.complete(session, false, error),
            Work::Preparing(w) => match w
                .join()
                .unwrap_or_else(|_| Err(anyhow::anyhow!("preparation worker panicked")))
            {
                Err(e) => self.complete(session, false, format!("{e:#}; retain any created files")),
                Ok(recorder) => {
                    self.observer = Some(recorder.observer());
                    if session.transport() != Transport::Starting {
                        recorder.mark_fault(101);
                        self.finish(recorder);
                        return;
                    }
                    match self
                        .device
                        .attach(recorder, &self.selected_sources, self.rate)
                    {
                        Err((recorder, e)) => {
                            recorder.mark_fault(102);
                            self.startup_error = Some(e);
                            session.stop();
                            self.finish(*recorder);
                        }
                        Ok(shared) => {
                            if shared.failure() != 0
                                || session.capture_ready(self.operation.unwrap()).is_err()
                            {
                                shared.fault(103);
                                self.shared = Some(shared);
                                self.stop(session);
                            } else {
                                shared.enable();
                                self.shared = Some(shared);
                                self.status = "Recording; STOP to finalize".into();
                            }
                        }
                    }
                }
            },
            Work::Finishing(w) => {
                let result = w
                    .join()
                    .unwrap_or_else(|_| Err(anyhow::anyhow!("finalization worker panicked")));
                match result {
                    Ok(report) => {
                        let complete = report.status == "complete"
                            && !self.recovering
                            && self.startup_error.is_none();
                        let detail = format!(
                            "{} frames; dropped {}, overflow {}, host {}; {}",
                            report.frames,
                            report.dropped_frames,
                            report.overflow_blocks,
                            report.external_fault,
                            self.startup_error
                                .take()
                                .or(report.error)
                                .unwrap_or(report.status)
                        );
                        self.complete(session, complete, detail);
                    }
                    Err(e) => {
                        self.complete(session, false, format!("{e:#}; original evidence retained"))
                    }
                }
            }
        }
    }
    pub fn recover(&mut self, source: PathBuf, session: &mut Session) -> Result<()> {
        ensure!(!self.busy(), "STOP before recovery");
        ensure!(
            self.destination.is_absolute(),
            "destination must be absolute"
        );
        let id = match session.record().map_err(anyhow::Error::msg)? {
            Effect::PrepareCapture(id) => id,
            _ => unreachable!(),
        };
        session.stop();
        self.operation = Some(id);
        self.observer = None;
        self.startup_error = None;
        self.recovering = true;
        let path = new_path(&self.destination, "recovered");
        self.path = Some(path.clone());
        let parent = self.destination.clone();
        self.work = Some(
            match thread::Builder::new()
                .name("rec-recovery".into())
                .spawn(move || {
                    fs::DirBuilder::new()
                        .recursive(true)
                        .mode(0o700)
                        .create(parent)?;
                    recorder::recover_with_metadata(&source, &path, |directory, count| {
                        crate::take::capture_identity(directory, count as usize)?
                            .map(|identity| {
                                serde_json::to_vec_pretty(&identity).map_err(Into::into)
                            })
                            .transpose()
                    })
                }) {
                Ok(worker) => Work::Finishing(worker),
                Err(error) => Work::Failed(format!("cannot spawn recovery: {error}")),
            },
        );
        self.status = "Copy recovery; original evidence retained".into();
        Ok(())
    }
    pub fn peak(&self, armed_index: usize) -> f32 {
        self.shared.as_ref().map_or(0., |s| s.peak(armed_index))
    }
    pub fn progress(&self) -> String {
        if let Some(o) = &self.observer {
            let p = o.snapshot();
            format!(
                "{}s A{} W{} drop{} over{}",
                p.accepted_frames / u64::from(self.rate.max(1)),
                p.accepted_frames,
                p.written_frames,
                p.dropped_frames,
                p.overflow_blocks
            )
        } else {
            String::new()
        }
    }
    pub fn limit(&self, config: &Config) -> String {
        let rate = self.inputs.as_ref().map_or(48000, |i| i.rate);
        format!(
            "RIFF {}s; {} B/s; 16MiB reserve",
            recorder::MAX_WAV_FRAMES / u64::from(rate),
            u64::from(rate) * config.tracks.iter().filter(|t| t.armed).count() as u64 * 3
        )
    }
}
impl Drop for Capture {
    fn drop(&mut self) {
        if let Some(r) = self.device.quiesce() {
            r.mark_fault(104);
            let _ = r.finish();
        }
        if let Some(work) = self.work.take() {
            match work {
                Work::Preparing(w) => {
                    if let Ok(Ok(r)) = w.join() {
                        r.mark_fault(104);
                        let _ = r.finish();
                    }
                }
                Work::Finishing(w) => {
                    let _ = w.join();
                }
                Work::Failed(_) => {}
            }
        }
    }
}
fn new_path(root: &Path, prefix: &str) -> PathBuf {
    let ns = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    root.join(format!("{prefix}-{ns}-{}.take", std::process::id()))
}
fn write_identity(path: &Path, identity: &Identity) -> Result<()> {
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path.join("capture.json"))?;
    f.write_all(&serde_json::to_vec_pretty(identity)?)?;
    f.sync_all()?;
    File::open(path)?.sync_all()?;
    Ok(())
}
pub fn resolution(track: &Track, inputs: &[String]) -> &'static str {
    match track.resolve(inputs) {
        Resolution::Unassigned => "UNSET",
        Resolution::Missing => "MISSING",
        Resolution::Ambiguous => "AMBIGUOUS",
        Resolution::Resolved => "READY",
    }
}
