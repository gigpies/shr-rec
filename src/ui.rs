use crate::{
    config::{Config, Resolution},
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
}

#[derive(Clone, Copy)]
pub struct Button {
    pub label: &'static str,
    pub action: Action,
}

const NORMAL: [Button; 8] = [
    Button {
        label: "PREV",
        action: Action::Previous,
    },
    Button {
        label: "NEXT",
        action: Action::Next,
    },
    Button {
        label: "ARM",
        action: Action::Arm,
    },
    Button {
        label: "RECORD",
        action: Action::Record,
    },
    Button {
        label: "PLAY",
        action: Action::Play,
    },
    Button {
        label: "STOP",
        action: Action::Stop,
    },
    Button {
        label: "",
        action: Action::Cancel,
    },
    Button {
        label: "EXIT",
        action: Action::Exit,
    },
];
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
}

impl App {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            session: Session::default(),
            selected: 0,
            status: "Audio engine not implemented".into(),
        }
    }

    pub fn dispatch(&mut self, action: Action) {
        if self.session.exit_confirmation() {
            match action {
                Action::Confirm => {
                    self.session.confirm_exit();
                }
                Action::Cancel => self.session.cancel_exit(),
                _ => {}
            }
            return;
        }
        match action {
            Action::Previous => self.selected = self.selected.saturating_sub(1),
            Action::Next => self.selected = (self.selected + 1).min(self.config.tracks.len() - 1),
            Action::Select(index) if index < self.config.tracks.len() => self.selected = index,
            Action::Arm if self.session.transport() == Transport::Stopped => {
                self.config.tracks[self.selected].armed ^= true;
                self.status = "Arm changes last for this run".into();
            }
            Action::Record | Action::Play => self.status = "Audio engine not implemented".into(),
            Action::Stop => {
                self.session.stop();
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
                    KeyCode::Char(' ') | KeyCode::Enter => Some(Action::Arm),
                    KeyCode::Char('r') => Some(Action::Record),
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
                if (10..12).contains(&mouse.row) {
                    let index = usize::from((mouse.row - 10) * 4 + mouse.column / 10);
                    return (!NORMAL[index].label.is_empty()).then_some(NORMAL[index].action);
                }
                if (2..7).contains(&mouse.row) {
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
            let route = match track.resolve(&[]) {
                Resolution::Unassigned => "UNSET",
                _ => "MISSING",
            };
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
        line(
            frame,
            8,
            "Sources and meters await audio engine",
            Style::default(),
        );
        line(
            frame,
            9,
            "Touch a channel; ARM toggles it",
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
            for (index, button) in NORMAL.iter().enumerate() {
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
