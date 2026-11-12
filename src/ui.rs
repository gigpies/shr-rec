use crate::{
    config::Config,
    session::{Session, Transport},
};
use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind};
use ratatui::{
    Frame,
    backend::Backend,
    layout::Rect,
    style::{Color, Style},
    widgets::Paragraph,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Previous,
    Next,
    Arm,
    Record,
    Play,
    Stop,
    Exit,
    Confirm,
    Cancel,
    Select(usize),
    Apply,
    Details,
    Takes,
    Open,
    Refresh,
    SeekBack,
    SeekForward,
    Outputs,
    Mode,
    Assign,
    Save,
    Mix,
    LevelDown,
    LevelUp,
    Pan,
    Mute,
    Inputs,
    Destination,
    Recover,
}

#[derive(Clone, Copy)]
pub struct Button {
    pub label: &'static str,
    pub action: Action,
}

const CONFIRM: [Button; 2] = [
    Button {
        label: "STOP AND FINALIZE",
        action: Action::Confirm,
    },
    Button {
        label: "CANCEL",
        action: Action::Cancel,
    },
];

pub struct App {
    pub config: Config,
    pub session: Session,
    pub selected: usize,
    pub status: String,
    pub playback: crate::playback_ui::PlaybackUi,
    pub capture: crate::capture::Capture,
    pub capture_view: crate::capture_ui::View,
}

