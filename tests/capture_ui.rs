mod support;
use anyhow::{Result, ensure};
use crossterm::event::{Event, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{Terminal, backend::TestBackend, layout::Rect};
use shr_rec::{
    capture::Capture,
    capture_ui::Page,
    config::Config,
    jack_playback::{
        self,
        capture::{Device, Inputs, SharedCapture, Source},
    },
    playback::{self, Prefetch, Render, Routing, Shared},
    playback_ui::{self, PlaybackUi},
    recorder::Recorder,
    session::Transport,
    take::Take,
    ui::{Action, App},
};
use std::{
    cell::RefCell,
    fs,
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};
use support::Scratch;
#[derive(Default)]
struct State {
    running: bool,
    fail: bool,
    replaced: bool,
    fault: u32,
    frames: u64,
    samples: Vec<[f32; 2]>,
    channels: usize,
    all_samples: Vec<f32>,
    attached: usize,
    quiesced: usize,
}
struct NullCapture {
    r: Option<Recorder>,
    shared: Option<Arc<SharedCapture>>,
    state: Rc<RefCell<State>>,
}
impl Device for NullCapture {
    fn inputs(&mut self) -> Result<Inputs> {
        Ok(Inputs {
            rate: 48000,
            sources: (0..self.state.borrow().channels.max(2))
                .map(|i| Source {
                    name: format!("synthetic:input-{i}"),
                    uuid: (i + 1) as u64,
                })
                .collect(),
        })
    }
    fn attach(
        &mut self,
        r: Recorder,
        s: &[Source],
        rate: u32,
    ) -> std::result::Result<Arc<SharedCapture>, (Box<Recorder>, String)> {
        let mut state = self.state.borrow_mut();
        assert_eq!(rate, 48000);
        assert_eq!(s.len(), state.channels.max(2));
        for (i, source) in s.iter().enumerate() {
            assert_eq!(source.name, format!("synthetic:input-{i}"));
        }
        if state.fail || state.replaced {
            return Err((Box::new(r), "synthetic missing/replaced source".into()));
        }
        state.attached += 1;
        state.running = true;
        self.r = Some(r);
        let shared = Arc::new(SharedCapture::default());
        self.shared = Some(shared.clone());
        Ok(shared)
    }
    fn poll(&mut self) {
        if let Some(r) = &mut self.r {
            let mut state = self.state.borrow_mut();
            if state.fault != 0 {
                self.shared.as_ref().unwrap().fault(state.fault);
                return;
            }
            let channels = state.channels.max(2);
            let mut samples = [0.; 64 * 32];
            for frame in 0..32 {
                for channel in 0..channels {
                    samples[frame * channels + channel] = if channels == 2 {
                        if channel == 0 { 0.25 } else { -0.5 }
                    } else {
                        (channel + 1) as f64 / 64.
                    };
                }
            }
            assert_eq!(r.push(state.frames, &samples[..channels * 32], 32), 0);
            state.frames += 32;
        }
    }
    fn quiesce(&mut self) -> Option<Recorder> {
        let r = self.r.take();
        if let Some(r) = &r {
            let mut s = self.state.borrow_mut();
            s.running = false;
            s.quiesced += 1;
            if s.fault != 0 {
                r.mark_fault(s.fault);
            }
        }
        self.shared = None;
        r
    }
}
struct NullPlayback {
    r: Option<Render>,
    w: Option<Prefetch>,
    state: Rc<RefCell<State>>,
}
impl jack_playback::Device for NullPlayback {
    fn outputs(&mut self) -> Result<Vec<String>> {
        if self.state.borrow().channels == 0 {
            Ok(vec!["sink:left".into(), "sink:right".into()])
        } else {
            Ok((0..self.state.borrow().channels)
                .map(|i| format!("sink:out-{i}"))
                .collect())
        }
    }
    fn start(
        &mut self,
        take: Take,
        routes: &Routing,
        damaged: bool,
        start: u64,
    ) -> Result<Arc<Shared>> {
        ensure!(!self.state.borrow().running, "capture still owns device");
        let (r, w) = playback::prepare(take, routes, 48000, damaged, start)?;
        let shared = r.shared.clone();
        self.r = Some(r);
        self.w = Some(w);
        Ok(shared)
    }
    fn stop(&mut self) {
        self.r = None;
        self.w = None;
    }
    fn poll(&mut self) {
        if let Some(r) = &mut self.r {
            let mut buffers = vec![[0.; 8]; self.state.borrow().channels.max(2)];
            let mut outputs: Vec<_> = buffers.iter_mut().map(|b| &mut b[..]).collect();
            r.process(&mut outputs);
            let mut state = self.state.borrow_mut();
            state.samples.push([buffers[0][0], buffers[1][0]]);
            state.all_samples = buffers.iter().map(|b| b[0]).collect();
        }
    }
}
fn app(root: &Scratch) -> (App, Rc<RefCell<State>>) {
    let state = Rc::new(RefCell::new(State::default()));
    let mut config = Config::default();
    config.tracks.truncate(2);
    let mut app = App::new(config);
    app.capture = Capture::with_device(
        root.0.join("takes"),
        Box::new(NullCapture {
            r: None,
            shared: None,
            state: state.clone(),
        }),
    );
    app.capture_view.root = root.0.join("private");
    app.playback = PlaybackUi::with_device(
        root.0.join("private"),
        root.0.join("takes"),
        Box::new(NullPlayback {
            r: None,
            w: None,
            state: state.clone(),
        }),
    );
    (app, state)
}
fn tap(app: &mut App, x: u16, y: u16) {
    let a = app
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
    app.dispatch(a);
}
fn until(app: &mut App, t: Transport) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.session.transport() != t {
        assert!(
            Instant::now() < deadline,
            "{:?}: {}",
            app.session.transport(),
            app.status
        );
        app.tick();
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn setup(app: &mut App) {
    tap(app, 5, 11);
    assert_eq!(app.capture_view.page, Page::Setup);
    tap(app, 35, 10); // REFRESH
    for i in 0..2 {
        tap(app, 25, 10); // ASSIGN
        if i == 1 {
            tap(app, 15, 10);
        }
        tap(app, 25, 10); // USE
        if i == 0 {
            tap(app, 15, 10);
        }
    }
    tap(app, 15, 11); // SAVE
    tap(app, 25, 11); // BACK
    tap(app, 25, 10); // ARM 2
    tap(app, 5, 10);
    tap(app, 25, 10); // ARM 1
}
#[test]
fn actual_touch_capture_finalize_discover_prepare_play_reopen_and_private_setup() {
    let root = Scratch::new();
    let (mut app, state) = app(&root);
    setup(&mut app);
    assert!(!root.0.join("takes").exists());
    assert_eq!(state.borrow().attached, 0);
    assert!(root.0.join("private/capture.json").exists());
    let mut terminal = Terminal::new(TestBackend::new(40, 13)).unwrap();
    terminal.draw(|f| app.draw(f)).unwrap();
    tap(&mut app, 35, 10);
    assert_eq!(app.session.transport(), Transport::Starting);
    tap(&mut app, 25, 11);
    assert!(!app.playback.active); // exclusive browser
    until(&mut app, Transport::Recording);
    app.tick();
    app.tick();
    let path = app.capture.path.clone().unwrap();
    assert!(!path.join("result.json").exists());
    tap(&mut app, 15, 11);
    assert_eq!(app.session.transport(), Transport::Finalizing);
    assert!(!state.borrow().running);
    app.dispatch(Action::Record);
    assert_eq!(app.session.transport(), Transport::Finalizing);
    until(&mut app, Transport::Stopped);
    assert!(app.session.retained().unwrap().complete, "{}", app.status);
    assert_eq!(state.borrow().quiesced, 1);
    let take = Take::open(&path).unwrap();
    assert_eq!(take.stems[0].id, "input-1");
    assert_eq!(take.stems[1].label, "Input 2");
    drop(take);
    tap(&mut app, 25, 11);
    assert_eq!(app.playback.entries.len(), 1);
    tap(&mut app, 25, 10);
    assert_eq!(app.playback.page, playback_ui::Page::Transport);
    tap(&mut app, 5, 11); // OUTPUTS
    tap(&mut app, 25, 10);
    tap(&mut app, 35, 10);
    tap(&mut app, 15, 10);
    tap(&mut app, 35, 10);
    tap(&mut app, 35, 10);
    tap(&mut app, 35, 11); // APPLY
    tap(&mut app, 5, 10);
    assert_eq!(app.session.transport(), Transport::Playing);
    app.tick();
    assert_eq!(state.borrow().samples[0], [0.25, -0.5]);
    tap(&mut app, 15, 10);
    assert_eq!(app.session.transport(), Transport::Stopped);
    tap(&mut app, 15, 11);
    tap(&mut app, 25, 10);
    assert_eq!(app.playback.page, playback_ui::Page::Transport);
    let second = support::shr(&root.0.join("takes"), 64, 2);
    app.dispatch(Action::Takes);
    let index = app
        .playback
        .entries
        .iter()
        .position(|e| e.path == second)
        .unwrap();
    app.dispatch(Action::Select(index));
    app.dispatch(Action::Open);
    assert_eq!(app.playback.label, "Synthetic band");
    assert!(app.playback.routing.is_none());
    tap(&mut app, 35, 11);
    assert_eq!(app.session.transport(), Transport::Exited);
}
#[test]
fn startup_failure_preparing_stop_fault_and_cancel_exit_keep_authoritative_outcomes() {
    for mode in 0..4 {
        let root = Scratch::new();
        let (mut app, state) = app(&root);
        setup(&mut app);
        if mode == 0 {
            state.borrow_mut().replaced = true;
        }
        app.dispatch(Action::Record);
        if mode == 1 {
            app.dispatch(Action::Stop);
        }
        if mode >= 2 {
            until(&mut app, Transport::Recording);
            app.tick();
            app.dispatch(Action::Exit);
            assert!(app.session.exit_confirmation());
            app.dispatch(Action::Cancel);
            assert_eq!(app.session.transport(), Transport::Recording);
            if mode == 2 {
                state.borrow_mut().fault = 2;
            } else {
                app.dispatch(Action::Exit);
                app.dispatch(Action::Confirm);
            }
        }
        until(
            &mut app,
            if mode == 3 {
                Transport::Exited
            } else {
                Transport::Stopped
            },
        );
        assert_eq!(
            app.session.retained().unwrap().complete,
            mode == 3,
            "{}",
            app.status
        );
        if mode != 3 {
            assert!(app.status.contains("INCOMPLETE"));
            app.session.play().unwrap();
            app.capture.tick(&mut app.session);
            assert_eq!(
                app.session.transport(),
                Transport::Playing,
                "retired capture must not stop another owner"
            );
            app.session.playback_stopped().unwrap();
        }
        assert!(!state.borrow().running);
    }
}
#[test]
fn interrupted_take_explicit_copy_recovery_preserves_evidence_and_requires_acceptance() {
    let root = Scratch::new();
    let (mut app, _) = app(&root);
    setup(&mut app);
    app.dispatch(Action::Record);
    until(&mut app, Transport::Recording);
    app.tick();
    app.dispatch(Action::Stop);
    until(&mut app, Transport::Stopped);
    let source = app.capture.path.clone().unwrap();
    fs::remove_file(source.join("result.json")).unwrap();
    fs::rename(
        source.join("channel-001.wav"),
        source.join("channel-001.wav.part"),
    )
    .unwrap();
    let before: Vec<_> = fs::read_dir(&source)
        .unwrap()
        .map(|e| {
            let p = e.unwrap().path();
            (p.clone(), fs::read(p).unwrap())
        })
        .collect();
    tap(&mut app, 25, 11);
    tap(&mut app, 5, 11);
    assert_eq!(app.capture_view.page, Page::Recovery);
    assert!(!app.capture.busy());
    tap(&mut app, 15, 10);
    assert!(app.playback.active); // cancel
    tap(&mut app, 5, 11);
    tap(&mut app, 5, 10);
    assert_eq!(app.session.transport(), Transport::Finalizing);
    until(&mut app, Transport::Stopped);
    assert!(!app.session.retained().unwrap().complete);
    let recovered = app.capture.path.clone().unwrap();
    let take = Take::open(&recovered).unwrap();
    assert!(take.warning.is_some());
    assert_eq!(take.stems[0].id, "input-1");
    drop(take);
    for (p, b) in before {
        assert_eq!(fs::read(p).unwrap(), b);
    }
    app.dispatch(Action::Takes);
    app.playback.open(&recovered).unwrap();
    assert_eq!(app.playback.page, playback_ui::Page::Damaged);
}
#[test]
fn destination_picker_and_source_edits_cancel_without_rewriting_preferences() {
    let root = Scratch::new();
    fs::create_dir(root.0.join("other")).unwrap();
    let (mut app, _) = app(&root);
    app.dispatch(Action::Inputs);
    app.dispatch(Action::Refresh);
    let old = app.capture.destination.clone();
    app.dispatch(Action::Destination);
    assert_eq!(app.capture_view.page, Page::Destination);
    app.dispatch(Action::Cancel);
    assert_eq!(app.capture.destination, old);
    app.dispatch(Action::Destination);
    app.dispatch(Action::Apply);
    assert_eq!(app.capture.destination, root.0);
    app.dispatch(Action::Assign);
    app.dispatch(Action::Cancel);
    assert!(app.config.tracks[0].preferred_source.is_none());
    app.config.tracks[0].preferred_source = Some("missing:remembered".into());
    app.dispatch(Action::Refresh);
    assert_eq!(
        app.config.tracks[0].preferred_source.as_deref(),
        Some("missing:remembered")
    );
}

#[test]
fn writer_startup_and_publication_failures_never_announce_complete_and_allow_retry() {
    for failure in 0..2 {
        let root = Scratch::new();
        let (mut app, state) = app(&root);
        setup(&mut app);
        if failure == 0 {
            fs::write(root.0.join("takes"), b"not a directory").unwrap();
        }
        app.dispatch(Action::Record);
        if failure == 1 {
            until(&mut app, Transport::Recording);
            app.tick();
            let path = app.capture.path.clone().unwrap();
            fs::write(path.join("channel-002.wav"), b"collision evidence").unwrap();
            app.dispatch(Action::Exit);
            app.dispatch(Action::Confirm);
        }
        until(&mut app, Transport::Stopped);
        assert!(!app.session.retained().unwrap().complete, "{}", app.status);
        assert!(app.status.contains("INCOMPLETE"));
        assert!(!state.borrow().running);
        if failure == 0 {
            assert_eq!(state.borrow().attached, 0);
            fs::remove_file(root.0.join("takes")).unwrap();
        } else {
            let path = app.capture.path.clone().unwrap();
            assert_eq!(
                fs::read(path.join("channel-002.wav")).unwrap(),
                b"collision evidence"
            );
            assert!(path.join("failure.json").exists());
        }
        state.borrow_mut().frames = 0;
        app.dispatch(Action::Record);
        until(&mut app, Transport::Recording);
        app.tick();
        app.dispatch(Action::Stop);
        until(&mut app, Transport::Stopped);
        assert!(app.session.retained().unwrap().complete, "{}", app.status);
    }
}

#[test]
fn signal_style_shutdown_and_drop_join_the_producer_and_writer() {
    for preparing in [true, false] {
        let root = Scratch::new();
        let (mut app, state) = app(&root);
        setup(&mut app);
        app.dispatch(Action::Record);
        if !preparing {
            until(&mut app, Transport::Recording);
            app.tick();
        }
        app.shutdown();
        until(
            &mut app,
            if preparing {
                Transport::Stopped
            } else {
                Transport::Exited
            },
        );
        assert!(!state.borrow().running);
    }
    let root = Scratch::new();
    let (mut app, state) = app(&root);
    setup(&mut app);
    app.dispatch(Action::Record);
    until(&mut app, Transport::Recording);
    app.tick();
    let path = app.capture.path.clone().unwrap();
    drop(app);
    assert!(!state.borrow().running);
    let take = Take::open(&path).unwrap();
    assert!(take.warning.is_some());
}

#[test]
fn eighteen_distinguishable_channels_follow_touch_assignment_to_capture_and_player() {
    let root = Scratch::new();
    let (mut app, state) = app(&root);
    state.borrow_mut().channels = 18;
    app.config = Config::default();
    tap(&mut app, 5, 11);
    tap(&mut app, 35, 10);
    for channel in 0..18 {
        tap(&mut app, 25, 10);
        for _ in 0..channel {
            tap(&mut app, 15, 10);
        }
        tap(&mut app, 25, 10);
        if channel != 17 {
            tap(&mut app, 15, 10);
        }
    }
    tap(&mut app, 25, 11);
    for channel in (0..18).rev() {
        tap(&mut app, 25, 10);
        if channel != 0 {
            tap(&mut app, 5, 10);
        }
    }
    tap(&mut app, 35, 10);
    until(&mut app, Transport::Recording);
    app.tick();
    let mut terminal = Terminal::new(TestBackend::new(40, 13)).unwrap();
    terminal.draw(|f| app.draw(f)).unwrap();
    let meter_row: String = (0..40)
        .map(|x| terminal.backend().buffer().get(x, 7).symbol.as_str())
        .collect();
    assert_eq!(meter_row.matches('●').count(), 18);
    tap(&mut app, 15, 11);
    until(&mut app, Transport::Stopped);
    assert!(app.session.retained().unwrap().complete, "{}", app.status);
    tap(&mut app, 25, 11);
    tap(&mut app, 25, 10);
    assert_eq!(app.playback.ids.len(), 18);
    tap(&mut app, 5, 11);
    tap(&mut app, 25, 10); // deliberate exact output mode
    for channel in 0..18 {
        for _ in 0..=channel {
            tap(&mut app, 35, 10);
        }
        if channel != 17 {
            tap(&mut app, 15, 10);
        }
    }
    tap(&mut app, 35, 11);
    tap(&mut app, 5, 10);
    assert_eq!(
        app.session.transport(),
        Transport::Playing,
        "{}: {:?}",
        app.playback.status,
        app.playback.routing
    );
    app.tick();
    assert_eq!(
        state.borrow().all_samples,
        (1..=18).map(|i| i as f32 / 64.).collect::<Vec<_>>()
    );
    tap(&mut app, 15, 10);
}

#[test]
fn standalone_recovery_rejects_unsafe_or_newer_identity_before_creating_destination() {
    use std::os::unix::fs::symlink;
    for malformed in 0..3 {
        let root = Scratch::new();
        let (mut app, _) = app(&root);
        setup(&mut app);
        app.dispatch(Action::Record);
        until(&mut app, Transport::Recording);
        app.tick();
        app.dispatch(Action::Stop);
        until(&mut app, Transport::Stopped);
        let source = app.capture.path.clone().unwrap();
        let metadata = source.join("capture.json");
        if malformed == 2 {
            let outside = root.0.join("outside.json");
            fs::rename(&metadata, &outside).unwrap();
            symlink(&outside, &metadata).unwrap();
        } else {
            let mut value: serde_json::Value =
                serde_json::from_slice(&fs::read(&metadata).unwrap()).unwrap();
            if malformed == 0 {
                value["version"] = 2.into();
            } else {
                value["tracks"][1]["id"] = value["tracks"][0]["id"].clone();
            }
            fs::write(&metadata, serde_json::to_vec(&value).unwrap()).unwrap();
        }
        assert!(Take::open(&source).is_err());
        app.dispatch(Action::Takes);
        app.dispatch(Action::Recover);
        app.dispatch(Action::Confirm);
        until(&mut app, Transport::Stopped);
        assert!(!app.session.retained().unwrap().complete);
        assert!(!app.capture.path.as_ref().unwrap().exists());
        assert!(source.exists());
    }
}

#[test]
fn private_capture_preferences_round_trip_without_discovery_and_reject_stale_or_unsafe_files() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let root = Scratch::new();
    let (mut app, state) = app(&root);
    setup(&mut app);
    app.dispatch(Action::Inputs);
    app.dispatch(Action::Save);
    let mut restored = Capture::new(root.0.join("unused"));
    let mut config = Config::default();
    restored.load(&app.capture_view.root, &mut config).unwrap();
    assert!(restored.inputs.is_none());
    assert_eq!(state.borrow().attached, 0);
    assert!(config.tracks.iter().all(|t| t.armed));
    assert_eq!(
        config.tracks[1].preferred_source.as_deref(),
        Some("synthetic:input-1")
    );
    assert_eq!(restored.destination, app.capture.destination);
    let path = app.capture_view.root.join("capture.json");
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let before = fs::read(&path).unwrap();
    fs::write(
        app.capture_view.root.join(".capture.json.part"),
        b"retain temporary evidence",
    )
    .unwrap();
    app.dispatch(Action::Save);
    assert_eq!(fs::read(&path).unwrap(), before);
    assert!(app.status.contains("stale"));
    fs::remove_file(&path).unwrap();
    symlink(root.0.join("absent"), &path).unwrap();
    assert!(restored.load(&app.capture_view.root, &mut config).is_err());
}
