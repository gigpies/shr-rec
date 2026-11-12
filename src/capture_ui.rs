//! Compact, keyboardless capture setup. Draft source/destination selection is
//! cancellable; discovery never rewrites a remembered exact source.
use crate::{
    capture::Capture,
    config::Config,
    session::Transport,
    ui::{Action, Button},
};
use anyhow::{Result, ensure};
use ratatui::{
    Frame,
    backend::Backend,
    layout::Rect,
    style::{Color, Style},
    widgets::{Paragraph, Wrap},
};
use std::path::PathBuf;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Inputs,
    Setup,
    Source,
    Destination,
    Details,
    Recovery,
}
pub struct View {
    pub page: Page,
    pub root: PathBuf,
    pub cursor: usize,
    pub directory: PathBuf,
    pub directories: Vec<PathBuf>,
    pub recovery: Option<PathBuf>,
    previous: Page,
    scroll: u16,
}
impl Default for View {
    fn default() -> Self {
        Self {
            page: Page::Inputs,
            root: PathBuf::new(),
            cursor: 0,
            directory: PathBuf::new(),
            directories: vec![],
            recovery: None,
            previous: Page::Inputs,
            scroll: 0,
        }
    }
}
impl View {
    pub fn buttons(&self) -> [Button; 8] {
        use Action::*;
        let cells = match self.page {
            Page::Inputs => [
                ("PREV", Previous),
                ("NEXT", Next),
                ("ARM", Arm),
                ("RECORD", Record),
                ("SETUP", Inputs),
                ("STOP", Stop),
                ("TAKES", Takes),
                ("EXIT", Exit),
            ],
            Page::Setup => [
                ("PREV", Previous),
                ("NEXT", Next),
                ("ASSIGN", Assign),
                ("REFRESH", Refresh),
                ("DEST", Destination),
                ("SAVE", Save),
                ("BACK", Cancel),
                ("EXIT", Exit),
            ],
            Page::Source => [
                ("PREV", Previous),
                ("NEXT", Next),
                ("USE", Apply),
                ("UNSET", Mute),
                ("", Cancel),
                ("", Cancel),
                ("CANCEL", Cancel),
                ("EXIT", Exit),
            ],
            Page::Destination => [
                ("PREV", Previous),
                ("NEXT", Next),
                ("OPEN", Open),
                ("UP", SeekBack),
                ("USE DIR", Apply),
                ("", Cancel),
                ("CANCEL", Cancel),
                ("EXIT", Exit),
            ],
            Page::Details => [
                ("PREV", Previous),
                ("NEXT", Next),
                ("", Cancel),
                ("", Cancel),
                ("", Cancel),
                ("STOP", Stop),
                ("", Cancel),
                ("EXIT", Exit),
            ],
            Page::Recovery => [
                ("COPY", Confirm),
                ("CANCEL", Cancel),
                ("", Cancel),
                ("", Cancel),
                ("", Cancel),
                ("", Cancel),
                ("", Cancel),
                ("EXIT", Exit),
            ],
        };
        cells.map(|(label, action)| Button { label, action })
    }
    pub fn details(&mut self) {
        if self.page == Page::Details {
            self.page = self.previous;
        } else {
            self.previous = self.page;
            self.scroll = 0;
            self.page = Page::Details;
        }
    }
    pub fn browse(&mut self, path: PathBuf) -> Result<()> {
        let path = path.canonicalize()?;
        ensure!(path.is_dir(), "select a directory");
        let mut dirs = vec![];
        for (index, e) in std::fs::read_dir(&path)?.enumerate() {
            ensure!(index < 4096, "directory exceeds 4096 entries");
            let e = e?;
            if e.file_type()?.is_dir() {
                ensure!(
                    dirs.len() < 256,
                    "too many directories; choose smaller folder"
                );
                dirs.push(e.path());
            }
        }
        dirs.sort();
        self.directory = path;
        self.directories = dirs;
        self.cursor = 0;
        Ok(())
    }
    pub fn edit(
        &mut self,
        action: Action,
        capture: &mut Capture,
        config: &mut Config,
        selected: &mut usize,
        status: &mut String,
    ) -> Result<bool> {
        use Action::*;
        if action == Details {
            self.details();
            return Ok(true);
        }
        if self.page == Page::Inputs {
            return Ok(false);
        }
        match self.page {
            Page::Setup => match action {
                Previous => *selected = selected.saturating_sub(1),
                Next => *selected = (*selected + 1).min(config.tracks.len() - 1),
                Assign => {
                    self.cursor = config.tracks[*selected]
                        .preferred_source
                        .as_ref()
                        .and_then(|s| capture.sources().iter().position(|n| n == s))
                        .unwrap_or(0);
                    self.page = Page::Source;
                }
                Refresh => {
                    capture.refresh()?;
                    *status = capture.status.clone();
                }
                Destination => {
                    let mut p = capture.destination.clone();
                    while !p.is_dir() {
                        if !p.pop() {
                            p = PathBuf::from("/");
                            break;
                        }
                    }
                    self.browse(p)?;
                    self.page = Page::Destination;
                }
                Save => {
                    capture.save(&self.root, config)?;
                    *status = "Capture routes/destination saved privately".into();
                }
                Cancel => self.page = Page::Inputs,
                _ => return Ok(false),
            },
            Page::Source => match action {
                Previous => self.cursor = self.cursor.saturating_sub(1),
                Next => {
                    self.cursor = (self.cursor + 1).min(capture.sources().len().saturating_sub(1))
                }
                Apply => {
                    config.tracks[*selected].preferred_source = Some(
                        capture
                            .sources()
                            .get(self.cursor)
                            .ok_or_else(|| anyhow::anyhow!("REFRESH sources first"))?
                            .clone(),
                    );
                    self.page = Page::Setup;
                }
                Mute => {
                    config.tracks[*selected].preferred_source = None;
                    self.page = Page::Setup;
                }
                Cancel => self.page = Page::Setup,
                _ => return Ok(false),
            },
            Page::Destination => match action {
                Previous => self.cursor = self.cursor.saturating_sub(1),
                Next => {
                    self.cursor = (self.cursor + 1).min(self.directories.len().saturating_sub(1))
                }
                Open => {
                    let p = self
                        .directories
                        .get(self.cursor)
                        .ok_or_else(|| anyhow::anyhow!("No child directory"))?
                        .clone();
                    self.browse(p)?;
                }
                SeekBack => {
                    let p = self
                        .directory
                        .parent()
                        .unwrap_or(&self.directory)
                        .to_path_buf();
                    self.browse(p)?;
                }
                Apply => {
                    capture.destination = self.directory.clone();
                    self.page = Page::Setup;
                    *status = "Destination selected; SAVE to remember".into();
                }
                Cancel => self.page = Page::Setup,
                _ => return Ok(false),
            },
            Page::Details => match action {
                Previous => self.scroll = self.scroll.saturating_sub(8),
                Next => self.scroll = self.scroll.saturating_add(8).min(512),
                Cancel => self.page = self.previous,
                _ => return Ok(false),
            },
            _ => return Ok(false),
        }
        Ok(true)
    }
    pub fn draw<B: Backend>(
        &self,
        frame: &mut Frame<B>,
        capture: &Capture,
        config: &Config,
        selected: usize,
        status: &str,
        transport: Transport,
    ) {
        let text = match self.page {
            Page::Setup => format!(
                "CAPTURE SETUP — tap title to return\nChannel {} / {}: {}\nExact source:\n{}\nStatus: {}\nJACK: {}\nDestination:\n{}\n{}",
                selected + 1,
                config.tracks.len(),
                config.tracks[selected].label,
                config.tracks[selected]
                    .preferred_source
                    .as_deref()
                    .unwrap_or("UNSET"),
                crate::capture::resolution(&config.tracks[selected], &capture.sources()),
                capture.inputs.as_ref().map_or_else(
                    || "not inspected".into(),
                    |i| format!("{} Hz / {} ports", i.rate, i.sources.len())
                ),
                capture.destination.display(),
                capture.limit(config)
            ),
            Page::Source => format!(
                "SELECT EXACT SOURCE\n{} / {}\n{}\n\nUSE applies; CANCEL keeps old route.\nNames are never inferred from numbers.",
                self.cursor + 1,
                capture.sources().len(),
                capture
                    .sources()
                    .get(self.cursor)
                    .map_or("No discovered ports", String::as_str)
            ),
            Page::Destination => format!(
                "SELECT DESTINATION\n{}\n\nChild {} / {}:\n{}\n\nOPEN descends; UP goes to parent.\nUSE DIR selects current directory.",
                self.directory.display(),
                self.cursor + 1,
                self.directories.len(),
                self.directories.get(self.cursor).map_or_else(
                    || "No child directories".into(),
                    |p| p
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned()
                )
            ),
            Page::Details => format!(
                "{transport:?}\n{status}\n{}\n{}",
                capture.progress(),
                capture.limit(config)
            ),
            Page::Recovery => format!(
                "RECOVER COPY?\n{}\n\nOriginal files remain unchanged.\nValidate every stem, copy common frames.\nResult stays recovered-incomplete.\nDestination: {}",
                self.recovery
                    .as_ref()
                    .map_or_else(String::new, |p| p.display().to_string()),
                capture.destination.display()
            ),
            Page::Inputs => return,
        };
        if self.page == Page::Details {
            frame.render_widget(
                Paragraph::new("CAPTURE STATUS — tap title to close"),
                Rect::new(0, 0, 40, 1),
            );
        }
        frame.render_widget(
            Paragraph::new(text).wrap(Wrap { trim: false }).scroll((
                if self.page == Page::Details {
                    self.scroll
                } else {
                    0
                },
                0,
            )),
            if self.page == Page::Details {
                Rect::new(0, 1, 40, 9)
            } else {
                Rect::new(0, 0, 40, 10)
            },
        );
        for (index, b) in self.buttons().iter().enumerate() {
            frame.render_widget(
                Paragraph::new(b.label).style(Style::default().fg(Color::Cyan)),
                Rect::new((index % 4) as u16 * 10, 10 + (index / 4) as u16, 10, 1),
            );
        }
        frame.render_widget(Paragraph::new(status), Rect::new(0, 12, 40, 1));
    }
}
