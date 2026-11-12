mod support;
use anyhow::{Result, ensure};
use crossterm::event::{Event, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{Terminal, backend::TestBackend, layout::Rect};
use shr_rec::{
    config::Config,
    jack_playback::Device,
    playback::{self, Prefetch, Render, Routing, Shared, State},
    playback_ui::{Page, PlaybackUi},
    session::Transport,
    take::Take,
    ui::{Action, App},
};
use std::{cell::RefCell, fs, rc::Rc, sync::Arc};
use support::*;
#[derive(Default)]
struct Evidence {
    starts: usize,
    stops: usize,
    positions: Vec<u64>,
    samples: Vec<[f32; 2]>,
    fail: bool,
    rate: u32,
}
struct NullDevice {
    render: Option<Render>,
    worker: Option<Prefetch>,
    evidence: Rc<RefCell<Evidence>>,
}
impl Device for NullDevice {
    fn outputs(&mut self) -> Result<Vec<String>> {
        Ok(vec![
            "sink:out-0".into(),
            "sink:out-1".into(),
            "sink:out-2".into(),
        ])
    }
    fn start(
        &mut self,
        take: Take,
        routing: &Routing,
        damaged: bool,
        start: u64,
    ) -> Result<Arc<Shared>> {
        ensure!(!self.evidence.borrow().fail, "synthetic device unavailable");
        routing.validate(take.stems.len())?;
        let (render, worker) =
            playback::prepare(take, routing, self.evidence.borrow().rate, damaged, start)?;
        let shared = render.shared.clone();
        self.render = Some(render);
        self.worker = Some(worker);
        self.evidence.borrow_mut().starts += 1;
        Ok(shared)
    }
    fn stop(&mut self) {
        if self.render.is_some() {
            self.evidence.borrow_mut().stops += 1;
        }
        self.render = None;
        self.worker = None;
    }
    fn poll(&mut self) {
        if let Some(r) = &mut self.render {
            let mut a = [0.; 256];
            let mut b = [0.; 256];
            r.process(&mut [&mut a, &mut b]);
            let mut e = self.evidence.borrow_mut();
            e.positions.push(r.shared.position());
            e.samples.push([a[0], b[0]]);
        }
    }
}
fn app(root: &Scratch) -> (App, Rc<RefCell<Evidence>>) {
    let evidence = Rc::new(RefCell::new(Evidence {
        rate: 48000,
        ..Evidence::default()
    }));
    let device = NullDevice {
        render: None,
        worker: None,
        evidence: evidence.clone(),
    };
    let mut app = App::new(Config::default());
    app.playback = PlaybackUi::with_device(root.0.join("config"), root.0.clone(), Box::new(device));
    (app, evidence)
}
fn tap(app: &mut App, x: u16, y: u16) {
    let action = app
        .event(
            Event::Mouse(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: x,
                row: y,
                modifiers: KeyModifiers::NONE,
            }),
            Rect::new(0, 0, 40, 13),
        )
        .unwrap();
    app.dispatch(action);
}
fn row(t: &Terminal<TestBackend>, y: u16) -> String {
    (0..40)
        .map(|x| t.backend().buffer().get(x, y).symbol.as_str())
        .collect()
}
#[test]
fn complete_touch_workflow_exact_routes_play_pause_seek_stop_save_reopen_exit() {
    let root = Scratch::new();
    let path = shr(&root.0, 300000, 2);
    let before = fs::read(path.join("session.json")).unwrap();
    let (mut app, e) = app(&root);
    // Shared shell → takes → open → explicit exact assignment, no input inference.
    tap(&mut app, 25, 11);
    assert!(app.playback.active);
    tap(&mut app, 25, 10);
    assert_eq!(app.playback.page, Page::Transport);
    assert!(app.playback.routing.is_none());
    tap(&mut app, 5, 10);
    assert_eq!(e.borrow().starts, 0);
    assert!(app.playback.status.contains("OUTPUTS"));
    tap(&mut app, 5, 11);
    assert_eq!(app.playback.page, Page::Routes);
    tap(&mut app, 25, 10);
    tap(&mut app, 35, 10);
    tap(&mut app, 15, 10);
    tap(&mut app, 35, 10);
    tap(&mut app, 35, 10);
    tap(&mut app, 15, 11);
    assert!(root.0.join("config/playback.json").exists());
    tap(&mut app, 35, 11);
    assert_eq!(app.playback.page, Page::Transport);
    assert!(matches!(app.playback.routing, Some(Routing::Exact { .. })));
    tap(&mut app, 5, 10);
    assert_eq!(app.session.transport(), Transport::Playing);
    app.tick();
    assert_eq!(app.playback.position, 256);
    assert_eq!(
        e.borrow().samples[0],
        [
            sample(0, 0) as f32 / 8388608.,
            sample(1, 0) as f32 / 8388608.
        ]
    );
    tap(&mut app, 5, 10);
    assert_eq!(app.session.transport(), Transport::Paused);
    app.tick();
    assert_eq!(app.playback.position, 256);
    tap(&mut app, 35, 10);
    assert_eq!(app.playback.position, 240256);
    assert_eq!(app.playback.state(), State::Paused);
    assert_eq!(app.session.transport(), Transport::Paused);
    tap(&mut app, 5, 10);
    app.tick();
    assert_eq!(app.playback.position, 240512);
    assert_eq!(
        e.borrow().samples.last().unwrap()[1],
        sample(1, 240256) as f32 / 8388608.
    );
    tap(&mut app, 25, 10);
    assert_eq!(app.playback.position, 512);
    assert_eq!(app.playback.state(), State::Playing);
    tap(&mut app, 15, 10);
    assert_eq!(app.session.transport(), Transport::Stopped);
    assert_eq!(app.playback.position, 0);
    tap(&mut app, 15, 11);
    tap(&mut app, 25, 10);
    assert_eq!(app.playback.page, Page::Transport);
    assert!(app.playback.routing.is_some());
    tap(&mut app, 5, 10);
    tap(&mut app, 35, 11);
    assert_eq!(app.session.transport(), Transport::Exited);
    assert_eq!(e.borrow().starts, e.borrow().stops);
    assert_eq!(before, fs::read(path.join("session.json")).unwrap());
}
#[test]
fn mix_edits_are_cancellable_and_damaged_selection_is_deliberate() {
    let root = Scratch::new();
    let path = shr(&root.0, 1500, 2);
    edit(&path, |m| m["completeness"] = "incomplete".into());
    let (mut app, e) = app(&root);
    app.dispatch(Action::Takes);
    app.dispatch(Action::Open);
    assert_eq!(app.playback.page, Page::Damaged);
    app.dispatch(Action::Play);
    assert_eq!(e.borrow().starts, 0);
    let mut terminal = Terminal::new(TestBackend::new(40, 13)).unwrap();
    terminal.draw(|f| app.draw(f)).unwrap();
    assert!(row(&terminal, 7).contains("INCOMPLETE"));
    tap(&mut app, 5, 10);
    assert_eq!(app.playback.page, Page::Transport);
    tap(&mut app, 5, 11);
    tap(&mut app, 25, 10);
    tap(&mut app, 25, 10); // Stereo mode
    tap(&mut app, 35, 10);
    tap(&mut app, 15, 10);
    tap(&mut app, 35, 10);
    tap(&mut app, 35, 10);
    tap(&mut app, 5, 11);
    assert_eq!(app.playback.page, Page::Mix);
    tap(&mut app, 25, 10);
    tap(&mut app, 5, 11);
    tap(&mut app, 15, 11);
    if let Some(Routing::Stereo { stems, .. }) = &app.playback.routing {
        assert!(stems[0].mute);
        assert!((stems[0].level - 0.9).abs() < 0.001);
        assert_eq!(stems[0].pan, 0.25);
    } else {
        panic!();
    }
    tap(&mut app, 25, 11);
    assert_eq!(app.playback.page, Page::Routes);
    tap(&mut app, 25, 11);
    assert!(app.playback.routing.is_none());
    app.dispatch(Action::Outputs);
    app.dispatch(Action::Mode);
    app.dispatch(Action::Mode);
    app.dispatch(Action::Assign);
    app.dispatch(Action::Next);
    app.dispatch(Action::Assign);
    app.dispatch(Action::Assign);
    app.dispatch(Action::Apply);
    app.dispatch(Action::Play);
    for _ in 0..6 {
        app.tick();
    }
    assert!(app.playback.status.contains("End of take"));
    assert_eq!(app.session.transport(), Transport::Stopped);
    app.dispatch(Action::Play);
    assert_eq!(app.playback.position, 0);
    app.shutdown();
    assert_eq!(e.borrow().starts, e.borrow().stops);
}
#[test]
fn startup_rate_failure_status_details_and_shutdown_reopen() {
    let root = Scratch::new();
    let path = shr(&root.0, 1500, 2);
    let (mut app, e) = app(&root);
    app.playback.active = true;
    app.playback.open(&path).unwrap();
    app.playback.routing = Some(exact(2));
    e.borrow_mut().fail = true;
    app.dispatch(Action::Play);
    assert!(app.playback.status.contains("unavailable"));
    assert_eq!(app.session.transport(), Transport::Stopped);
    e.borrow_mut().fail = false;
    e.borrow_mut().rate = 44100;
    app.dispatch(Action::Play);
    assert!(app.playback.status.contains("resampling unsupported"));
    tap(&mut app, 0, 12);
    assert!(app.playback.details);
    let mut terminal = Terminal::new(TestBackend::new(40, 13)).unwrap();
    terminal.draw(|f| app.draw(f)).unwrap();
    assert!(row(&terminal, 1).contains("48000"));
    assert!(row(&terminal, 2).contains("unsupported"));
    tap(&mut app, 0, 0);
    assert!(!app.playback.details);
    e.borrow_mut().rate = 48000;
    app.dispatch(Action::Play);
    app.shutdown();
    assert_eq!(e.borrow().starts, e.borrow().stops);
    assert_eq!(app.session.transport(), Transport::Exited);
    let (mut second, e2) = self::app(&root);
    second.playback.active = true;
    second.playback.open(&path).unwrap();
    second.playback.routing = Some(exact(2));
    second.dispatch(Action::Play);
    second.tick();
    assert_eq!(second.playback.position, 256);
    drop(second);
    assert_eq!(e2.borrow().starts, e2.borrow().stops);
}
