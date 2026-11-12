//! Standalone take browser/transport. Device injection uses the same workflow in
//! offline tests; the default adapter opens JACK only on OUTPUTS or PLAY.
use crate::{
    jack_playback::{Device, Jack},
    playback::{Mix, Routing, Shared, State},
    take::{self, Entry, Take},
    ui::{Action, Button},
};
use anyhow::{Context, Result, ensure};
use ratatui::{
    Frame,
    backend::Backend,
    layout::Rect,
    style::{Color, Style},
    widgets::Paragraph,
};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Takes,
    Transport,
    Routes,
    Mix,
    Damaged,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Saved {
    version: u32,
    take: PathBuf,
    ids: Vec<String>,
    routing: Routing,
}
pub struct PlaybackUi {
    pub active: bool,
    pub page: Page,
    pub entries: Vec<Entry>,
    pub selected: usize,
    pub status: String,
    pub routing: Option<Routing>,
    pub outputs: Vec<String>,
    pub stem: usize,
    pub position: u64,
    pub frames: u64,
    pub rate: u32,
    pub label: String,
    pub warning: Option<String>,
    pub ids: Vec<String>,
    pub labels: Vec<String>,
    root: PathBuf,
    library: PathBuf,
    path: Option<PathBuf>,
    allow_damaged: bool,
    draft_before: Option<Option<Routing>>,
    pub details: bool,
    device: Box<dyn Device>,
    shared: Option<Arc<Shared>>,
}
impl PlaybackUi {
    pub fn new(root: PathBuf, library: PathBuf) -> Self {
        Self::with_device(root, library, Box::<Jack>::default())
    }
    pub fn with_device(root: PathBuf, library: PathBuf, device: Box<dyn Device>) -> Self {
        Self {
            active: false,
            page: Page::Takes,
            entries: vec![],
            selected: 0,
            status: String::new(),
            routing: None,
            outputs: vec![],
            stem: 0,
            position: 0,
            frames: 0,
            rate: 0,
            label: String::new(),
            warning: None,
            ids: vec![],
            labels: vec![],
            root,
            library,
            path: None,
            allow_damaged: false,
            draft_before: None,
            details: false,
            device,
            shared: None,
        }
    }
    pub fn refresh(&mut self) -> Result<()> {
        let previous = self.entries.get(self.selected).map(|e| e.path.clone());
        self.entries = take::discover(&self.library)?;
        self.selected = previous
            .and_then(|p| self.entries.iter().position(|e| e.path == p))
            .unwrap_or(self.selected.min(self.entries.len().saturating_sub(1)));
        self.status = if self.entries.is_empty() {
            "No takes; set --takes-root".into()
        } else {
            format!("{} takes; OPEN validates", self.entries.len())
        };
        Ok(())
    }
    pub fn open(&mut self, path: &Path) -> Result<()> {
        ensure!(self.shared.is_none(), "STOP before changing take");
        let take = Take::open(path)?;
        self.path = Some(take.path.clone());
        self.label = take.label.clone();
        self.frames = take.frames;
        self.rate = take.sample_rate;
        self.position = 0;
        self.stem = 0;
        self.ids = take.stems.iter().map(|s| s.id.clone()).collect();
        self.labels = take.stems.iter().map(|s| s.label.clone()).collect();
        self.routing = None;
        self.draft_before = None;
        self.allow_damaged = false;
        self.warning = take.warning.clone();
        self.status = take
            .warning
            .clone()
            .unwrap_or_else(|| "Assign OUTPUTS before PLAY".into());
        self.page = if take.warning.is_some() {
            Page::Damaged
        } else {
            Page::Transport
        };
        // Loading preferences never discovers, connects or substitutes devices.
        match self.load_routes() {
            Ok(()) => {}
            Err(e) => self.status = format!("Output settings: {e:#}"),
        };
        Ok(())
    }
    fn load_routes(&mut self) -> Result<()> {
        let path = self.root.join("playback.json");
        let file = match OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)
        {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e.into()),
        };
        ensure!(
            file.metadata()?.is_file() && file.metadata()?.len() <= 128 * 1024,
            "invalid settings file"
        );
        let mut bytes = Vec::new();
        file.take(128 * 1024 + 1).read_to_end(&mut bytes)?;
        ensure!(bytes.len() <= 128 * 1024, "settings too large");
        let saved: Saved = serde_json::from_slice(&bytes)?;
        ensure!(saved.version == 1, "unsupported settings version");
        if self.path.as_ref() == Some(&saved.take) && saved.ids == self.ids {
            saved.routing.validate(self.ids.len())?;
            self.routing = Some(saved.routing);
        }
        Ok(())
    }
    fn save(&self) -> Result<()> {
        let routing = self.routing.as_ref().context("assign outputs first")?;
        routing.validate(self.ids.len())?;
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&self.root)?;
        let tmp = self.root.join(".playback.json.part");
        let saved = Saved {
            version: 1,
            take: self.path.clone().context("open a take")?,
            ids: self.ids.clone(),
            routing: routing.clone(),
        };
        let bytes = serde_json::to_vec_pretty(&saved)?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)
            .context("create settings; remove a stale .playback.json.part only after inspection")?;
        let result = (|| -> Result<()> {
            file.write_all(&bytes)?;
            file.sync_all()?;
            fs::rename(&tmp, self.root.join("playback.json"))?;
            std::fs::File::open(&self.root)?.sync_all()?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(tmp);
        }
        result
    }
    pub fn state(&self) -> State {
        self.shared.as_ref().map_or(State::Stopped, |s| s.state())
    }
    pub fn tick(&mut self) {
        self.device.poll();
        if let Some(s) = &self.shared {
            self.position = s.position();
            match s.state() {
                State::Eof
                | State::Underrun
                | State::ReadError
                | State::DeviceLost
                | State::RateMismatch
                | State::Xrun
                | State::RouteLost
                | State::InvalidPeriod => {
                    let state = s.state();
                    let detail = s.failure_detail();
                    self.device.stop();
                    self.position = s.position();
                    self.shared = None;
                    self.status = match state {
                        State::Eof => "End of take; PLAY restarts",
                        State::Underrun => "Underrun; stopped all stems",
                        State::ReadError => "Disk read failed; all stems stopped",
                        State::RateMismatch => "JACK sample rate changed; stopped",
                        State::Xrun => "JACK xrun; all stems stopped",
                        State::RouteLost => "Exact output disconnected; stopped",
                        State::InvalidPeriod => "Unsupported JACK period; stopped",
                        _ => "JACK device lost; all stems stopped",
                    }
                    .into();
                    if let Some(detail) = detail {
                        self.status.push_str(": ");
                        self.status.push_str(&detail);
                    }
                }
                _ => {}
            }
        }
    }
    pub fn stop(&mut self) {
        self.device.stop();
        self.shared = None;
        self.position = 0;
    }
    fn start(&mut self, frame: u64, play: bool) -> Result<()> {
        let take = Take::open(self.path.as_deref().context("OPEN a take first")?)?;
        // Revalidate identity before reusing remembered routes after source edits.
        ensure!(
            take.frames == self.frames
                && take.sample_rate == self.rate
                && take
                    .stems
                    .iter()
                    .map(|s| s.id.as_str())
                    .eq(self.ids.iter().map(String::as_str)),
            "take changed; OPEN again"
        );
        let routing = self.routing.as_ref().context("assign OUTPUTS first")?;
        let shared = self
            .device
            .start(take, routing, self.allow_damaged, frame)?;
        if shared.state() != State::Paused {
            self.device.stop();
            anyhow::bail!("JACK changed during playback startup");
        }
        if play && let Err(error) = shared.play() {
            self.device.stop();
            return Err(error);
        }
        self.shared = Some(shared);
        self.position = frame;
        Ok(())
    }
    fn seek(&mut self, delta: i64) -> Result<()> {
        ensure!(self.frames > 0, "OPEN a take first");
        let playing = self.state() == State::Playing;
        let active = self.shared.is_some();
        self.device.stop();
        if let Some(shared) = self.shared.take() {
            self.position = shared.position();
        }
        let target =
            (self.position as i64 + delta).clamp(0, self.frames.saturating_sub(1) as i64) as u64;
        self.position = target;
        if active {
            self.start(target, playing)?;
        }
        self.status = "Seek: all stems reprime together".into();
        Ok(())
    }
    fn edit_route(&mut self) -> Result<()> {
        ensure!(self.shared.is_none(), "STOP before output setup");
        let routing = self
            .routing
            .as_mut()
            .context("choose exact or stereo mode")?;
        let target = match routing {
            Routing::Exact { outputs } => &mut outputs[self.stem],
            Routing::Stereo { left, right, .. } => {
                if self.stem == 0 {
                    left
                } else {
                    right
                }
            }
        };
        let index = self
            .outputs
            .iter()
            .position(|p| p == target)
            .map_or(0, |i| (i + 1) % self.outputs.len().max(1));
        *target = self
            .outputs
            .get(index)
            .context("no JACK audio destinations")?
            .clone();
        Ok(())
    }
    pub fn dispatch(&mut self, action: Action) {
        if let Err(error) = self.handle(action) {
            self.status = format!("{error:#}");
        }
    }
    fn handle(&mut self, action: Action) -> Result<()> {
        match action {
            Action::Previous => match self.page {
                Page::Takes => self.selected = self.selected.saturating_sub(1),
                Page::Mix => self.stem = self.stem.saturating_sub(1),
                Page::Routes => self.stem = self.stem.saturating_sub(1),
                _ => {}
            },
            Action::Next => match self.page {
                Page::Takes => {
                    self.selected = (self.selected + 1).min(self.entries.len().saturating_sub(1))
                }
                Page::Mix => self.stem = (self.stem + 1).min(self.ids.len().saturating_sub(1)),
                Page::Routes => {
                    let n = match &self.routing {
                        Some(Routing::Stereo { .. }) => 2,
                        _ => self.ids.len(),
                    };
                    self.stem = (self.stem + 1).min(n.saturating_sub(1));
                }
                _ => {}
            },
            Action::Select(index) => {
                if self.page == Page::Takes && index < self.entries.len() {
                    self.selected = index;
                }
            }
            Action::Open => {
                let path = self
                    .entries
                    .get(self.selected)
                    .context("No take selected")?
                    .path
                    .clone();
                self.open(&path)?;
            }
            Action::Refresh => {
                if self.page == Page::Takes {
                    self.refresh()?;
                } else {
                    self.outputs = self.device.outputs()?;
                    self.status = format!("{} exact destinations", self.outputs.len());
                }
            }
            Action::Confirm if self.page == Page::Damaged => {
                self.allow_damaged = true;
                self.page = Page::Transport;
                self.status = "Damaged take deliberately selected".into();
            }
            Action::Cancel => match self.page {
                Page::Takes => {
                    self.active = false;
                }
                Page::Mix => {
                    self.page = Page::Routes;
                    self.stem = 0;
                }
                Page::Routes => {
                    if let Some(previous) = self.draft_before.take() {
                        self.routing = previous;
                    }
                    self.page = Page::Transport;
                    self.status = "Output edits cancelled".into();
                }
                _ => {
                    self.stop();
                    self.page = Page::Takes;
                }
            },
            Action::Takes => {
                ensure!(self.shared.is_none(), "STOP before browsing");
                self.page = Page::Takes;
                self.refresh()?;
            }
            Action::Play => {
                ensure!(
                    self.page == Page::Transport,
                    "OPEN and confirm the selected take before PLAY"
                );
                match self.state() {
                    State::Playing => {
                        self.shared.as_ref().unwrap().pause();
                        self.status = "Paused on common timeline".into();
                    }
                    State::Paused => {
                        self.shared.as_ref().unwrap().play()?;
                        self.status = "Playing".into();
                    }
                    _ => {
                        let at = if self.position >= self.frames {
                            0
                        } else {
                            self.position
                        };
                        self.start(at, true)?;
                        self.status = "Playing".into();
                    }
                }
            }
            Action::Stop => {
                self.stop();
                self.status = "Stopped; position reset".into();
            }
            Action::SeekBack => self.seek(-i64::from(self.rate) * 5)?,
            Action::SeekForward => self.seek(i64::from(self.rate) * 5)?,
            Action::Outputs => {
                ensure!(self.shared.is_none(), "STOP before output setup");
                self.draft_before = Some(self.routing.clone());
                self.page = Page::Routes;
                self.stem = 0;
                self.outputs = self.device.outputs()?;
                self.status = "Choose mode; assign exact ports".into();
            }
            Action::Mode => {
                ensure!(self.shared.is_none(), "STOP before editing");
                self.stem = 0;
                self.routing = Some(match &self.routing {
                    Some(Routing::Exact { .. }) => Routing::Stereo {
                        left: String::new(),
                        right: String::new(),
                        stems: vec![Mix::default(); self.ids.len()],
                    },
                    _ => Routing::Exact {
                        outputs: vec![String::new(); self.ids.len()],
                    },
                });
            }
            Action::Assign => self.edit_route()?,
            Action::Details => self.details = !self.details,
            Action::Apply => {
                self.routing
                    .as_ref()
                    .context("choose output mode")?
                    .validate(self.ids.len())?;
                self.draft_before = None;
                self.page = Page::Transport;
                self.status = "Output assignment applied".into();
            }
            Action::Save => {
                self.save()?;
                self.draft_before = Some(self.routing.clone());
                self.status = "Saved exact outputs in private config".into();
            }
            Action::Mix => {
                ensure!(self.shared.is_none(), "STOP before editing");
                ensure!(
                    matches!(self.routing, Some(Routing::Stereo { .. })),
                    "choose stereo audition mode first"
                );
                self.stem = 0;
                self.page = Page::Mix;
            }
            Action::LevelDown | Action::LevelUp | Action::Pan | Action::Mute => {
                ensure!(self.shared.is_none(), "STOP before editing");
                if let Some(Routing::Stereo { stems, .. }) = &mut self.routing {
                    let m = &mut stems[self.stem];
                    match action {
                        Action::LevelDown => m.level = (m.level - 0.1).max(0.),
                        Action::LevelUp => m.level = (m.level + 0.1).min(1.),
                        Action::Pan => {
                            m.pan = if m.pan >= 1. {
                                -1.
                            } else {
                                (m.pan + 0.25).min(1.)
                            }
                        }
                        Action::Mute => m.mute = !m.mute,
                        _ => {}
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }
    pub fn buttons(&self) -> [Button; 8] {
        let cells = match self.page {
            Page::Takes => [
                ("PREV", Action::Previous),
                ("NEXT", Action::Next),
                ("OPEN", Action::Open),
                ("REFRESH", Action::Refresh),
                ("", Action::Cancel),
                ("", Action::Cancel),
                ("INPUTS", Action::Cancel),
                ("EXIT", Action::Exit),
            ],
            Page::Transport => [
                ("PLAY/PAUSE", Action::Play),
                ("STOP", Action::Stop),
                ("-5s", Action::SeekBack),
                ("+5s", Action::SeekForward),
                ("OUTPUTS", Action::Outputs),
                ("TAKES", Action::Takes),
                ("", Action::Cancel),
                ("EXIT", Action::Exit),
            ],
            Page::Damaged => [
                ("ACCEPT", Action::Confirm),
                ("CANCEL", Action::Cancel),
                ("", Action::Cancel),
                ("", Action::Cancel),
                ("", Action::Cancel),
                ("", Action::Cancel),
                ("", Action::Cancel),
                ("EXIT", Action::Exit),
            ],
            Page::Routes => [
                ("PREV", Action::Previous),
                ("NEXT", Action::Next),
                ("MODE", Action::Mode),
                ("ASSIGN", Action::Assign),
                ("MIX", Action::Mix),
                ("SAVE", Action::Save),
                ("CANCEL", Action::Cancel),
                ("APPLY", Action::Apply),
            ],
            Page::Mix => [
                ("PREV", Action::Previous),
                ("NEXT", Action::Next),
                ("LEVEL -", Action::LevelDown),
                ("LEVEL +", Action::LevelUp),
                ("PAN", Action::Pan),
                ("MUTE", Action::Mute),
                ("BACK", Action::Cancel),
                ("SAVE", Action::Save),
            ],
        };
        cells.map(|(label, action)| Button { label, action })
    }
    pub fn first(&self) -> usize {
        self.selected.saturating_sub(4)
    }
    pub fn draw<B: Backend>(&self, frame: &mut Frame<B>) {
        fn line<B: Backend>(f: &mut Frame<B>, row: u16, text: &str) {
            f.render_widget(Paragraph::new(text), Rect::new(0, row, 40, 1));
        }
        line(
            frame,
            0,
            &format!(
                "shr-rec {}  {:?}",
                if cfg!(debug_assertions) { "DEV" } else { "REL" },
                self.page
            ),
        );
        match self.page {
            Page::Takes => {
                line(frame, 1, "Select a take; OPEN validates all stems");
                for (row, (i, e)) in self
                    .entries
                    .iter()
                    .enumerate()
                    .skip(self.first())
                    .take(5)
                    .enumerate()
                {
                    line(
                        frame,
                        row as u16 + 2,
                        &format!("{} {}", if i == self.selected { ">" } else { " " }, e.label),
                    );
                }
            }
            Page::Transport | Page::Damaged => {
                line(frame, 1, &self.label);
                line(
                    frame,
                    2,
                    &format!("{} stems   {} Hz", self.ids.len(), self.rate),
                );
                line(
                    frame,
                    3,
                    &format!(
                        "{:02}:{:02} / {:02}:{:02}",
                        self.position / u64::from(self.rate.max(1)) / 60,
                        self.position / u64::from(self.rate.max(1)) % 60,
                        self.frames / u64::from(self.rate.max(1)) / 60,
                        self.frames / u64::from(self.rate.max(1)) % 60
                    ),
                );
                let n = (self.position * 36)
                    .checked_div(self.frames)
                    .unwrap_or(0)
                    .min(36) as usize;
                line(
                    frame,
                    4,
                    &format!("[{}{}]", "●".repeat(n), "○".repeat(36 - n)),
                );
                if self.warning.is_some() {
                    line(frame, 5, "! INCOMPLETE / RECOVERED / INTEGRITY");
                }
                line(
                    frame,
                    6,
                    match self.routing {
                        Some(Routing::Exact { .. }) => "Exact stem outputs (unity)",
                        Some(Routing::Stereo { .. }) => "Stereo audition; -6 dB + 1/N headroom",
                        None => "Outputs unassigned",
                    },
                );
                line(frame, 9, "Tap status for full details");
                if self.page == Page::Damaged {
                    line(frame, 7, "INCOMPLETE / RECOVERED / INTEGRITY");
                    line(frame, 8, "ACCEPT to audition; source read-only");
                }
            }
            Page::Routes => {
                line(frame, 1, "MODE chooses exact / stereo audition");
                let port = match &self.routing {
                    Some(Routing::Exact { outputs }) => {
                        line(
                            frame,
                            2,
                            &format!(
                                "{}/{} {}",
                                self.stem + 1,
                                self.ids.len(),
                                self.labels.get(self.stem).map_or("", String::as_str)
                            ),
                        );
                        outputs.get(self.stem).map_or("", String::as_str)
                    }
                    Some(Routing::Stereo { left, right, .. }) => {
                        line(
                            frame,
                            2,
                            if self.stem == 0 {
                                "Stereo LEFT; -6 dB + 1/N headroom"
                            } else {
                                "Stereo RIGHT; -6 dB + 1/N headroom"
                            },
                        );
                        if self.stem == 0 {
                            left.as_str()
                        } else {
                            right.as_str()
                        }
                    }
                    None => "Choose MODE to begin assignment",
                };
                frame.render_widget(
                    Paragraph::new(if port.is_empty() {
                        "UNASSIGNED; ASSIGN cycles exact ports"
                    } else {
                        port
                    })
                    .wrap(ratatui::widgets::Wrap { trim: false }),
                    Rect::new(0, 3, 40, 7),
                );
            }
            Page::Mix => {
                if let Some(Routing::Stereo { stems, .. }) = &self.routing {
                    let m = &stems[self.stem];
                    line(
                        frame,
                        1,
                        &format!("Stem {} / {}", self.stem + 1, self.ids.len()),
                    );
                    line(frame, 2, &self.labels[self.stem]);
                    line(
                        frame,
                        4,
                        &format!("Level {:3.0}%  Pan {:+.2}", m.level * 100., m.pan),
                    );
                    line(frame, 5, if m.mute { "MUTED" } else { "Audible" });
                    line(frame, 7, "-6 dB + 1/N; PAN cycles L..R");
                }
            }
        }
        if self.details {
            frame.render_widget(
                Paragraph::new("▼ Status details (tap title to close)"),
                Rect::new(0, 0, 40, 1),
            );
            frame.render_widget(ratatui::widgets::Clear, Rect::new(0, 1, 40, 9));
            frame.render_widget(
                Paragraph::new(self.status.as_str()).wrap(ratatui::widgets::Wrap { trim: false }),
                Rect::new(0, 1, 40, 9),
            );
        }
        for (i, b) in self.buttons().iter().enumerate() {
            frame.render_widget(
                Paragraph::new(b.label).style(Style::default().fg(Color::Cyan)),
                Rect::new((i % 4) as u16 * 10, 10 + (i / 4) as u16, 10, 1),
            );
        }
        let symbol = match self.state() {
            State::Playing => ">",
            State::Paused => "‖",
            _ => "■",
        };
        line(frame, 12, &format!("{symbol} {}", self.status));
    }
}
impl Drop for PlaybackUi {
    fn drop(&mut self) {
        self.device.stop();
    }
}