impl App {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            session: Session::default(),
            selected: 0,
            status: "SETUP inputs or TAKES for playback".into(),
            capture: crate::capture::Capture::new(std::path::PathBuf::new()),
            capture_view: crate::capture_ui::View::default(),
            playback: crate::playback_ui::PlaybackUi::new(
                std::path::PathBuf::new(),
                std::path::PathBuf::new(),
            ),
        }
    }

    fn synchronize_playback(&mut self) {
        use crate::playback::State;
        match self.playback.state() {
            State::Playing => {
                if matches!(
                    self.session.transport(),
                    Transport::Stopped | Transport::Paused
                ) {
                    let _ = self.session.play();
                }
            }
            State::Paused => {
                if self.session.transport() == Transport::Playing {
                    let _ = self.session.pause();
                }
            }
            _ => {
                if matches!(
                    self.session.transport(),
                    Transport::Playing | Transport::Paused | Transport::StoppingPlayback
                ) {
                    let _ = self.session.playback_stopped();
                }
            }
        }
    }

    pub fn tick(&mut self) {
        let was_busy = self.capture.busy();
        self.capture.tick(&mut self.session);
        if was_busy {
            self.status = self.capture.status.clone();
        }
        self.playback.tick();
        self.synchronize_playback();
    }

    pub fn shutdown(&mut self) {
        self.playback.stop();
        self.synchronize_playback();
        self.session.request_exit();
        if self.session.exit_confirmation() {
            self.session.confirm_exit();
        }
        if self.capture.busy() {
            self.capture.stop(&mut self.session);
        }
    }

    pub fn dispatch(&mut self, action: Action) {
        if self.playback.active {
            if action == Action::Recover && self.session.transport() == Transport::Stopped {
                if let Some(entry) = self.playback.entries.get(self.playback.selected) {
                    self.capture_view.recovery = Some(entry.path.clone());
                    self.capture_view.page = crate::capture_ui::Page::Recovery;
                    self.playback.active = false;
                }
            } else if action == Action::Exit {
                self.shutdown();
            } else {
                self.playback.dispatch(action);
                self.synchronize_playback();
            }
            return;
        }
        if action == Action::Takes && self.session.transport() == Transport::Stopped {
            if !self.capture.destination.as_os_str().is_empty() {
                self.playback.use_library(self.capture.destination.clone());
            }
            self.playback.active = true;
            self.playback.dispatch(Action::Refresh);
            return;
        }
        if self.session.exit_confirmation() {
            match action {
                Action::Confirm => {
                    self.session.confirm_exit();
                    self.capture.stop(&mut self.session);
                }
                Action::Cancel => self.session.cancel_exit(),
                _ => {}
            }
            return;
        }
        if self.capture_view.page == crate::capture_ui::Page::Recovery {
            match action {
                Action::Confirm => {
                    if let Some(path) = self.capture_view.recovery.take() {
                        if let Err(e) = self.capture.recover(path, &mut self.session) {
                            self.status = format!("{e:#}");
                        } else {
                            self.status = self.capture.status.clone();
                        }
                    }
                    self.capture_view.page = crate::capture_ui::Page::Details;
                }
                Action::Cancel => {
                    self.capture_view.page = crate::capture_ui::Page::Inputs;
                    self.playback.active = true;
                }
                Action::Exit => {
                    self.session.request_exit();
                }
                _ => {}
            }
            return;
        }
        if action == Action::Inputs && self.session.transport() == Transport::Stopped {
            self.capture_view.page = if self.capture_view.page == crate::capture_ui::Page::Setup {
                crate::capture_ui::Page::Inputs
            } else {
                crate::capture_ui::Page::Setup
            };
            return;
        }
        if action == Action::Details
            || self.capture_view.page == crate::capture_ui::Page::Details
            || !self.capture.busy()
        {
            match self.capture_view.edit(
                action,
                &mut self.capture,
                &mut self.config,
                &mut self.selected,
                &mut self.status,
            ) {
                Ok(true) => return,
                Ok(false) => {}
                Err(e) => {
                    self.status = format!("{e:#}");
                    return;
                }
            }
        }
        match action {
            Action::Previous => self.selected = self.selected.saturating_sub(1),
            Action::Next => self.selected = (self.selected + 1).min(self.config.tracks.len() - 1),
            Action::Select(index) if index < self.config.tracks.len() => self.selected = index,
            Action::Arm if self.session.transport() == Transport::Stopped => {
                self.config.tracks[self.selected].armed ^= true;
                self.status = "Arm changes last for this run; SAVE in SETUP".into();
            }
            Action::Record => match self.capture.start(&self.config, &mut self.session) {
                Ok(()) => {
                    self.status = self.capture.status.clone();
                    self.capture_view.page = crate::capture_ui::Page::Inputs;
                }
                Err(e) => self.status = format!("{e:#}"),
            },
            Action::Play if self.session.transport() == Transport::Stopped => {
                self.playback.active = true;
                self.playback.dispatch(Action::Refresh);
            }
            Action::Stop => {
                self.capture.stop(&mut self.session);
            }
            Action::Exit => {
                self.session.request_exit();
            }
            _ => {}
        }
    }

    fn first_track(&self) -> usize {
        self.selected.saturating_sub(4)
    }

    pub fn event(&self, event: Event, size: Rect) -> Option<Action> {
        if self.playback.active {
            return match event {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    if key.modifiers.contains(KeyModifiers::CONTROL)
                        && key.code == KeyCode::Char('c')
                    {
                        return Some(Action::Exit);
                    }
                    match key.code {
                        KeyCode::Up => Some(Action::Previous),
                        KeyCode::Down => Some(Action::Next),
                        KeyCode::Char('q') => Some(Action::Exit),
                        KeyCode::Esc => Some(Action::Cancel),
                        KeyCode::Enter => Some(match self.playback.page {
                            crate::playback_ui::Page::Takes => Action::Open,
                            crate::playback_ui::Page::Damaged => Action::Confirm,
                            _ => Action::Play,
                        }),
                        KeyCode::Char('p' | ' ') => Some(Action::Play),
                        KeyCode::Char('e') => Some(Action::Details),
                        KeyCode::Char('s') => Some(Action::Stop),
                        KeyCode::Left => Some(Action::SeekBack),
                        KeyCode::Right => Some(Action::SeekForward),
                        KeyCode::Char('1'..='8') => {
                            let KeyCode::Char(c) = key.code else {
                                unreachable!()
                            };
                            let button = self.playback.buttons()[c as usize - '1' as usize];
                            (!button.label.is_empty()).then_some(button.action)
                        }
                        _ => None,
                    }
                }
                Event::Mouse(mouse) if mouse.kind == MouseEventKind::Down(MouseButton::Left) => {
                    if size.width < 40 || size.height < 13 {
                        return (mouse.row == size.height.saturating_sub(1))
                            .then_some(Action::Exit);
                    }
                    if mouse.column >= 40 {
                        return None;
                    }
                    if mouse.row == 12 || (mouse.row == 0 && self.playback.details) {
                        return Some(Action::Details);
                    }
                    if (10..12).contains(&mouse.row) {
                        let b = self.playback.buttons()
                            [usize::from((mouse.row - 10) * 4 + mouse.column / 10)];
                        return (!b.label.is_empty()).then_some(b.action);
                    }
                    if self.playback.page == crate::playback_ui::Page::Takes
                        && (2..7).contains(&mouse.row)
                    {
                        return Some(Action::Select(
                            self.playback.first() + usize::from(mouse.row - 2),
                        ));
                    }
                    None
                }
                _ => None,
            };
        }
        match event {
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                if self.session.exit_confirmation() {
                    return match key.code {
                        KeyCode::Enter => Some(Action::Confirm),
                        KeyCode::Esc | KeyCode::Char('c') => Some(Action::Cancel),
                        _ => None,
                    };
                }
                if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                    return Some(Action::Exit);
                }
                match key.code {
                    KeyCode::Up | KeyCode::Char('k') => Some(Action::Previous),
                    KeyCode::Down | KeyCode::Char('j') => Some(Action::Next),
                    KeyCode::Enter
                        if self.capture_view.page == crate::capture_ui::Page::Recovery =>
                    {
                        Some(Action::Confirm)
                    }
                    KeyCode::Enter
                        if self.capture_view.page == crate::capture_ui::Page::Source
                            || self.capture_view.page == crate::capture_ui::Page::Destination =>
                    {
                        Some(Action::Apply)
                    }
                    KeyCode::Esc if self.capture_view.page != crate::capture_ui::Page::Inputs => {
                        Some(Action::Cancel)
                    }
                    KeyCode::Char(' ') | KeyCode::Enter => Some(Action::Arm),
                    KeyCode::Char('r') => Some(Action::Record),
                    KeyCode::Char('i') => Some(Action::Inputs),
                    KeyCode::Char('e') => Some(Action::Details),
                    KeyCode::Char('1'..='8') => {
                        let KeyCode::Char(c) = key.code else {
                            unreachable!()
                        };
                        let b = self.capture_view.buttons()[c as usize - '1' as usize];
                        (!b.label.is_empty()).then_some(b.action)
                    }
                    KeyCode::Char('p') => Some(Action::Play),
                    KeyCode::Char('s') => Some(Action::Stop),
                    KeyCode::Esc | KeyCode::Char('q') => Some(Action::Exit),
                    _ => None,
                }
            }
            Event::Mouse(mouse) if mouse.kind == MouseEventKind::Down(MouseButton::Left) => {
                if size.width < 40 || size.height < 13 {
                    return (mouse.row == size.height.saturating_sub(1)).then_some(Action::Exit);
                }
                if mouse.column >= 40 {
                    return None;
                }
                if self.session.exit_confirmation() {
                    return (10..12)
                        .contains(&mouse.row)
                        .then_some(if mouse.column < 20 {
                            Action::Confirm
                        } else {
                            Action::Cancel
                        });
                }
                if mouse.row == 12 {
                    return Some(Action::Details);
                }
                if mouse.row == 0 {
                    return Some(
                        if self.capture_view.page == crate::capture_ui::Page::Details {
                            Action::Details
                        } else {
                            Action::Inputs
                        },
                    );
                }
                if (10..12).contains(&mouse.row) {
                    let index = usize::from((mouse.row - 10) * 4 + mouse.column / 10);
                    let b = self.capture_view.buttons()[index];
                    return (!b.label.is_empty()).then_some(b.action);
                }
                if self.capture_view.page == crate::capture_ui::Page::Inputs
                    && (2..7).contains(&mouse.row)
                {
                    let index = self.first_track() + usize::from(mouse.row - 2);
                    return (index < self.config.tracks.len()).then_some(Action::Select(index));
                }
                None
            }
            _ => None,
        }
    }

    pub fn draw<B: Backend>(&self, frame: &mut Frame<B>) {
        let size = frame.size();
        if size.width < 40 || size.height < 13 {
            frame.render_widget(Paragraph::new("shr-rec needs 40x13"), size);
            if size.height > 0 {
                frame.render_widget(
                    Paragraph::new("EXIT"),
                    Rect::new(0, size.height - 1, size.width, 1),
                );
            }
            return;
        }
        if self.playback.active {
            self.playback.draw(frame);
            return;
        }
        if self.capture_view.page != crate::capture_ui::Page::Inputs
            && !self.session.exit_confirmation()
        {
            self.capture_view.draw(
                frame,
                &self.capture,
                &self.config,
                self.selected,
                &self.status,
                self.session.transport(),
            );
            return;
        }
        let build = if cfg!(debug_assertions) { "DEV" } else { "REL" };
        line(
            frame,
            0,
            &format!("shr-rec  {build}   INPUT SETUP"),
            Style::default(),
        );
        line(
            frame,
            1,
            "    CHANNEL                   SOURCE",
            Style::default(),
        );
        for (row, index) in (self.first_track()..self.config.tracks.len())
            .take(5)
            .enumerate()
        {
            let track = &self.config.tracks[index];
            let route = crate::capture::resolution(track, &self.capture.sources());
            let text = format!(
                "{} {} {:2} {}",
                if index == self.selected { ">" } else { " " },
                if track.armed { "●" } else { "○" },
                index + 1,
                track.label
            );
            let style = if index == self.selected {
                Style::default().fg(Color::Black).bg(Color::White)
            } else {
                Style::default()
            };
            frame.render_widget(
                Paragraph::new(text).style(style),
                Rect::new(0, 2 + row as u16, 30, 1),
            );
            frame.render_widget(
                Paragraph::new(route).style(style),
                Rect::new(30, 2 + row as u16, 10, 1),
            );
        }
        line(
            frame,
            7,
            &format!(
                "Channel {} / {}",
                self.selected + 1,
                self.config.tracks.len()
            ),
            Style::default(),
        );
        if self.session.transport() == Transport::Recording {
            line(frame, 7, "", Style::default());
            let bank = self.selected / 18 * 18;
            for (index, track) in self.config.tracks.iter().enumerate().skip(bank).take(18) {
                let armed_index = self.config.tracks[..index]
                    .iter()
                    .filter(|t| t.armed)
                    .count();
                let peak = if track.armed {
                    self.capture.peak(armed_index)
                } else {
                    0.
                };
                let color = if peak >= 0.99 {
                    Color::Red
                } else if peak >= 0.5 {
                    Color::Yellow
                } else if peak > 0.001 {
                    Color::Green
                } else {
                    Color::DarkGray
                };
                frame.render_widget(
                    Paragraph::new(if track.armed { "●" } else { "○" })
                        .style(Style::default().fg(color)),
                    Rect::new(((index - bank) * 2) as u16, 7, 2, 1),
                );
            }
        }
        line(frame, 8, &self.capture.progress(), Style::default());
        line(
            frame,
            9,
            &self.capture.limit(&self.config),
            Style::default(),
        );
        if self.session.exit_confirmation() {
            line(
                frame,
                8,
                "Exit: stop and finalize this take?",
                Style::default(),
            );
            for (index, button) in CONFIRM.iter().enumerate() {
                frame.render_widget(
                    Paragraph::new(button.label).style(Style::default().fg(Color::Yellow)),
                    Rect::new(index as u16 * 20, 10, 20, 2),
                );
            }
        } else {
            for (index, button) in self.capture_view.buttons().iter().enumerate() {
                frame.render_widget(
                    Paragraph::new(button.label).style(Style::default().fg(Color::Cyan)),
                    Rect::new((index as u16 % 4) * 10, 10 + index as u16 / 4, 10, 1),
                );
            }
        }
        let symbol = match self.session.transport() {
            Transport::Recording => "●",
            Transport::Playing => ">",
            Transport::Paused => "‖",
            _ => "■",
        };
        line(
            frame,
            12,
            &format!("{symbol} {}", self.status),
            Style::default(),
        );
    }
}

fn line<B: Backend>(frame: &mut Frame<B>, row: u16, text: &str, style: Style) {
    frame.render_widget(Paragraph::new(text).style(style), Rect::new(0, row, 40, 1));
}
